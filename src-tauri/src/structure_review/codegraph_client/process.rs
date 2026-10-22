//! 一次性有界 stdio；写完关闭输入，最终消息和完整退出共同决定结果。

use super::{
    cancelled, failure,
    protocol::{self, Completed, Message},
    CodeGraphAnalyzer, Result,
};
use crate::child_process::ProcessTree;
use crate::structure_review::code_graph::CodeGraph;
use serde_json::Value;
use std::{
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
        Arc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// 固定轮询粒度只用于响应取消，不改变各阶段绝对截止时间。
const POLL: Duration = Duration::from_millis(10);

/// 三个工作线程回报事件，队列容量固定，不能被恶意 stdout 无限撑大。
enum Event {
    Line(Vec<u8>),
    StdoutEof,
    StderrEof(Vec<u8>),
    Written,
    Error(String),
}

/// 当前进程与全部线程的唯一所有者，清理完毕前不会释放调用栈。
pub(super) struct Session {
    process: ProcessTree,
    incoming: Option<Receiver<Event>>,
    workers: Vec<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}

impl Session {
    /// 直接启动对应语言分析器，工作目录和临时目录均为仓库外 scratch。
    pub(super) fn start(
        analyzer: &CodeGraphAnalyzer,
        request: &Value,
        scratch: &Path,
    ) -> Result<Self> {
        let mut bytes = serde_json::to_vec(request).map_err(failure)?;
        bytes.push(b'\n');
        let mut command = Command::new(&analyzer.executable);
        #[cfg(test)]
        command.args(&analyzer.test_prefix_args);
        command
            .arg("analyze")
            .current_dir(scratch)
            .env("TMPDIR", scratch)
            .env("TMP", scratch)
            .env("TEMP", scratch)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(test)]
        command.envs(&analyzer.test_environment);
        let process = ProcessTree::spawn(&mut command).map_err(failure)?;
        let (events, incoming) = mpsc::sync_channel(4);
        let mut session = Self {
            process,
            incoming: Some(incoming),
            workers: Vec::new(),
            stop: Arc::new(AtomicBool::new(false)),
        };
        let stdout = session
            .process
            .take_stdout()
            .ok_or_else(|| failure("CodeGraph 缺少 stdout"))?;
        let stderr = session
            .process
            .take_stderr()
            .ok_or_else(|| failure("CodeGraph 缺少 stderr"))?;
        let stdin = session
            .process
            .take_stdin()
            .ok_or_else(|| failure("CodeGraph 缺少 stdin"))?;
        nonblocking(&stdout)?;
        nonblocking(&stderr)?;
        nonblocking(&stdin)?;
        let stop = session.stop.clone();
        let sender = events.clone();
        let limit = analyzer.limits.max_result_bytes;
        session.workers.push(
            std::thread::Builder::new()
                .name("codegraph-stdout".into())
                .spawn(move || {
                    let mut pipe = stdout;
                    read_stdout(&mut pipe, sender, stop.clone(), limit);
                    drain_pipe(pipe, &stop);
                })
                .map_err(failure)?,
        );
        let stop = session.stop.clone();
        let sender = events.clone();
        let limit = analyzer.limits.max_diagnostic_bytes;
        session.workers.push(
            std::thread::Builder::new()
                .name("codegraph-stderr".into())
                .spawn(move || {
                    let mut pipe = stderr;
                    read_stderr(&mut pipe, sender, stop.clone(), limit);
                    drain_pipe(pipe, &stop);
                })
                .map_err(failure)?,
        );
        let stop = session.stop.clone();
        session.workers.push(
            std::thread::Builder::new()
                .name("codegraph-stdin".into())
                .spawn(move || write_stdin(stdin, bytes, events, stop))
                .map_err(failure)?,
        );
        Ok(session)
    }

    /// 回收过程不观察取消；退出或失败后关闭队列并等待每个线程。
    fn finish(&mut self) -> Result<()> {
        let cleanup = self.process.finish(Duration::ZERO).map_err(failure);
        self.stop.store(true, Ordering::Release);
        self.incoming.take();
        let mut panicked = false;
        for worker in self.workers.drain(..) {
            panicked |= worker.join().is_err();
        }
        cleanup?;
        if panicked {
            return Err(failure("CodeGraph 读写线程异常退出"));
        }
        Ok(())
    }
}

