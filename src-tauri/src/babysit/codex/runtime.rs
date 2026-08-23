//! Codex 懒运行时状态与应用设置 adapter。
//!
//! Runtime 拥有 discovery 与 provider 初始化周围的小状态机，
//! 不向前端或诊断暴露可执行文件路径。

use super::discovery::CodexExecutableDiscovery;
use super::CodexAppServerProvider;
use crate::application_settings::ApplicationSettingsApplication;
use crate::babysit::{BabysitAgentProvider, BabysitError};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tokio::sync::{Mutex as AsyncMutex, Notify};

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
    /// 读取绝对路径选择；格式错误或相对路径会被忽略。
    fn load(&self) -> Option<PathBuf> {
        self.application
            .load()
            .ok()
            .and_then(|settings| settings.codex_executable)
            .filter(|path| path.is_absolute())
    }

    /// 通过唯一 Application Settings 契约持久化可执行文件偏好。
    fn save(&self, executable: PathBuf) -> Result<(), BabysitError> {
        self.application
            .update_codex_executable(Some(executable))
            .map(|_| ())
            .map_err(|_| BabysitError::CodexInitializationFailed)
    }
}

/// 进程内懒运行时，供当前所有 File Change 共享。
pub(crate) struct CodexRuntime {
    provider: Arc<CodexAppServerProvider>,
    discovery: CodexExecutableDiscovery,
    settings: ApplicationCodexSettings,
    status: Mutex<CodexRuntimeStatus>,
    flight: AsyncMutex<Option<Arc<Notify>>>,
}

impl CodexRuntime {
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
            flight: AsyncMutex::new(None),
        }
    }

    /// 返回当前 typed 状态，但不暴露选择的路径。
    pub(crate) fn status(&self) -> CodexRuntimeStatus {
        self.status
            .lock()
            .map(|status| *status)
            .unwrap_or(CodexRuntimeStatus::InitFailed)
    }

    /// 并发调用只执行一次探测；后续重试会开启新的 flight。
    pub(crate) async fn ensure_ready(&self, workspace_path: &Path) -> Result<(), BabysitError> {
        loop {
            let waiter = {
                let mut flight = self.flight.lock().await;
                match flight.as_ref() {
                    Some(waiter) => Some(waiter.clone()),
                    None => {
                        if self.status() == CodexRuntimeStatus::Ready {
                            return Ok(());
                        }
                        let waiter = Arc::new(Notify::new());
                        *flight = Some(waiter.clone());
                        if let Ok(mut status) = self.status.lock() {
                            *status = CodexRuntimeStatus::Probing;
                        }
                        None
                    }
                }
            };

            if let Some(waiter) = waiter {
                waiter.notified().await;
                if self.status() == CodexRuntimeStatus::Probing {
                    continue;
                }
                return status_error(self.status());
            }

            let result = self.probe_once(workspace_path).await;
            if let Ok(mut status) = self.status.lock() {
                *status = result
                    .as_ref()
                    .map(|_| CodexRuntimeStatus::Ready)
                    .unwrap_or_else(|error| status_for_error(error));
            }
            let waiter = self.flight.lock().await.take();
            if let Some(waiter) = waiter {
                waiter.notify_waiters();
            }
            return result;
        }
    }

    /// 校验并保存 picker 结果；下一次明确启用时重试初始化。
    pub(crate) async fn select_executable(&self, path: &Path) -> Result<(), BabysitError> {
        let resolved = self.discovery.validate_selected(path.to_path_buf()).await?;
        self.settings.save(resolved.path.clone())?;
        self.provider.configure_executable(resolved.path).await?;
        if let Ok(mut status) = self.status.lock() {
            *status = CodexRuntimeStatus::Unknown;
        }
        Ok(())
    }

    /// 应用退出时释放 provider 进程，并让下一次显式启用重新完成 discovery。
    pub(crate) async fn shutdown(&self) {
        self.provider.shutdown().await;
        if let Ok(mut status) = self.status.lock() {
            *status = CodexRuntimeStatus::Unknown;
        }
        if let Some(waiter) = self.flight.lock().await.take() {
            waiter.notify_waiters();
        }
    }

    /// 解析持久化/PATH 候选，配置 #74，并在同一工作区完成握手与能力探针。
    async fn probe_once(&self, workspace_path: &Path) -> Result<(), BabysitError> {
        let candidates = self
            .discovery
            .resolve_all(self.settings.load().as_deref())
            .await?;
        let mut last_error = BabysitError::CodexNotFound;
        let mut found_signed_out = false;
        for candidate in candidates {
            self.provider.configure_executable(candidate.path).await?;
            match self.provider.prepare_process(workspace_path).await {
                Ok(()) => return Ok(()),
                Err(BabysitError::CodexSignedOut) => found_signed_out = true,
                Err(error) => last_error = error,
            }
        }
        if found_signed_out {
            Err(BabysitError::CodexSignedOut)
        } else {
            Err(last_error)
        }
    }
}

/// 将完成的探测映射为 Tauri 使用的同一稳定 typed 错误。
fn status_error(status: CodexRuntimeStatus) -> Result<(), BabysitError> {
    match status {
        CodexRuntimeStatus::Ready => Ok(()),
        CodexRuntimeStatus::Missing => Err(BabysitError::CodexNotFound),
        CodexRuntimeStatus::SignedOut => Err(BabysitError::CodexSignedOut),
        CodexRuntimeStatus::Incompatible => Err(BabysitError::CodexIncompatible),
        CodexRuntimeStatus::InitFailed => Err(BabysitError::CodexInitializationFailed),
        CodexRuntimeStatus::Unknown | CodexRuntimeStatus::Probing => {
            Err(BabysitError::CodexInitializationFailed)
        }
    }
}

/// 保持状态转换表小而且与 provider 无关。
fn status_for_error(error: &BabysitError) -> CodexRuntimeStatus {
    match error {
        BabysitError::CodexNotFound | BabysitError::CodexExecutableInvalid => {
            CodexRuntimeStatus::Missing
        }
        BabysitError::CodexSignedOut => CodexRuntimeStatus::SignedOut,
        BabysitError::CodexIncompatible => CodexRuntimeStatus::Incompatible,
        _ => CodexRuntimeStatus::InitFailed,
    }
}

#[cfg(test)]
#[path = "../tests/codex/runtime.rs"]
mod tests;
