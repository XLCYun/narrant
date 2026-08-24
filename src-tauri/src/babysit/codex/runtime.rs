//! Codex 懒运行时状态与应用设置 adapter。
//!
//! Runtime 拥有 discovery 与 provider 初始化周围的小状态机，
//! 不向前端或诊断暴露可执行文件路径。

use super::discovery::CodexExecutableDiscovery;
use super::CodexAppServerProvider;
use crate::application_settings::ApplicationSettingsApplication;
use crate::babysit::{
    codex_setup_error, BabysitAgentProvider, BabysitError, BabysitProviderRuntimeStatus,
    BabysitProviderSetupFailure, InstalledProviderRuntime, CODEX_PROVIDER_ID,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

/// 用户可见的设置状态；契约不包含最低版本号。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CodexRuntimeStatus {
    #[default]
    Unknown,
    Probing,
    Ready,
    Missing,
    SignedOut,
    Incompatible,
    InitFailed,
}

/// 将 Babysit runtime 的 Codex 偏好映射到统一 Application Settings 契约。
struct ApplicationCodexSettings {
    application: ApplicationSettingsApplication,
}

impl ApplicationCodexSettings {
    /// 读取权威偏好；配置损坏必须显式失败，不能降级为自动发现。
    fn load(&self) -> Result<Option<PathBuf>, BabysitError> {
        self.application
            .load()
            .map(|settings| settings.codex_executable)
            .map_err(|_| codex_setup_error(BabysitProviderSetupFailure::ExecutableInvalid))
    }

    /// 通过统一 Application Settings 边界保存 executable 偏好。
    async fn save(&self, executable: Option<PathBuf>) -> Result<Option<PathBuf>, BabysitError> {
        self.application
            .update_codex_executable(executable)
            .await
            .map(|settings| settings.codex_executable)
            .map_err(|_| codex_setup_error(BabysitProviderSetupFailure::InitializationFailed))
    }
}

/// 进程内懒运行时，供当前所有 File Change 共享。
pub(crate) struct CodexRuntime {
    provider: Arc<CodexAppServerProvider>,
    discovery: CodexExecutableDiscovery,
    settings: ApplicationCodexSettings,
    status: Mutex<CodexRuntimeStatus>,
    lifecycle: AsyncMutex<()>,
}

impl CodexRuntime {
    /// 每次新分析读取权威模型偏好，并用新鲜目录解析实际模型与思考强度。
    pub(crate) async fn prepare_analysis(
        &self,
        workspace_path: &Path,
    ) -> Result<(super::ResolvedCodexModel, tokio::sync::MutexGuard<'_, ()>), BabysitError> {
        let lifecycle = self.lifecycle.lock().await;
        let preference = self
            .settings
            .application
            .load()
            .map_err(|_| BabysitError::ModelPreferenceInvalid)?
            .babysit_model;
        let catalog = self.model_catalog_once().await?;
        self.ensure_ready_once(workspace_path).await?;
        let resolved = catalog
            .resolve(&preference)
            .map_err(|_| BabysitError::ModelPreferenceInvalid)?;
        Ok((resolved, lifecycle))
    }

    /// 无需 Review Workspace 地发现 executable，并读取只依赖 initialize/account 的模型目录。
    pub(crate) async fn model_catalog(&self) -> Result<super::CodexModelCatalog, BabysitError> {
        let _lifecycle = self.lifecycle.lock().await;
        self.model_catalog_once().await
    }

    /// 在持有 lifecycle 锁时发现 executable 并读取目录。
    async fn model_catalog_once(&self) -> Result<super::CodexModelCatalog, BabysitError> {
        let persisted = self.settings.load()?;
        let candidates = self.discovery.resolve_all(persisted.as_deref()).await?;
        let mut last_error = codex_setup_error(BabysitProviderSetupFailure::NotFound);
        for candidate in candidates {
            let candidate_path = candidate.path;
            if self.provider.activate_executable(candidate_path).await? {
                if let Ok(mut status) = self.status.lock() {
                    *status = CodexRuntimeStatus::Unknown;
                }
            }
            let generation_before = self.provider.process_generation();
            match self.provider.model_catalog().await {
                Ok(catalog) => {
                    if self.provider.process_generation() != generation_before {
                        if let Ok(mut status) = self.status.lock() {
                            *status = CodexRuntimeStatus::Unknown;
                        }
                    }
                    return Ok(catalog);
                }
                Err(error) => {
                    if let Ok(mut status) = self.status.lock() {
                        *status = status_for_error(&error);
                    }
                    last_error = error;
                }
            }
        }
        Err(last_error)
    }

