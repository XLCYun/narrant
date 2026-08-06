use std::ffi::{OsStr, OsString};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

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
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// 产生超限输出的 Core Git 管道。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitOutputStream {
    Stdout,
    Stderr,
}

/// Core Git 失败的稳定类别；诊断文本不参与业务分支。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreGitFailureKind {
    Cancelled,
    TimedOut,
    OutputLimitExceeded {
        stream: GitOutputStream,
        limit: usize,
    },
    SpawnFailed,
    Exited {
        code: Option<i32>,
    },
    OutputReadFailed,
    WaitFailed,
    TerminationFailed,
    InvalidData,
}

/// Core Git 命令失败，消息只用于诊断，不参与业务分支判断。
#[derive(Debug)]
pub struct CoreGitFailure {
    kind: CoreGitFailureKind,
    message: String,
}

impl CoreGitFailure {
    /// 构造调用方解析或校验失败；该兼容入口不代表进程执行失败。
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            kind: CoreGitFailureKind::InvalidData,
            message: message.into(),
        }
    }

    /// 构造由调用方取消产生的稳定错误类别。
    fn cancelled() -> Self {
        Self {
            kind: CoreGitFailureKind::Cancelled,
            message: "Core Git 操作已取消".to_owned(),
        }
    }

    /// 构造指定稳定类别和诊断文本的 Core Git 失败。
    fn from_kind(kind: CoreGitFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    /// 返回供业务分支判断的稳定失败类别。
    pub fn kind(&self) -> CoreGitFailureKind {
        self.kind
    }

    /// 添加领域上下文，同时保留原始稳定失败类别。
    pub fn with_context(mut self, context: impl AsRef<str>) -> Self {
        self.message = format!("{}：{}", context.as_ref(), self.message);
        self
    }

    /// 返回错误是否由合作式取消产生。
    pub fn is_cancelled(&self) -> bool {
        self.kind == CoreGitFailureKind::Cancelled
    }

    /// 返回可用于测试和界面诊断的错误消息。
    pub fn message(&self) -> &str {
        &self.message
    }
}

impl std::fmt::Display for CoreGitFailure {
    /// 将稳定的诊断消息写入错误 formatter。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CoreGitFailure {}

/// 兼容上层领域模块的既有名称；稳定失败模型由 `CoreGitFailure` 提供。
pub type GitCommandError = CoreGitFailure;

/// 一次成功的 Core Git 命令输出。
#[derive(Debug)]
pub struct GitOutput {
    pub stdout: Vec<u8>,
}

/// Core Git 调用的策略类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GitOperationClass {
    Query,
    Network,
    Workspace,
}

/// 一次 Core Git 调用的仓库、参数数组与策略类别。
pub struct GitInvocation {
    repository_path: PathBuf,
    arguments: Vec<OsString>,
    class: GitOperationClass,
}

impl GitInvocation {
    /// 从直接参数数组创建调用；参数不会经过 shell。
    pub fn new<I, S>(repository_path: &Path, arguments: I, class: GitOperationClass) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        Self {
            repository_path: repository_path.to_owned(),
            arguments: arguments
                .into_iter()
                .map(|argument| argument.as_ref().to_owned())
                .collect(),
            class,
        }
    }
}

/// 统一执行、限制、终止并回收 Core Git 子进程。
#[derive(Clone, Default)]
pub struct CoreGitProcess {
    limits: Option<(Duration, usize)>,
}

impl CoreGitProcess {
    /// 异步执行 Core Git，同时避免阻塞调用方的异步运行时线程。
    pub async fn execute(
        &self,
        invocation: GitInvocation,
        cancellation: &CancellationToken,
    ) -> Result<GitOutput, CoreGitFailure> {
        let cancellation = cancellation.clone();
        let limits = self.limits;
        tokio::task::spawn_blocking(move || execute_core_git(invocation, &cancellation, limits))
            .await
            .map_err(|error| {
                CoreGitFailure::from_kind(
                    CoreGitFailureKind::OutputReadFailed,
                    format!("Core Git 执行任务异常：{error}"),
                )
            })?
    }

