//! Codex 懒运行时状态与应用设置 adapter。
//!
//! Runtime 拥有 discovery 与 provider 初始化周围的小状态机，
//! 不向前端或诊断暴露可执行文件路径。

use super::discovery::CodexExecutableDiscovery;
use super::CodexAppServerProvider;
use crate::babysit::BabysitError;
use serde::{Deserialize, Serialize};
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

/// 持久化应用设置；Review 状态和缓存永远不会接收此路径。
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct CodexSettings {
    executable: Option<PathBuf>,
}

/// 与可执行文件 discovery 分离的文件设置 adapter。
struct FileCodexSettings {
    path: PathBuf,
}

impl FileCodexSettings {
    /// 读取绝对路径选择；格式错误或相对路径会被忽略。
    fn load(&self) -> Option<PathBuf> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|value| serde_json::from_str::<CodexSettings>(&value).ok())
            .and_then(|settings| settings.executable)
            .filter(|path| path.is_absolute())
    }

    /// 只持久化选择的可执行文件路径，并在同目录替换文件。
    fn save(&self, executable: PathBuf) -> Result<(), BabysitError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|_| BabysitError::CodexInitializationFailed)?;
        }
        let payload = serde_json::to_vec(&CodexSettings {
            executable: Some(executable),
        })
        .map_err(|_| BabysitError::CodexInitializationFailed)?;
        let temporary = self.path.with_extension("tmp");
        std::fs::write(&temporary, payload)
            .and_then(|_| std::fs::rename(&temporary, &self.path))
            .map_err(|_| BabysitError::CodexInitializationFailed)
    }
}

/// 进程内懒运行时，供当前所有 File Change 共享。
pub(crate) struct CodexRuntime {
    provider: Arc<CodexAppServerProvider>,
    discovery: CodexExecutableDiscovery,
    settings: FileCodexSettings,
    status: Mutex<CodexRuntimeStatus>,
    flight: AsyncMutex<Option<Arc<Notify>>>,
}

impl CodexRuntime {
    /// 构造 idle runtime；此处不执行设置、PATH 或进程 I/O。
    pub(crate) fn new(provider: Arc<CodexAppServerProvider>, settings_path: PathBuf) -> Self {
        Self {
            provider,
            discovery: CodexExecutableDiscovery,
            settings: FileCodexSettings {
                path: settings_path,
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
        self.provider.configure_executable(resolved.path)?;
        if let Ok(mut status) = self.status.lock() {
            *status = CodexRuntimeStatus::Unknown;
        }
        Ok(())
    }

    /// 解析持久化/PATH 候选，配置 #74，并在同一工作区完成握手与能力探针。
    async fn probe_once(&self, workspace_path: &Path) -> Result<(), BabysitError> {
        let resolved = self
            .discovery
            .resolve(self.settings.load().as_deref())
            .await?;
        self.provider.configure_executable(resolved.path)?;
        self.provider.prepare_process(workspace_path).await
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
