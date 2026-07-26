use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_OUTPUT_LIMIT: usize = 16 * 1024 * 1024;

/// 可由调用方共享的取消标记，用于终止正在运行的 Core Git 子进程。
#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);

impl CancellationToken {
    /// 请求取消所有共享此标记的 Core Git 操作。
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// 返回调用方是否已经请求取消。
    fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// Core Git 命令失败，消息只用于诊断，不参与业务分支判断。
#[derive(Debug)]
pub struct GitCommandError {
    message: String,
}

impl GitCommandError {
    /// 构造带有可读诊断信息的 Core Git 错误。
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// 返回可用于测试和界面诊断的错误消息。
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for GitCommandError {
    /// 将稳定的诊断消息写入错误 formatter。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for GitCommandError {}

/// 一次成功的 Core Git 命令输出。
#[derive(Debug)]
pub struct GitOutput {
    pub stdout: Vec<u8>,
}

/// 直接执行 Core Git 参数数组，并统一施加超时、取消和输出上限。
#[derive(Clone)]
pub struct GitProcessAdapter {
    repository_path: PathBuf,
    timeout: Duration,
    output_limit: usize,
}

impl GitProcessAdapter {
    /// 为指定仓库创建使用默认安全限制的进程 adapter。
    pub fn new(repository_path: &Path) -> Self {
        Self {
            repository_path: repository_path.to_owned(),
            timeout: DEFAULT_TIMEOUT,
            output_limit: DEFAULT_OUTPUT_LIMIT,
        }
    }

    /// 使用参数数组运行 Core Git；命令永远不会经过 shell。
    pub fn run<I, S>(
        &self,
        arguments: I,
        cancellation: &CancellationToken,
    ) -> Result<GitOutput, GitCommandError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let mut child = Command::new("git")
            .arg("-C")
            .arg(&self.repository_path)
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| GitCommandError::new(format!("无法启动 Core Git：{error}")))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| GitCommandError::new("无法读取 Core Git 标准输出"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| GitCommandError::new("无法读取 Core Git 标准错误"))?;
        let stdout_reader = read_limited(stdout, self.output_limit);
        let stderr_reader = read_limited(stderr, self.output_limit);
        let started = Instant::now();

        let status = loop {
            if cancellation.is_cancelled() {
                terminate_and_drain(&mut child, stdout_reader, stderr_reader)?;
                return Err(GitCommandError::new("Core Git 操作已取消"));
            }
            if started.elapsed() >= self.timeout {
                terminate_and_drain(&mut child, stdout_reader, stderr_reader)?;
                return Err(GitCommandError::new("Core Git 操作超时"));
            }
            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) => thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    terminate_and_drain(&mut child, stdout_reader, stderr_reader)?;
                    return Err(GitCommandError::new(format!(
                        "等待 Core Git 进程失败：{error}"
                    )));
                }
            }
        };

        let (stdout, stdout_exceeded) = join_reader(stdout_reader)?;
        let (stderr, stderr_exceeded) = join_reader(stderr_reader)?;
        if stdout_exceeded || stderr_exceeded {
            return Err(GitCommandError::new("Core Git 输出超过安全上限"));
        }
        if !status.success() {
            let diagnostic = String::from_utf8_lossy(&stderr).trim().to_owned();
            return Err(GitCommandError::new(if diagnostic.is_empty() {
                format!("Core Git 退出状态：{status}")
            } else {
                format!("Core Git 失败：{diagnostic}")
            }));
        }
        Ok(GitOutput { stdout })
    }

    /// 在精确受管路径创建 detached linked worktree，并校验其 HEAD。
    pub fn create_detached_worktree(
        &self,
        workspace_path: &Path,
        head_object_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        if workspace_path.exists() {
            let actual_head = read_head_object_id(workspace_path, cancellation)?;
            if actual_head == head_object_id {
                return Ok(());
            }
            return Err(GitCommandError::new(
                "Review Workspace 已存在但 HEAD 与 current Snapshot 不一致；请关闭 Review 后重试。",
            ));
        }
        let parent = workspace_path
            .parent()
            .ok_or_else(|| GitCommandError::new("Review Workspace 路径无效"))?;
        std::fs::create_dir_all(parent).map_err(|error| {
            GitCommandError::new(format!("无法创建 Review Workspace 目录：{error}"))
        })?;
        self.run(
            [
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("--detach"),
                OsStr::new("--"),
                workspace_path.as_os_str(),
                OsStr::new(head_object_id),
            ],
            cancellation,
        )
        .map_err(|error| {
            GitCommandError::new(format!(
                "无法创建 Review Workspace：{error}。请确认磁盘空间和仓库 worktree 状态后重试。"
            ))
        })?;
        let postcondition = read_head_object_id(workspace_path, cancellation).and_then(|actual| {
            if actual == head_object_id {
                Ok(())
            } else {
                Err(GitCommandError::new(
                    "Review Workspace HEAD 与 Snapshot head object ID 不一致。",
                ))
            }
        });
        if let Err(error) = postcondition {
            let cleanup = self.remove_worktree(workspace_path, &CancellationToken::default());
            return Err(match cleanup {
                Ok(()) => error,
                Err(cleanup_error) => GitCommandError::new(format!(
                    "{error}；自动清理失败：{cleanup_error}。请关闭 Narrant 并检查该仓库的 linked worktrees。"
                )),
            });
        }
        Ok(())
    }

    /// 精确移除调用方已验证归属的 disposable linked worktree。
    pub fn remove_worktree(
        &self,
        workspace_path: &Path,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        if !workspace_path.exists() {
            return Ok(());
        }
        self.run(
            [
                OsStr::new("worktree"),
                OsStr::new("remove"),
                OsStr::new("--force"),
                OsStr::new("--"),
                workspace_path.as_os_str(),
            ],
            cancellation,
        )
        .map(|_| ())
        .map_err(|error| GitCommandError::new(format!("无法移除 Review Workspace：{error}")))
    }

    /// 在 common directory 与 registered worktree path 精确匹配后强制移除受管 workspace。
    pub fn remove_owned_worktree(
        &self,
        workspace_path: &Path,
        expected_common_directory: &Path,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        self.validate_owned_worktree(workspace_path, expected_common_directory, cancellation)?;
        self.remove_worktree(workspace_path, cancellation)
    }

    /// 验证路径确属保存的 common directory，且仍是该仓库精确注册的 linked worktree。
    pub fn validate_owned_worktree(
        &self,
        workspace_path: &Path,
        expected_common_directory: &Path,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        let actual_common = self
            .run(
                ["rev-parse", "--path-format=absolute", "--git-common-dir"],
                cancellation,
            )
            .and_then(|output| {
                String::from_utf8(output.stdout)
                    .map(|value| PathBuf::from(value.trim()))
                    .map_err(|_| GitCommandError::new("Git common directory 编码无效"))
            })?;
        let actual_common = std::fs::canonicalize(&actual_common).map_err(|error| {
            GitCommandError::new(format!("无法解析 Git common directory：{error}"))
        })?;
        let expected_common =
            std::fs::canonicalize(expected_common_directory).map_err(|error| {
                GitCommandError::new(format!("无法解析保存的 Git common directory：{error}"))
            })?;
        if actual_common != expected_common {
            return Err(GitCommandError::new(
                "Review Workspace 所有权验证失败：Git common directory 不匹配。",
            ));
        }
        let expected_workspace = std::fs::canonicalize(workspace_path).map_err(|_| {
            GitCommandError::new("Review Workspace 所有权验证失败：受管路径不存在。")
        })?;
        let registered = self.run(["worktree", "list", "--porcelain", "-z"], cancellation)?;
        let path_is_registered = registered.stdout.split(|byte| *byte == 0).any(|field| {
            field
                .strip_prefix(b"worktree ")
                .and_then(|path| std::str::from_utf8(path).ok())
                .and_then(|path| std::fs::canonicalize(path).ok())
                .is_some_and(|path| path == expected_workspace)
        });
        if !path_is_registered {
            return Err(GitCommandError::new(
                "Review Workspace 所有权验证失败：路径不是该仓库注册的 linked worktree。",
            ));
        }
        Ok(())
    }
}