impl Drop for Session {
    /// panic 和任何提前返回都执行相同的显式进程、线程清理。
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

/// 单次请求的全部阶段，以真实失败优先的规则汇总退出与清理结果。
pub(super) fn run(
    analyzer: &CodeGraphAnalyzer,
    request: &Value,
    scratch: &Path,
) -> Result<CodeGraph> {
    let mut session = Session::start(analyzer, request, scratch)?;
    let result = exchange(&mut session, analyzer);
    let cleanup = session.finish();
    match (result, cleanup) {
        (Err(mut error), Err(cleanup)) => {
            error.message.push_str(&format!("；清理失败：{cleanup}"));
            if error.is_cancelled() {
                Err(failure(error))
            } else {
                Err(error)
            }
        }
        (Err(error), _) => Err(error),
        (_, Err(error)) => Err(error),
        (Ok(value), Ok(())) => Ok(value),
    }
}

/// 从启动起只有一个分析期限；结果或根退出只收紧期限，不能延长等待。
fn exchange(session: &mut Session, analyzer: &CodeGraphAnalyzer) -> Result<CodeGraph> {
    let mut deadline = Instant::now() + analyzer.limits.analysis_timeout;
    let mut analyze_written = false;
    let mut terminal: Option<Result<CodeGraph>> = None;
    let mut root_exited = false;
    let mut stdout_eof = false;
    let mut stderr_eof = false;
    let mut diagnostics = Vec::new();
    loop {
        // 已观察的真实消息先于同轮用户取消；后端失败不能被取消掩盖。
        let event = session
            .incoming
            .as_ref()
            .expect("会话尚未清理")
            .recv_timeout(POLL);
        match event {
            Ok(Event::Line(bytes)) => {
                if terminal.is_some() {
                    return preserve(terminal, failure("completed 后出现额外协议消息"));
                }
                let result = completed(protocol::decode(&bytes)?, &analyzer.id)?;
                deadline = deadline.min(Instant::now() + analyzer.limits.exit_timeout);
                terminal =
                    Some(result.and_then(|completed| {
                        super::graph::build(completed, analyzer.languages())
                    }));
            }
            Ok(Event::StdoutEof) => {
                stdout_eof = true;
                if terminal.is_none() {
                    let mut error = failure("CodeGraph 在完整 completed 前关闭 stdout");
                    if !diagnostics.is_empty() {
                        error.message.push_str(&format!(
                            "；stderr：{}",
                            String::from_utf8_lossy(&diagnostics)
                        ));
                    }
                    terminal = Some(Err(error));
                    deadline = deadline.min(Instant::now() + analyzer.limits.exit_timeout);
                }
            }
            Ok(Event::StderrEof(bytes)) => {
                stderr_eof = true;
                diagnostics = bytes;
                if let Some(Err(error)) = terminal.as_mut() {
                    if !diagnostics.is_empty() {
                        error.message.push_str(&format!(
                            "；stderr：{}",
                            String::from_utf8_lossy(&diagnostics)
                        ));
                    }
                }
            }
            Ok(Event::Error(error)) => return preserve(terminal, failure(error)),
            Ok(Event::Written) => analyze_written = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) if !stdout_eof || !stderr_eof => {
                return preserve(terminal, failure("CodeGraph 读写线程意外关闭"))
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => std::thread::sleep(POLL),
        }
        let status = session.process.status().map_err(failure)?;
        if let Some(status) = status {
            if !root_exited {
                root_exited = true;
                // 根退出也可能留下持有 stdout/stderr 的后代，不能继续等满分析期限。
                deadline = deadline.min(Instant::now() + analyzer.limits.exit_timeout);
            }
            if stdout_eof && stderr_eof {
                if !status.success() {
                    return preserve(
                        terminal,
                        failure(format!(
                            "CodeGraph 异常退出 {status}：{}",
                            String::from_utf8_lossy(&diagnostics)
                        )),
                    );
                }
                if analyze_written || terminal.as_ref().is_some_and(|value| value.is_err()) {
                    return terminal.unwrap_or_else(|| Err(failure("CodeGraph 缺少终结响应")));
                }
            }
        }
        if (analyzer.cancelled)() {
            if let Some(status) = status.filter(|status| !status.success()) {
                return preserve(terminal, failure(format!("CodeGraph 异常退出 {status}")));
            }
            return preserve(terminal, cancelled());
        }
        if Instant::now() >= deadline {
            let reason = if terminal.is_some() || root_exited {
                "CodeGraph 退出超时"
            } else {
                "CodeGraph 分析超时"
            };
            let reason = match status.filter(|status| !status.success()) {
                Some(status) => format!("CodeGraph 异常退出 {status}；{reason}"),
                None => reason.into(),
            };
            return preserve(terminal, failure(reason));
        }
    }
}

/// 已观察的真实后端错误不被后来的退出异常、协议尾部错误或取消覆盖。
fn preserve<T>(
    terminal: Option<Result<CodeGraph>>,
    next: crate::structure_review::code_graph::AnalysisError,
) -> Result<T> {
    if let Some(Err(mut error)) = terminal {
        if !error.is_cancelled() {
            error.message.push_str(&format!("；{next}"));
            return Err(error);
        }
    }
    Err(next)
}

/// 每个最终分支都核对固定身份；只有拒收封套的两类错误允许 null 请求 ID。
fn completed(message: Message, pack: &str) -> Result<Result<Completed>> {
    let (version, engine, identity, kind, id, outcome) = match &message {
        Message::Ok(value) => (
            &value.protocol,
            &value.engine_version,
            &value.pack_id,
            &value.message_type,
            Some(value.id.as_str()),
            &value.outcome,
        ),
        Message::Error(value) => (
            &value.protocol,
            &value.engine_version,
            &value.pack_id,
            &value.message_type,
            value.id.0.as_deref(),
            &value.outcome,
        ),
    };
    if version != protocol::PROTOCOL
        || engine != super::ENGINE_VERSION
        || identity != pack
        || kind != "completed"
        || id.is_some_and(|id| id != protocol::REQUEST_ID)
        || !["ok", "error"].contains(&outcome.as_str())
    {
        return Err(failure("completed 固定身份、请求 ID 或封套无效"));
    }
    let missing_id = id.is_none();
    Ok(match message {
        Message::Ok(value) => Ok(value),
        Message::Error(value) => {
            let error = value.error;
            if ![
                "invalid_input",
                "invalid_request",
                "incompatible_protocol",
                "invalid_pack",
                "analysis_failed",
                "timeout",
                "io_failure",
                "cleanup_failed",
                "resource_limit",
                "internal_failure",
            ]
            .contains(&error.code.as_str())
                || error.message.is_empty()
                || (missing_id
                    && !["invalid_request", "incompatible_protocol"].contains(&error.code.as_str()))
            {
                return Err(failure("CodeGraph 错误码、消息或请求 ID 无效"));
            }
            Err(failure(format!("{}：{}", error.code, error.message)))
        }
    })
}

/// Unix 管道使用非阻塞模式，使 panic 清理也能取消所有读写线程。
#[cfg(unix)]
fn nonblocking(pipe: &impl std::os::fd::AsRawFd) -> Result<()> {
    // SAFETY: FD 借自当前仍存活的管道，F_SETFL 只改变其非阻塞属性。
    let flags = unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_GETFL) };
    if flags < 0
        || unsafe { libc::fcntl(pipe.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0
    {
        return Err(failure(std::io::Error::last_os_error()));
    }
    Ok(())
}

/// Windows Job 终止全部成员后管道自动解阻，保持系统管道默认行为。
#[cfg(not(unix))]
fn nonblocking<T>(_pipe: &T) -> Result<()> {
    Ok(())
}

/// 使用非阻塞队列投递，使接收方早退或 panic 时生产线程仍能退出。
fn emit(sender: &SyncSender<Event>, stop: &AtomicBool, mut event: Event) -> bool {
    while !stop.load(Ordering::Acquire) {
        match sender.try_send(event) {
            Ok(()) => return true,
            Err(mpsc::TrySendError::Disconnected(_)) => return false,
            Err(mpsc::TrySendError::Full(value)) => {
                event = value;
                std::thread::sleep(POLL);
            }
        }
    }
    false
}

/// 按字节有界累积完整行，拒绝半行 EOF，正确处理任意 UTF-8 分片。
fn read_stdout(
    mut pipe: impl Read,
    sender: SyncSender<Event>,
    stop: Arc<AtomicBool>,
    limit: usize,
) {
    let mut buffer = [0; 8192];
    let mut line = Vec::new();
    while !stop.load(Ordering::Acquire) {
        match pipe.read(&mut buffer) {
            Ok(0) => {
                emit(
                    &sender,
                    &stop,
                    if line.is_empty() {
                        Event::StdoutEof
                    } else {
                        Event::Error("CodeGraph stdout 半行 EOF".into())
                    },
                );
                return;
            }
            Ok(count) => {
                for byte in &buffer[..count] {
                    if *byte == b'\n' {
                        if !emit(&sender, &stop, Event::Line(std::mem::take(&mut line))) {
                            return;
                        }
                    } else {
                        if line.len() >= limit {
                            emit(
                                &sender,
                                &stop,
                                Event::Error("CodeGraph 结果超过字节上限".into()),
                            );
                            return;
                        }
                        line.push(*byte);
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(POLL)
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                emit(&sender, &stop, Event::Error(error.to_string()));
                return;
            }
        }
    }
}

/// 持续排空 stderr，但保留总字节上限，不让诊断堵塞或无限增长。
fn read_stderr(
    mut pipe: impl Read,
    sender: SyncSender<Event>,
    stop: Arc<AtomicBool>,
    limit: usize,
) {
    let mut buffer = [0; 8192];
    let mut diagnostics = Vec::new();
    while !stop.load(Ordering::Acquire) {
        match pipe.read(&mut buffer) {
            Ok(0) => {
                emit(&sender, &stop, Event::StderrEof(diagnostics));
                return;
            }
            Ok(count) => {
                if count > limit.saturating_sub(diagnostics.len()) {
                    emit(
                        &sender,
                        &stop,
                        Event::Error("CodeGraph 诊断超过字节上限".into()),
                    );
                    return;
                }
                diagnostics.extend_from_slice(&buffer[..count]);
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(POLL)
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                emit(&sender, &stop, Event::Error(error.to_string()));
                return;
            }
        }
    }
}

/// 错误或取消后仍排空管道；终止树后遇到无数据即可退出，不再向已关闭队列投递。
fn drain_pipe(mut pipe: impl Read, stop: &AtomicBool) {
    let mut buffer = [0; 8192];
    loop {
        match pipe.read(&mut buffer) {
            Ok(0) => return,
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                if stop.load(Ordering::Acquire) {
                    return;
                }
                std::thread::sleep(POLL);
            }
            Err(_) => return,
        }
    }
}

/// 写线程只接受一个请求，刷新后释放 stdin；正常 EOF 是语言程序开始处理的边界。
fn write_stdin(
    mut pipe: impl Write,
    bytes: Vec<u8>,
    sender: SyncSender<Event>,
    stop: Arc<AtomicBool>,
) {
    let mut remaining = bytes.as_slice();
    while !remaining.is_empty() && !stop.load(Ordering::Acquire) {
        match pipe.write(remaining) {
            Ok(0) => {
                emit(
                    &sender,
                    &stop,
                    Event::Error("CodeGraph stdin 写入零字节".into()),
                );
                return;
            }
            Ok(count) => remaining = &remaining[count..],
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(POLL)
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                emit(&sender, &stop, Event::Error(error.to_string()));
                return;
            }
        }
    }
    if remaining.is_empty() {
        if let Err(error) = pipe.flush() {
            emit(&sender, &stop, Event::Error(error.to_string()));
            return;
        }
        drop(pipe);
        emit(&sender, &stop, Event::Written);
    }
}
