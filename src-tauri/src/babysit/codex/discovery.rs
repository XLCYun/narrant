//! Codex 可执行文件发现。
//!
//! 发现 seam 只负责解析和校验可执行文件。持久化设置由 `runtime.rs` 单独拥有，
//! App Server 协议和进程隔离仍由 provider 负责，避免路径进入 Review 状态和诊断。

use crate::babysit::BabysitError;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// `codex --version` 必须快速结束；挂起的可执行文件不可用。
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);
/// 版本探测只允许保留很小的 stdout/stderr；异常输出不会进入诊断。
const VERSION_OUTPUT_CAP: usize = 4 * 1024;

/// 已通过校验的可执行文件及其仅供内部使用的版本标签。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ResolvedExecutable {
    pub(super) path: PathBuf,
    pub(super) version: String,
}

/// 纯候选搜索与有界直接进程校验。
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct CodexExecutableDiscovery;

impl CodexExecutableDiscovery {
    /// 先搜索持久化路径，再搜索当前进程 PATH 和已知安装位置。
    pub(super) async fn resolve(
        &self,
        persisted: Option<&Path>,
    ) -> Result<ResolvedExecutable, BabysitError> {
        self.resolve_candidates(persisted, self.candidates()).await
    }

    /// 按既定优先级校验候选；测试可注入候选而不改变当前进程 PATH。
    async fn resolve_candidates<I>(
        &self,
        persisted: Option<&Path>,
        candidates: I,
    ) -> Result<ResolvedExecutable, BabysitError>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        if let Some(path) = persisted.and_then(canonical_executable) {
            if let Ok(executable) = self.validate(path, true).await {
                return Ok(executable);
            }
        }

        for candidate in candidates {
            if let Some(candidate) = canonical_executable(&candidate) {
                if let Ok(executable) = self.validate(candidate, false).await {
                    return Ok(executable);
                }
            }
        }
        Err(BabysitError::CodexNotFound)
    }

    /// 校验用户选择的绝对路径，但不在发现模块内持久化。
    pub(super) async fn validate_selected(
        &self,
        path: PathBuf,
    ) -> Result<ResolvedExecutable, BabysitError> {
        let path = canonical_executable(&path).ok_or(BabysitError::CodexExecutableInvalid)?;
        self.validate(path, true).await
    }

    /// 校验 `--version`；内存中只保留简短版本字符串。
    async fn validate(
        &self,
        path: PathBuf,
        selected: bool,
    ) -> Result<ResolvedExecutable, BabysitError> {
        self.validate_with_timeout(path, selected, VERSION_TIMEOUT)
            .await
    }

    /// 以直接子进程、有界管道读取和显式 reap 完成版本校验。
    async fn validate_with_timeout(
        &self,
        path: PathBuf,
        selected: bool,
        timeout: Duration,
    ) -> Result<ResolvedExecutable, BabysitError> {
        let child = Command::new(&path)
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn();
        let Ok(mut child) = child else {
            return Err(if selected {
                BabysitError::CodexExecutableInvalid
            } else {
                BabysitError::CodexNotFound
            });
        };
        let Some(stdout) = child.stdout.take() else {
            terminate_version_child(&mut child, None, None).await;
            return Err(if selected {
                BabysitError::CodexExecutableInvalid
            } else {
                BabysitError::CodexNotFound
            });
        };
        let Some(stderr) = child.stderr.take() else {
            terminate_version_child(&mut child, None, None).await;
            return Err(if selected {
                BabysitError::CodexExecutableInvalid
            } else {
                BabysitError::CodexNotFound
            });
        };

        let mut stdout_task = tokio::spawn(read_capped(stdout));
        let mut stderr_task = tokio::spawn(read_capped(stderr));
        let probe = tokio::time::timeout(timeout, async {
            let stdout = (&mut stdout_task)
                .await
                .map_err(|_| VersionProbeFailure::Reader)?
                .map_err(|_| VersionProbeFailure::Reader)?;
            let stderr = (&mut stderr_task)
                .await
                .map_err(|_| VersionProbeFailure::Reader)?
                .map_err(|_| VersionProbeFailure::Reader)?;
            let status = child
                .wait()
                .await
                .map_err(|_| VersionProbeFailure::Process)?;
            Ok::<_, VersionProbeFailure>((status, stdout, stderr))
        })
        .await;

        let (status, stdout, stderr) = match probe {
            Ok(Ok(output)) => output,
            Ok(Err(_)) | Err(_) => {
                terminate_version_child(&mut child, Some(&mut stdout_task), Some(&mut stderr_task))
                    .await;
                return Err(if selected {
                    BabysitError::CodexExecutableInvalid
                } else {
                    BabysitError::CodexNotFound
                });
            }
        };
        if !status.success() {
            return Err(if selected {
                BabysitError::CodexExecutableInvalid
            } else {
                BabysitError::CodexNotFound
            });
        }
        let version = stdout
            .iter()
            .chain(stderr.iter())
            .copied()
            .collect::<Vec<_>>();
        let version = String::from_utf8_lossy(&version)
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .ok_or(if selected {
                BabysitError::CodexExecutableInvalid
            } else {
                BabysitError::CodexNotFound
            })?;
        Ok(ResolvedExecutable { path, version })
    }

    /// Search PATH first, followed by platform-supported installation locations.
    fn candidates(&self) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        if let Some(path) = std::env::var_os("PATH") {
            candidates.extend(std::env::split_paths(&path).flat_map(|directory| {
                executable_names()
                    .into_iter()
                    .map(move |name| directory.join(name))
            }));
        }
        candidates.extend(known_install_locations());
        let mut seen = HashSet::new();
        candidates.retain(|candidate| seen.insert(candidate.clone()));
        candidates
    }
}