    /// 构造 idle runtime；此处不执行设置、PATH 或进程 I/O。
    pub(crate) fn new(provider: Arc<CodexAppServerProvider>, settings_path: PathBuf) -> Self {
        Self::new_with_discovery(provider, settings_path, CodexExecutableDiscovery::default())
    }

    /// 使用显式 discovery 构造 runtime，协议测试通过它替换 executable 候选。
    fn new_with_discovery(
        provider: Arc<CodexAppServerProvider>,
        settings_path: PathBuf,
        discovery: CodexExecutableDiscovery,
    ) -> Self {
        Self {
            provider,
            discovery,
            settings: ApplicationCodexSettings {
                application: ApplicationSettingsApplication::new(
                    settings_path.clone(),
                    settings_path.with_extension("legacy.json"),
                ),
            },
            status: Mutex::new(CodexRuntimeStatus::Unknown),
            lifecycle: AsyncMutex::new(()),
        }
    }

    /// 返回当前 typed 状态，但不暴露选择的路径。
    pub(crate) fn status(&self) -> CodexRuntimeStatus {
        self.status
            .lock()
            .map(|status| *status)
            .unwrap_or(CodexRuntimeStatus::InitFailed)
    }

    /// 并发调用由 lifecycle 锁串行；已 Ready 的同一进程不重复探测。
    pub(crate) async fn ensure_ready(&self, workspace_path: &Path) -> Result<(), BabysitError> {
        let _lifecycle = self.lifecycle.lock().await;
        self.ensure_ready_once(workspace_path).await
    }

    /// 在持有 lifecycle 锁时完成一次 readiness 状态转换。
    async fn ensure_ready_once(&self, workspace_path: &Path) -> Result<(), BabysitError> {
        if self.status() == CodexRuntimeStatus::Ready {
            return Ok(());
        }
        if let Ok(mut status) = self.status.lock() {
            *status = CodexRuntimeStatus::Probing;
        }
        let result = self.probe_once(workspace_path).await;
        if let Ok(mut status) = self.status.lock() {
            *status = result
                .as_ref()
                .map(|_| CodexRuntimeStatus::Ready)
                .unwrap_or_else(|error| status_for_error(error));
        }
        result
    }

    /// 通过生产设置边界验证并保存显式可执行文件偏好。
    pub(crate) async fn select_executable(&self, path: &Path) -> Result<(), BabysitError> {
        let selected = self.discovery.validate_selected(path.to_path_buf()).await?;
        let resolved = self
            .settings
            .save(Some(selected.path))
            .await?
            .ok_or_else(|| codex_setup_error(BabysitProviderSetupFailure::InitializationFailed))?;
        self.provider.activate_executable(resolved).await?;
        if let Ok(mut status) = self.status.lock() {
            *status = CodexRuntimeStatus::Unknown;
        }
        Ok(())
    }

    /// 测试中清空显式偏好，使下一次分析重新执行自动发现。
    #[cfg(all(test, unix))]
    pub(crate) async fn clear_executable(&self) -> Result<(), BabysitError> {
        self.settings.save(None).await?;
        if let Ok(mut status) = self.status.lock() {
            *status = CodexRuntimeStatus::Unknown;
        }
        Ok(())
    }

    /// 应用退出时释放 provider 进程，并让下一次显式启用重新完成 discovery。
    pub(crate) async fn shutdown(&self) {
        let _lifecycle = self.lifecycle.lock().await;
        self.provider.shutdown().await;
        if let Ok(mut status) = self.status.lock() {
            *status = CodexRuntimeStatus::Unknown;
        }
    }

