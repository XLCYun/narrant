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
        self.validate_with_timeout_observed(path, selected, timeout)
            .await
            .0
    }

    /// 执行版本探测并返回直接子进程是否已经完成 wait；测试用它验证回收契约。
    async fn validate_with_timeout_observed(
        &self,
        path: PathBuf,
        selected: bool,
        timeout: Duration,
    ) -> (Result<ResolvedExecutable, BabysitError>, bool) {
        let child = Command::new(&path)
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn();
        let Ok(mut child) = child else {
            return (
                Err(if selected {
                    BabysitError::CodexExecutableInvalid
                } else {
                    BabysitError::CodexNotFound
                }),
                true,
            );
        };
        let Some(stdout) = child.stdout.take() else {
            let reaped = kill_and_reap_child(&mut child).await;
            return (
                Err(if selected {
                    BabysitError::CodexExecutableInvalid
                } else {
                    BabysitError::CodexNotFound
                }),
                reaped,
            );
        };
        let Some(stderr) = child.stderr.take() else {
            let reaped = kill_and_reap_child(&mut child).await;
            return (
                Err(if selected {
                    BabysitError::CodexExecutableInvalid
                } else {
                    BabysitError::CodexNotFound
                }),
                reaped,
            );
        };

        let mut probe = VersionProbeChild::new(child, stdout, stderr);
        let result = tokio::time::timeout(timeout, probe.collect()).await;
        let (status, stdout, stderr) = match result {
            Ok(Ok(output)) => output,
            Ok(Err(_)) | Err(_) => {
                probe.stop_and_reap().await;
                let reaped = probe.reaped;
                return (
                    Err(if selected {
                        BabysitError::CodexExecutableInvalid
                    } else {
                        BabysitError::CodexNotFound
                    }),
                    reaped,
                );
            }
        };
        if !status.success() {
            return (
                Err(if selected {
                    BabysitError::CodexExecutableInvalid
                } else {
                    BabysitError::CodexNotFound
                }),
                probe.reaped,
            );
        }
        let version = stdout
            .iter()
            .chain(stderr.iter())
            .copied()
            .collect::<Vec<_>>();
        let version = match String::from_utf8_lossy(&version)
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
        {
            Some(version) => version,
            None => {
                return (
                    Err(if selected {
                        BabysitError::CodexExecutableInvalid
                    } else {
                        BabysitError::CodexNotFound
                    }),
                    probe.reaped,
                )
            }
        };
        (Ok(ResolvedExecutable { path, version }), probe.reaped)
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

/// 保持直接 Child、两个有界 reader 和 wait 状态的单一拥有者。
struct VersionProbeChild {
    child: Child,
    stdout_task: JoinHandle<Result<Vec<u8>, VersionProbeFailure>>,
    stderr_task: JoinHandle<Result<Vec<u8>, VersionProbeFailure>>,
    stdout_joined: bool,
    stderr_joined: bool,
    reaped: bool,
}

impl VersionProbeChild {
    /// 启动两个独立有界 reader，并把 Child 的回收责任集中到本对象。
    fn new(
        child: Child,
        stdout: tokio::process::ChildStdout,
        stderr: tokio::process::ChildStderr,
    ) -> Self {
        Self {
            child,
            stdout_task: tokio::spawn(read_capped(stdout)),
            stderr_task: tokio::spawn(read_capped(stderr)),
            stdout_joined: false,
            stderr_joined: false,
            reaped: false,
        }
    }

    /// 按顺序 join 两个 reader 后再等待直接 Child 退出。
    async fn collect(
        &mut self,
    ) -> Result<(std::process::ExitStatus, Vec<u8>, Vec<u8>), VersionProbeFailure> {
        let stdout = self.join_stdout().await?;
        let stderr = self.join_stderr().await?;
        let status = self
            .child
            .wait()
            .await
            .map_err(|_| VersionProbeFailure::Process)?;
        self.reaped = true;
        Ok((status, stdout, stderr))
    }

    /// 终止未完成 reader，逐个 join 后再显式 kill/wait 直接 Child。
    async fn stop_and_reap(&mut self) {
        if !self.stdout_joined {
            self.stdout_task.abort();
            let _ = (&mut self.stdout_task).await;
            self.stdout_joined = true;
        }
        if !self.stderr_joined {
            self.stderr_task.abort();
            let _ = (&mut self.stderr_task).await;
            self.stderr_joined = true;
        }
        let _ = self.child.start_kill();
        if self.child.wait().await.is_ok() {
            self.reaped = true;
        }
    }

    /// join stdout reader；标记只在 future 真正完成后更新，timeout 不会丢失所有权。
    async fn join_stdout(&mut self) -> Result<Vec<u8>, VersionProbeFailure> {
        let result = (&mut self.stdout_task).await;
        self.stdout_joined = true;
        result
            .map_err(|_| VersionProbeFailure::Reader)?
            .map_err(|_| VersionProbeFailure::Reader)
    }

    /// join stderr reader；与 stdout 使用相同的生命周期规则。
    async fn join_stderr(&mut self) -> Result<Vec<u8>, VersionProbeFailure> {
        let result = (&mut self.stderr_task).await;
        self.stderr_joined = true;
        result
            .map_err(|_| VersionProbeFailure::Reader)?
            .map_err(|_| VersionProbeFailure::Reader)
    }
}

/// 没有 reader pipe 时仍由调用方显式 kill/wait 直接 Child。
async fn kill_and_reap_child(child: &mut Child) -> bool {
    let _ = child.start_kill();
    child.wait().await.is_ok()
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