/// 读取单条版本流，超过固定上限立即失败，不积累未界定的进程输出。
async fn read_capped<R>(mut reader: R) -> Result<Vec<u8>, VersionProbeFailure>
where
    R: AsyncRead + Unpin,
{
    let mut output = Vec::with_capacity(VERSION_OUTPUT_CAP.min(256));
    let mut buffer = [0_u8; 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .await
            .map_err(|_| VersionProbeFailure::Reader)?;
        if read == 0 {
            return Ok(output);
        }
        if output.len().saturating_add(read) > VERSION_OUTPUT_CAP {
            return Err(VersionProbeFailure::OutputTooLarge);
        }
        output.extend_from_slice(&buffer[..read]);
    }
}

/// 版本读取任一路径失败后，取消管道读取并显式 kill/wait 子进程。
async fn terminate_version_child(
    child: &mut Child,
    stdout_task: Option<&mut JoinHandle<Result<Vec<u8>, VersionProbeFailure>>>,
    stderr_task: Option<&mut JoinHandle<Result<Vec<u8>, VersionProbeFailure>>>,
) {
    if let Some(task) = stdout_task {
        task.abort();
        if !task.is_finished() {
            let _ = task.await;
        }
    }
    if let Some(task) = stderr_task {
        task.abort();
        if !task.is_finished() {
            let _ = task.await;
        }
    }
    let _ = child.start_kill();
    let _ = child.wait().await;
}

/// 版本探测失败只在内部区分原因，永远不携带原始路径或输出。
#[derive(Clone, Copy, Debug)]
enum VersionProbeFailure {
    Reader,
    Process,
    OutputTooLarge,
}

/// 在启动前 canonicalize 候选路径，并要求它是普通可执行文件。
fn canonical_executable(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let canonical = path.canonicalize().ok()?;
    let metadata = std::fs::metadata(&canonical).ok()?;
    if !metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o111 == 0 {
        return None;
    }
    Some(canonical)
}

/// 可执行名称覆盖 Windows 原生 shim，同时不启动 shell。
fn executable_names() -> Vec<OsString> {
    if cfg!(windows) {
        vec![OsString::from("codex.exe"), OsString::from("codex.cmd")]
    } else {
        vec![OsString::from("codex")]
    }
}

/// 已知位置覆盖桌面启动时 PATH 不完整的场景。
fn known_install_locations() -> Vec<PathBuf> {
    let mut locations = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        locations.extend([
            home.join(".local/bin/codex"),
            home.join(".cargo/bin/codex"),
            home.join(".volta/bin/codex"),
            home.join(".asdf/shims/codex"),
            home.join(".npm-global/bin/codex"),
            home.join(".bun/bin/codex"),
        ]);
    }
    #[cfg(target_os = "macos")]
    locations.extend([
        PathBuf::from("/usr/local/bin/codex"),
        PathBuf::from("/opt/homebrew/bin/codex"),
        PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex"),
        PathBuf::from("/Applications/Codex.app/Contents/Resources/codex"),
    ]);
    #[cfg(target_os = "linux")]
    locations.extend([
        PathBuf::from("/usr/local/bin/codex"),
        PathBuf::from("/usr/bin/codex"),
    ]);
    #[cfg(windows)]
    if let Some(app_data) = std::env::var_os("APPDATA").map(PathBuf::from) {
        locations.push(app_data.join("npm/codex.cmd"));
    }
    locations
}

#[cfg(test)]
#[path = "../tests/codex/discovery.rs"]
mod tests;