/// 读取 linked worktree 的精确 HEAD object ID，供创建前后统一校验。
fn read_head_object_id(
    workspace_path: &Path,
    cancellation: &CancellationToken,
) -> Result<String, GitCommandError> {
    GitProcessAdapter::new(workspace_path)
        .run(["rev-parse", "HEAD"], cancellation)
        .and_then(|output| {
            String::from_utf8(output.stdout)
                .map(|value| value.trim().to_owned())
                .map_err(|_| GitCommandError::new("Review Workspace HEAD 编码无效"))
        })
}

/// 终止异常子进程并回收两条输出管道，确保错误路径不遗留进程或线程。
fn terminate_and_drain(
    child: &mut std::process::Child,
    stdout_reader: thread::JoinHandle<std::io::Result<(Vec<u8>, bool)>>,
    stderr_reader: thread::JoinHandle<std::io::Result<(Vec<u8>, bool)>>,
) -> Result<(), GitCommandError> {
    let _ = child.kill();
    let _ = child.wait();
    join_reader(stdout_reader)?;
    join_reader(stderr_reader)?;
    Ok(())
}

/// 在后台持续排空管道，并记录输出是否超过限制，避免子进程因管道写满而阻塞。
fn read_limited(
    mut reader: impl Read + Send + 'static,
    limit: usize,
) -> thread::JoinHandle<std::io::Result<(Vec<u8>, bool)>> {
    thread::spawn(move || {
        let mut captured = Vec::new();
        let mut buffer = [0_u8; 8192];
        let mut exceeded = false;
        loop {
            let count = reader.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            let remaining = limit.saturating_sub(captured.len());
            captured.extend_from_slice(&buffer[..count.min(remaining)]);
            exceeded |= count > remaining;
        }
        Ok((captured, exceeded))
    })
}

/// 合并输出读取线程，并将线程异常转换为稳定的 adapter 错误。
fn join_reader(
    reader: thread::JoinHandle<std::io::Result<(Vec<u8>, bool)>>,
) -> Result<(Vec<u8>, bool), GitCommandError> {
    reader
        .join()
        .map_err(|_| GitCommandError::new("Core Git 输出读取线程异常"))?
        .map_err(|error| GitCommandError::new(format!("读取 Core Git 输出失败：{error}")))
}

#[cfg(test)]
#[path = "core_git_tests.rs"]
mod tests;
