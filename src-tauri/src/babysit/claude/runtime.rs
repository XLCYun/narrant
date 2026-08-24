//! Claude Code executable 的懒发现、持久化选择与能力探针。

use super::super::{
    BabysitError, BabysitProviderRuntimeStatus, BabysitProviderSetupFailure, ClaudeCodeProvider,
    InstalledProviderRuntime,
};
use crate::application_settings::ApplicationSettingsApplication;
use async_trait::async_trait;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::process::Command;

use super::discovery::candidate_paths;

const PROVIDER_ID: &str = "claude-code";
#[cfg(not(test))]
const PROBE_STEP_TIMEOUT: Duration = Duration::from_secs(10);
#[cfg(test)]
const PROBE_STEP_TIMEOUT: Duration = Duration::from_millis(750);

/// Claude Code runtime；状态与 executable 设置均由 application 生命周期统一拥有。
pub(crate) struct ClaudeRuntime {
    provider: Arc<ClaudeCodeProvider>,
    settings: ApplicationSettingsApplication,
    status: Mutex<BabysitProviderRuntimeStatus>,
    #[cfg(test)]
    candidates_override: Option<Vec<ClaudeCandidate>>,
}

/// 一个可启动的 Claude Code 候选；测试可注入 shell fixture 前缀参数。
#[derive(Clone)]
struct ClaudeCandidate {
    program: PathBuf,
    prefix_args: Vec<OsString>,
}

impl ClaudeRuntime {
    /// 仅执行 Claude Code 完整能力探针并返回规范路径，不持久化或切换 provider。
    pub(crate) async fn validate_executable(path: &Path) -> Result<PathBuf, BabysitError> {
        let workspace = std::env::current_dir().map_err(|_| {
            BabysitError::provider_setup(
                PROVIDER_ID,
                BabysitProviderSetupFailure::InitializationFailed,
            )
        })?;
        let canonical = path.canonicalize().map_err(|_| {
            BabysitError::provider_setup(
                PROVIDER_ID,
                BabysitProviderSetupFailure::ExecutableInvalid,
            )
        })?;
        let candidate = ClaudeCandidate {
            program: canonical.clone(),
            prefix_args: Vec::new(),
        };
        Self::probe(&candidate, &workspace).await?;
        Ok(canonical)
    }

    /// 构造未知状态的 runtime；构造过程不启动任何 Claude 进程。
    pub(crate) fn new(provider: Arc<ClaudeCodeProvider>, settings_path: PathBuf) -> Self {
        Self {
            provider,
            settings: ApplicationSettingsApplication::new(settings_path),
            status: Mutex::new(BabysitProviderRuntimeStatus::Unknown),
            #[cfg(test)]
            candidates_override: None,
        }
    }

    /// 注入确定性候选，测试不依赖本机 Claude 安装或认证。
    #[cfg(test)]
    pub(super) fn with_candidates(
        provider: Arc<ClaudeCodeProvider>,
        settings_path: PathBuf,
        candidates: Vec<(PathBuf, Vec<OsString>)>,
    ) -> Self {
        Self {
            provider,
            settings: ApplicationSettingsApplication::new(settings_path),
            status: Mutex::new(BabysitProviderRuntimeStatus::Unknown),
            candidates_override: Some(
                candidates
                    .into_iter()
                    .map(|(program, prefix_args)| ClaudeCandidate {
                        program,
                        prefix_args,
                    })
                    .collect(),
            ),
        }
    }

    /// 返回设置路径、环境覆盖与 PATH 命令组成的有序候选集。
    fn candidates(&self) -> Vec<ClaudeCandidate> {
        #[cfg(test)]
        if let Some(candidates) = &self.candidates_override {
            return candidates.clone();
        }
        let selected = self
            .settings
            .load()
            .ok()
            .and_then(|settings| settings.babysit.claude_code.executable);
        let paths = candidate_paths(
            selected,
            std::env::var_os("CLAUDE_CODE_EXECUTABLE").map(PathBuf::from),
        );
        paths
            .into_iter()
            .map(|program| ClaudeCandidate {
                program,
                prefix_args: Vec::new(),
            })
            .collect()
    }