    /// 为确定性的进程限制测试创建执行器。
    #[cfg(test)]
    fn with_limits(timeout: Duration, output_limit: usize) -> Self {
        Self {
            limits: Some((timeout, output_limit)),
        }
    }

    /// 为同步领域调用执行同一套 Core Git 进程协议。
    fn execute_blocking(
        &self,
        invocation: GitInvocation,
        cancellation: &CancellationToken,
    ) -> Result<GitOutput, CoreGitFailure> {
        execute_core_git(invocation, cancellation, self.limits)
    }
}

/// 直接执行 Core Git 参数数组，并统一施加超时、取消和输出上限。
#[derive(Clone)]
pub struct GitProcessAdapter {
    repository_path: PathBuf,
}

impl GitProcessAdapter {
    /// 为指定仓库创建使用默认安全限制的进程 adapter。
    pub fn new(repository_path: &Path) -> Self {
        Self {
            repository_path: repository_path.to_owned(),
        }
    }

    /// 使用参数数组运行 Core Git；命令永远不会经过 shell。
    pub(crate) fn run<I, S>(
        &self,
        arguments: I,
        cancellation: &CancellationToken,
    ) -> Result<GitOutput, GitCommandError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run_classified(arguments, GitOperationClass::Query, cancellation)
    }

    /// 使用网络操作策略执行 Core Git 参数数组。
    pub(crate) fn run_network<I, S>(
        &self,
        arguments: I,
        cancellation: &CancellationToken,
    ) -> Result<GitOutput, GitCommandError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run_classified(arguments, GitOperationClass::Network, cancellation)
    }

    /// 使用 workspace 操作策略执行 Core Git 参数数组。
    fn run_workspace<I, S>(
        &self,
        arguments: I,
        cancellation: &CancellationToken,
    ) -> Result<GitOutput, GitCommandError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run_classified(arguments, GitOperationClass::Workspace, cancellation)
    }

    /// 将同步仓库操作收敛到同一进程实现，并选择对应的限制策略。
    fn run_classified<I, S>(
        &self,
        arguments: I,
        class: GitOperationClass,
        cancellation: &CancellationToken,
    ) -> Result<GitOutput, GitCommandError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        CoreGitProcess::default().execute_blocking(
            GitInvocation::new(&self.repository_path, arguments, class),
            cancellation,
        )
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
        self.run_workspace(
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
            error
                .with_context("无法创建 Review Workspace。请确认磁盘空间和仓库 worktree 状态后重试")
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
                Err(cleanup_error) => error.with_context(format!(
                    "自动清理失败：{cleanup_error}。请关闭 Narrant 并检查该仓库的 linked worktrees"
                )),
            });
        }
        Ok(())
    }

    /// 强制重建受管 Review Workspace，并将其恢复为指定 detached HEAD。
    fn force_restore_detached_worktree(
        &self,
        workspace_path: &Path,
        head_object_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        self.remove_worktree(workspace_path, cancellation)?;
        self.create_detached_worktree(workspace_path, head_object_id, cancellation)
    }

    /// 将 Workspace 切换到新 Snapshot；核心阶段失败时恢复旧 Snapshot HEAD。
    pub fn transition_detached_worktree(
        &self,
        workspace_path: &Path,
        expected_common_directory: &Path,
        previous_head_object_id: &str,
        next_head_object_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        self.validate_owned_worktree(workspace_path, expected_common_directory, cancellation)?;
        self.validate_registered_worktree(workspace_path, previous_head_object_id, cancellation)?;
        if let Err(error) =
            self.force_restore_detached_worktree(workspace_path, next_head_object_id, cancellation)
        {
            let rollback = self.force_restore_detached_worktree(
                workspace_path,
                previous_head_object_id,
                &CancellationToken::default(),
            );
            return Err(match rollback {
                Ok(()) => error,
                Err(rollback_error) => {
                    error.with_context(format!("恢复旧 Review Workspace 失败：{rollback_error}"))
                }
            });
        }
        Ok(())
    }

    /// 验证目标是当前 Repository 注册且位于预期 HEAD 的精确 linked worktree。
    fn validate_registered_worktree(
        &self,
        workspace_path: &Path,
        expected_head_object_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        let output = self.run_workspace(["worktree", "list", "--porcelain", "-z"], cancellation)?;
        let canonical_workspace = workspace_path.canonicalize().map_err(|error| {
            GitCommandError::new(format!("无法验证 Review Workspace canonical path：{error}"))
        })?;
        let expected_path =
            format!("worktree {}", canonical_workspace.to_string_lossy()).into_bytes();
        let expected_head = format!("HEAD {expected_head_object_id}").into_bytes();
        let fields = output.stdout.split(|byte| *byte == 0).collect::<Vec<_>>();
        let registered = fields
            .windows(2)
            .any(|fields| fields[0] == expected_path && fields[1] == expected_head);
        if !registered {
            return Err(GitCommandError::new(
                "拒绝重建 Review Workspace：目标路径、Git worktree 注册或 current Snapshot HEAD 不一致。",
            ));
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
        self.run_workspace(
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
        .map_err(|error| error.with_context("无法移除 Review Workspace"))
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
        let registered =
            self.run_workspace(["worktree", "list", "--porcelain", "-z"], cancellation)?;
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

enum ReaderEvent {
    LimitExceeded(GitOutputStream),
    Completed(GitOutputStream, std::io::Result<Vec<u8>>),
}

fn execute_core_git(
    invocation: GitInvocation,
    cancellation: &CancellationToken,
    limits: Option<(Duration, usize)>,
) -> Result<GitOutput, CoreGitFailure> {
    let (timeout, output_limit) = limits.unwrap_or_else(|| policy_for(invocation.class));
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(&invocation.repository_path)
        .args(invocation.arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                Err(std::io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command.spawn().map_err(|error| {
        CoreGitFailure::from_kind(
            CoreGitFailureKind::SpawnFailed,
            format!("无法启动 Core Git：{error}"),
        )
    })?;
    let Some(stdout) = child.stdout.take() else {
        terminate_process_tree(&mut child).map_err(|detail| {
            CoreGitFailure::from_kind(CoreGitFailureKind::TerminationFailed, detail)
        })?;
        return Err(CoreGitFailure::from_kind(
            CoreGitFailureKind::OutputReadFailed,
            "无法读取 Core Git 标准输出",
        ));
    };
    let Some(stderr) = child.stderr.take() else {
        terminate_process_tree(&mut child).map_err(|detail| {
            CoreGitFailure::from_kind(CoreGitFailureKind::TerminationFailed, detail)
        })?;
        return Err(CoreGitFailure::from_kind(
            CoreGitFailureKind::OutputReadFailed,
            "无法读取 Core Git 标准错误",
        ));
    };
    let (events, reader_events) = mpsc::channel();
    read_stream(
        stdout,
        GitOutputStream::Stdout,
        output_limit,
        events.clone(),
    );
    read_stream(stderr, GitOutputStream::Stderr, output_limit, events);
    let started = Instant::now();
    let mut status = None;
    let mut stdout = None;
    let mut stderr = None;
    let mut failure = None;

    while status.is_none() || stdout.is_none() || stderr.is_none() {
        while let Ok(event) = reader_events.try_recv() {
            record_reader_event(event, output_limit, &mut stdout, &mut stderr, &mut failure);
        }
        if failure.is_none() && cancellation.is_cancelled() {
            failure = Some(CoreGitFailure::cancelled());
        }
        if failure.is_none() && started.elapsed() >= timeout {
            failure = Some(CoreGitFailure::from_kind(
                CoreGitFailureKind::TimedOut,
                "Core Git 操作超时",
            ));
        }
        if failure.is_some() {
            terminate_process_tree(&mut child).map_err(|detail| {
                CoreGitFailure::from_kind(CoreGitFailureKind::TerminationFailed, detail)
            })?;
            drain_reader_events(
                &reader_events,
                output_limit,
                &mut stdout,
                &mut stderr,
                &mut failure,
            )?;
            return Err(failure.expect("failure was set before termination"));
        }
        if status.is_none() {
            match child.try_wait() {
                Ok(Some(exit)) => status = Some(exit),
                Ok(None) => {}
                Err(error) => {
                    let wait_failure = CoreGitFailure::from_kind(
                        CoreGitFailureKind::WaitFailed,
                        format!("等待 Core Git 进程失败：{error}"),
                    );
                    terminate_process_tree(&mut child).map_err(|detail| {
                        CoreGitFailure::from_kind(CoreGitFailureKind::TerminationFailed, detail)
                    })?;
                    return Err(wait_failure);
                }
            }
        }
        if status.is_some() && (stdout.is_none() || stderr.is_none()) {
            match reader_events.recv_timeout(Duration::from_millis(10)) {
                Ok(event) => {
                    record_reader_event(event, output_limit, &mut stdout, &mut stderr, &mut failure)
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return Err(CoreGitFailure::from_kind(
                        CoreGitFailureKind::OutputReadFailed,
                        "Core Git 输出读取线程提前结束",
                    ));
                }
            }
        } else {
            thread::sleep(Duration::from_millis(10));
        }
    }

    if let Some(failure) = failure {
        return Err(failure);
    }
    let status = status.expect("loop exits only after child status is available");
    let stdout = stdout
        .expect("loop exits only after stdout is available")
        .map_err(|error| {
            CoreGitFailure::from_kind(
                CoreGitFailureKind::OutputReadFailed,
                format!("读取 Core Git 标准输出失败：{error}"),
            )
        })?;
    let stderr = stderr
        .expect("loop exits only after stderr is available")
        .map_err(|error| {
            CoreGitFailure::from_kind(
                CoreGitFailureKind::OutputReadFailed,
                format!("读取 Core Git 标准错误失败：{error}"),
            )
        })?;
    if !status.success() {
        let diagnostic = String::from_utf8_lossy(&stderr).trim().to_owned();
        return Err(CoreGitFailure::from_kind(
            CoreGitFailureKind::Exited {
                code: status.code(),
            },
            if diagnostic.is_empty() {
                format!("Core Git 退出状态：{status}")
            } else {
                format!("Core Git 失败：{diagnostic}")
            },
        ));
    }
    Ok(GitOutput { stdout })
}

fn policy_for(class: GitOperationClass) -> (Duration, usize) {
    let timeout = match class {
        GitOperationClass::Query => DEFAULT_TIMEOUT,
        GitOperationClass::Network => Duration::from_secs(5 * 60),
        GitOperationClass::Workspace => Duration::from_secs(2 * 60),
    };
    (timeout, DEFAULT_OUTPUT_LIMIT)
}

fn read_stream(
    mut reader: impl Read + Send + 'static,
    stream: GitOutputStream,
    limit: usize,
    events: mpsc::Sender<ReaderEvent>,
) {
    thread::spawn(move || {
        let mut captured = Vec::new();
        let mut buffer = [0_u8; 8192];
        let result = loop {
            match reader.read(&mut buffer) {
                Ok(0) => break Ok(captured),
                Ok(count) => {
                    let remaining = limit.saturating_sub(captured.len());
                    captured.extend_from_slice(&buffer[..count.min(remaining)]);
                    if count > remaining {
                        let _ = events.send(ReaderEvent::LimitExceeded(stream));
                        while reader.read(&mut buffer).is_ok_and(|count| count > 0) {}
                        break Ok(captured);
                    }
                }
                Err(error) => break Err(error),
            }
        };
        let _ = events.send(ReaderEvent::Completed(stream, result));
    });
}

fn record_reader_event(
    event: ReaderEvent,
    output_limit: usize,
    stdout: &mut Option<std::io::Result<Vec<u8>>>,
    stderr: &mut Option<std::io::Result<Vec<u8>>>,
    failure: &mut Option<CoreGitFailure>,
) {
    match event {
        ReaderEvent::LimitExceeded(stream) => {
            if failure.is_none() {
                *failure = Some(CoreGitFailure::from_kind(
                    CoreGitFailureKind::OutputLimitExceeded {
                        stream,
                        limit: output_limit,
                    },
                    "Core Git 输出超过安全上限",
                ));
            }
        }
        ReaderEvent::Completed(stream, result) => {
            if let Err(error) = &result {
                if failure.is_none() {
                    *failure = Some(CoreGitFailure::from_kind(
                        CoreGitFailureKind::OutputReadFailed,
                        format!("读取 Core Git {stream:?} 失败：{error}"),
                    ));
                }
            }
            match stream {
                GitOutputStream::Stdout => *stdout = Some(result),
                GitOutputStream::Stderr => *stderr = Some(result),
            }
        }
    }
}

fn drain_reader_events(
    events: &mpsc::Receiver<ReaderEvent>,
    output_limit: usize,
    stdout: &mut Option<std::io::Result<Vec<u8>>>,
    stderr: &mut Option<std::io::Result<Vec<u8>>>,
    failure: &mut Option<CoreGitFailure>,
) -> Result<(), CoreGitFailure> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while stdout.is_none() || stderr.is_none() {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(CoreGitFailure::from_kind(
                CoreGitFailureKind::TerminationFailed,
                "Core Git 已终止，但输出管道未能及时回收",
            ));
        }
        match events.recv_timeout(remaining.min(Duration::from_millis(20))) {
            Ok(event) => {
                record_reader_event(event, output_limit, stdout, stderr, failure);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                return Err(CoreGitFailure::from_kind(
                    CoreGitFailureKind::OutputReadFailed,
                    "Core Git 输出读取线程提前结束",
                ));
            }
        }
    }
    if stdout.as_ref().is_some_and(Result::is_err) || stderr.as_ref().is_some_and(Result::is_err) {
        return Err(CoreGitFailure::from_kind(
            CoreGitFailureKind::OutputReadFailed,
            "读取 Core Git 输出失败",
        ));
    }
    Ok(())
}

fn terminate_process_tree(child: &mut std::process::Child) -> Result<(), String> {
    #[cfg(unix)]
    {
        if child
            .try_wait()
            .map_err(|error| format!("无法检查 Core Git 子进程状态：{error}"))?
            .is_some()
        {
            return Ok(());
        }
        let process_group = -(child.id() as i32);
        let group_result = unsafe { libc::kill(process_group, libc::SIGKILL) };
        if group_result != 0
            && child
                .try_wait()
                .map_err(|error| format!("无法检查 Core Git 子进程状态：{error}"))?
                .is_none()
        {
            let group_error = std::io::Error::last_os_error();
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("无法终止 Core Git 进程组：{group_error}"));
        }
    }
    #[cfg(windows)]
    {
        let process_id = child.id().to_string();
        let status = Command::new("taskkill")
            .args(["/PID", &process_id, "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .map_err(|error| format!("无法启动 Core Git 进程树终止命令：{error}"))?;
        if !status.success() && child.try_wait().ok().flatten().is_none() {
            return Err(format!("Core Git 进程树终止命令失败：{status}"));
        }
    }
    let _ = child.kill();
    child
        .wait()
        .map(|_| ())
        .map_err(|error| format!("无法回收 Core Git 子进程：{error}"))
}

#[cfg(test)]
#[path = "core_git_tests.rs"]
mod tests;
