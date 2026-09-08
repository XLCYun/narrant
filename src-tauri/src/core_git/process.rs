use super::*;

#[path = "input.rs"]
mod input;
use input::InputWriter;

impl CoreGitProcess {
    /// 异步执行 Core Git，同时避免阻塞调用方的异步运行时线程。
    pub async fn execute(
        &self,
        invocation: GitInvocation,
        cancellation: &CancellationSignal,
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
    pub(super) fn with_limits(timeout: Duration, output_limit: usize) -> Self {
        Self {
            limits: Some((timeout, output_limit)),
        }
    }

    /// 为同步领域调用执行同一套 Core Git 进程协议。
    fn execute_blocking(
        &self,
        invocation: GitInvocation,
        cancellation: &CancellationSignal,
    ) -> Result<GitOutput, CoreGitFailure> {
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            if runtime.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread {
                return tokio::task::block_in_place(|| {
                    runtime.block_on(self.execute(invocation, cancellation))
                });
            }
        }
        execute_core_git(invocation, cancellation, self.limits)
    }
}

impl GitProcessAdapter {
    /// 在既有查询限制下并发写入批处理输入、读取两条输出管道。
    pub(crate) fn run_with_input<I, S>(
        &self,
        arguments: I,
        input: Vec<u8>,
        cancellation: &CancellationSignal,
    ) -> Result<GitOutput, GitCommandError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        CoreGitProcess::default().execute_blocking(
            GitInvocation::new(&self.repository_path, arguments, GitOperationClass::Query)
                .with_input(input),
            cancellation,
        )
    }

    /// 使用参数数组运行 Core Git；命令永远不会经过 shell。
    pub(crate) fn run<I, S>(
        &self,
        arguments: I,
        cancellation: &CancellationSignal,
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
        cancellation: &CancellationSignal,
    ) -> Result<GitOutput, GitCommandError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run_classified(arguments, GitOperationClass::Network, cancellation)
    }

    /// 使用 workspace 只读检查策略执行 Core Git 参数数组。
    pub(super) fn run_workspace_inspect<I, S>(
        &self,
        arguments: I,
        cancellation: &CancellationSignal,
    ) -> Result<GitOutput, GitCommandError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run_classified(arguments, GitOperationClass::WorkspaceInspect, cancellation)
    }

    /// 使用 workspace 变更策略执行可能创建或删除大量文件的 Core Git 命令。
    pub(super) fn run_workspace_mutation<I, S>(
        &self,
        arguments: I,
        cancellation: &CancellationSignal,
    ) -> Result<GitOutput, GitCommandError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        self.run_classified(
            arguments,
            GitOperationClass::WorkspaceMutation,
            cancellation,
        )
    }

    /// 将同步仓库操作收敛到同一进程实现，并选择对应的限制策略。
    fn run_classified<I, S>(
        &self,
        arguments: I,
        class: GitOperationClass,
        cancellation: &CancellationSignal,
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
}

enum ReaderEvent {
    InputFailed(String),
    LimitExceeded(GitOutputStream),
    Completed(GitOutputStream, std::io::Result<Vec<u8>>),
}

fn execute_core_git(
    invocation: GitInvocation,
    cancellation: &CancellationSignal,
    limits: Option<(Duration, usize)>,
) -> Result<GitOutput, CoreGitFailure> {
    let (timeout, output_limit) = limits.unwrap_or_else(|| policy_for(invocation.class));
    let mut command = Command::new("git");
    configure_non_interactive_credentials(&mut command);
    command
        .arg("-C")
        .arg(&invocation.repository_path)
        .args(invocation.arguments)
        .stdin(if invocation.input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    crate::child_process::configure_std_command(&mut command);
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
    let input_writer = invocation.input.map(|input| {
        let stdin = child
            .stdin
            .take()
            .expect("piped stdin was configured for batch input");
        InputWriter::start(stdin, input, events.clone())
    });
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

    while status.is_none()
        || stdout.is_none()
        || stderr.is_none()
        || input_writer
            .as_ref()
            .is_some_and(|writer| !writer.is_finished())
    {
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
                    failure = Some(CoreGitFailure::from_kind(
                        CoreGitFailureKind::WaitFailed,
                        format!("等待 Core Git 进程失败：{error}"),
                    ));
                    continue;
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
    if let Some(writer) = input_writer {
        writer.finish()?;
    }
    Ok(GitOutput { stdout })
}

/// 保留已配置凭据助手的静默读取能力，但禁止 Core Git 或 GCM 打开交互式提示。
pub(super) fn configure_non_interactive_credentials(command: &mut Command) {
    command
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GCM_INTERACTIVE", "Never");
}

pub(super) fn policy_for(class: GitOperationClass) -> (Duration, usize) {
    let timeout = match class {
        GitOperationClass::Query => QUERY_TIMEOUT,
        GitOperationClass::Network => NETWORK_TIMEOUT,
        GitOperationClass::WorkspaceInspect => WORKSPACE_INSPECT_TIMEOUT,
        GitOperationClass::WorkspaceMutation => WORKSPACE_MUTATION_TIMEOUT,
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
        ReaderEvent::InputFailed(message) => {
            if failure.is_none() {
                *failure = Some(CoreGitFailure::from_kind(
                    CoreGitFailureKind::InputWriteFailed,
                    format!("写入 Core Git 输入失败：{message}"),
                ));
            }
        }
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
    let deadline = Instant::now() + Duration::from_secs(5);
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
        let child_already_exited = child
            .try_wait()
            .map_err(|error| format!("无法检查 Core Git 子进程状态：{error}"))?
            .is_some();
        let process_group = -(child.id() as i32);
        let group_result = unsafe { libc::kill(process_group, libc::SIGKILL) };
        if group_result != 0 {
            let group_error = std::io::Error::last_os_error();
            if group_error.raw_os_error() != Some(libc::ESRCH) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("无法终止 Core Git 进程组：{group_error}"));
            }
        }
        if child_already_exited {
            return Ok(());
        }
    }
    #[cfg(windows)]
    {
        let process_id = child.id().to_string();
        let mut command = Command::new("taskkill");
        command
            .args(["/PID", &process_id, "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        crate::child_process::configure_std_command(&mut command);
        let status = command
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