    /// 解析持久化/PATH 候选，配置 #74，并在同一工作区完成握手与能力探针。
    async fn probe_once(&self, workspace_path: &Path) -> Result<(), BabysitError> {
        let persisted = self.settings.load()?;
        let candidates = self.discovery.resolve_all(persisted.as_deref()).await?;
        let mut last_error = codex_setup_error(BabysitProviderSetupFailure::NotFound);
        let mut found_signed_out = false;
        for candidate in candidates {
            let candidate_path = candidate.path;
            self.provider.activate_executable(candidate_path).await?;
            match self.provider.prepare_process(workspace_path).await {
                Ok(()) => return Ok(()),
                Err(BabysitError::ProviderSetup {
                    provider_id,
                    failure: BabysitProviderSetupFailure::SignedOut,
                }) if provider_id == CODEX_PROVIDER_ID => found_signed_out = true,
                Err(error) => last_error = error,
            }
        }
        if found_signed_out {
            Err(codex_setup_error(BabysitProviderSetupFailure::SignedOut))
        } else {
            Err(last_error)
        }
    }
}

#[async_trait::async_trait]
impl InstalledProviderRuntime for CodexRuntime {
    /// 返回 Codex runtime 的稳定 provider identity。
    fn provider_id(&self) -> &str {
        CODEX_PROVIDER_ID
    }

    /// 将 Codex 内部状态投影到 provider-independent runtime status。
    fn status(&self) -> BabysitProviderRuntimeStatus {
        match CodexRuntime::status(self) {
            CodexRuntimeStatus::Unknown => BabysitProviderRuntimeStatus::Unknown,
            CodexRuntimeStatus::Probing => BabysitProviderRuntimeStatus::Probing,
            CodexRuntimeStatus::Ready => BabysitProviderRuntimeStatus::Ready,
            CodexRuntimeStatus::Missing => BabysitProviderRuntimeStatus::Missing,
            CodexRuntimeStatus::SignedOut => BabysitProviderRuntimeStatus::SignedOut,
            CodexRuntimeStatus::Incompatible => BabysitProviderRuntimeStatus::Incompatible,
            CodexRuntimeStatus::InitFailed => BabysitProviderRuntimeStatus::InitFailed,
        }
    }

    /// 复用 Codex 的 settings、模型目录和 lifecycle readiness 流程。
    async fn ensure_ready(&self, workspace_path: &Path) -> Result<(), BabysitError> {
        CodexRuntime::ensure_ready(self, workspace_path).await
    }

    /// 复用 Codex 的 executable 验证与统一设置持久化流程。
    async fn select_executable(&self, path: &Path) -> Result<(), BabysitError> {
        CodexRuntime::select_executable(self, path).await
    }

    /// 复用 Codex provider shutdown，并重置 runtime 状态。
    async fn shutdown(&self) {
        CodexRuntime::shutdown(self).await;
    }
}

/// 保持状态转换表小而且与 provider 无关。
fn status_for_error(error: &BabysitError) -> CodexRuntimeStatus {
    match error {
        BabysitError::ProviderSetup {
            provider_id,
            failure:
                BabysitProviderSetupFailure::NotFound | BabysitProviderSetupFailure::ExecutableInvalid,
        } if provider_id == CODEX_PROVIDER_ID => CodexRuntimeStatus::Missing,
        BabysitError::ProviderSetup {
            provider_id,
            failure: BabysitProviderSetupFailure::SignedOut,
        } if provider_id == CODEX_PROVIDER_ID => CodexRuntimeStatus::SignedOut,
        BabysitError::ProviderSetup {
            provider_id,
            failure: BabysitProviderSetupFailure::Incompatible,
        } if provider_id == CODEX_PROVIDER_ID => CodexRuntimeStatus::Incompatible,
        _ => CodexRuntimeStatus::InitFailed,
    }
}

#[cfg(all(test, unix))]
#[path = "../tests/codex/runtime.rs"]
mod tests;