    /// 仅用本地元数据命令验证版本、所需 flags 与认证，不触发付费模型请求。
    async fn probe(candidate: &ClaudeCandidate, workspace_path: &Path) -> Result<(), BabysitError> {
        let mut version_command = Command::new(&candidate.program);
        version_command
            .args(&candidate.prefix_args)
            .arg("--version")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        crate::child_process::configure_tokio_command(&mut version_command);
        let version = tokio::time::timeout(PROBE_STEP_TIMEOUT, version_command.status()).await;
        let version = match version {
            Ok(result) => result,
            Err(_) => {
                return Err(BabysitError::provider_setup(
                    PROVIDER_ID,
                    BabysitProviderSetupFailure::ExecutableInvalid,
                ))
            }
        };
        if !matches!(version, Ok(status) if status.success()) {
            return Err(BabysitError::provider_setup(
                PROVIDER_ID,
                BabysitProviderSetupFailure::ExecutableInvalid,
            ));
        }
        let mut help_command = Command::new(&candidate.program);
        help_command
            .args(&candidate.prefix_args)
            .arg("--help")
            .current_dir(workspace_path)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        crate::child_process::configure_tokio_command(&mut help_command);
        let help = tokio::time::timeout(PROBE_STEP_TIMEOUT, help_command.output())
            .await
            .map_err(|_| {
                BabysitError::provider_setup(
                    PROVIDER_ID,
                    BabysitProviderSetupFailure::InitializationFailed,
                )
            })?
            .map_err(|_| {
                BabysitError::provider_setup(
                    PROVIDER_ID,
                    BabysitProviderSetupFailure::InitializationFailed,
                )
            })?;
        let help_text = String::from_utf8_lossy(&help.stdout);
        if !help.status.success()
            || ![
                "--output-format",
                "--json-schema",
                "--no-session-persistence",
                "--permission-mode",
                "--tools",
                "--disallowedTools",
            ]
            .iter()
            .all(|flag| help_text.contains(flag))
        {
            return Err(BabysitError::provider_setup(
                PROVIDER_ID,
                BabysitProviderSetupFailure::Incompatible,
            ));
        }
        let mut auth_command = Command::new(&candidate.program);
        auth_command
            .args(&candidate.prefix_args)
            .args(["auth", "status", "--json"])
            .current_dir(workspace_path)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        crate::child_process::configure_tokio_command(&mut auth_command);
        let auth = tokio::time::timeout(PROBE_STEP_TIMEOUT, auth_command.output())
            .await
            .map_err(|_| {
                BabysitError::provider_setup(
                    PROVIDER_ID,
                    BabysitProviderSetupFailure::InitializationFailed,
                )
            })?
            .map_err(|_| {
                BabysitError::provider_setup(
                    PROVIDER_ID,
                    BabysitProviderSetupFailure::InitializationFailed,
                )
            })?;
        if !auth.status.success() {
            Err(BabysitError::provider_setup(
                PROVIDER_ID,
                BabysitProviderSetupFailure::SignedOut,
            ))
        } else {
            match serde_json::from_slice::<serde_json::Value>(&auth.stdout)
                .ok()
                .and_then(|value| value.get("loggedIn").and_then(serde_json::Value::as_bool))
            {
                Some(true) => Ok(()),
                Some(false) => Err(BabysitError::provider_setup(
                    PROVIDER_ID,
                    BabysitProviderSetupFailure::SignedOut,
                )),
                None => Err(BabysitError::provider_setup(
                    PROVIDER_ID,
                    BabysitProviderSetupFailure::InitializationFailed,
                )),
            }
        }
    }

    /// 更新公开 readiness；锁中毒统一收敛为初始化失败。
    fn set_status(&self, next: BabysitProviderRuntimeStatus) {
        if let Ok(mut status) = self.status.lock() {
            *status = next;
        }
    }
}

/// 按用户可操作性聚合候选错误，避免后续缺失项遮蔽更明确的诊断。
fn preferred_error(current: BabysitError, candidate: BabysitError) -> BabysitError {
    if setup_error_priority(&candidate) > setup_error_priority(&current) {
        candidate
    } else {
        current
    }
}

/// SignedOut 最明确；Incompatible 比瞬时初始化失败更可操作，缺失类最低。
fn setup_error_priority(error: &BabysitError) -> u8 {
    match error {
        BabysitError::ProviderSetup {
            failure: BabysitProviderSetupFailure::SignedOut,
            ..
        } => 4,
        BabysitError::ProviderSetup {
            failure: BabysitProviderSetupFailure::Incompatible,
            ..
        } => 3,
        BabysitError::ProviderSetup {
            failure: BabysitProviderSetupFailure::InitializationFailed,
            ..
        } => 2,
        _ => 1,
    }
}

#[async_trait]
impl InstalledProviderRuntime for ClaudeRuntime {
    /// 返回稳定 Claude Code provider identity。
    fn provider_id(&self) -> &str {
        PROVIDER_ID
    }

    /// 返回最近一次懒探针结果。
    fn status(&self) -> BabysitProviderRuntimeStatus {
        self.status
            .lock()
            .map(|value| *value)
            .unwrap_or(BabysitProviderRuntimeStatus::InitFailed)
    }

    /// 首次需要时依次探测候选；失败不会尝试或装配 Codex。
    async fn ensure_ready(&self, workspace_path: &Path) -> Result<(), BabysitError> {
        if self.status() == BabysitProviderRuntimeStatus::Ready {
            return Ok(());
        }
        self.set_status(BabysitProviderRuntimeStatus::Probing);
        let mut last =
            BabysitError::provider_setup(PROVIDER_ID, BabysitProviderSetupFailure::NotFound);
        for candidate in self.candidates() {
            match Self::probe(&candidate, workspace_path).await {
                Ok(()) => {
                    self.provider.set_program(candidate.program)?;
                    self.set_status(BabysitProviderRuntimeStatus::Ready);
                    return Ok(());
                }
                Err(error) => last = preferred_error(last, error),
            }
        }
        let status = match &last {
            BabysitError::ProviderSetup {
                failure: BabysitProviderSetupFailure::SignedOut,
                ..
            } => BabysitProviderRuntimeStatus::SignedOut,
            BabysitError::ProviderSetup {
                failure: BabysitProviderSetupFailure::Incompatible,
                ..
            } => BabysitProviderRuntimeStatus::Incompatible,
            BabysitError::ProviderSetup {
                failure:
                    BabysitProviderSetupFailure::NotFound
                    | BabysitProviderSetupFailure::ExecutableInvalid,
                ..
            } => BabysitProviderRuntimeStatus::Missing,
            _ => BabysitProviderRuntimeStatus::InitFailed,
        };
        self.set_status(status);
        Err(last)
    }

    /// Claude 每轮使用 fresh process，因此 runtime 无常驻进程需要关闭。
    async fn shutdown(&self) {}
}

#[cfg(test)]
#[path = "../tests/claude/runtime.rs"]
mod tests;
