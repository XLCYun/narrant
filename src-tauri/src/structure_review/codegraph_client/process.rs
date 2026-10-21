//! 有界 stdio 与阶段状态机；不等待无限 EOF，也不把 completed 提前交付。

use super::{
    cancelled, failure,
    protocol::{self, Completed, Message},
    CodeGraphAnalyzer, Result,
};
use crate::child_process::ProcessTree;
use crate::structure_review::code_graph::CodeGraph;
use serde_json::{json, Value};
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
    outgoing: Option<SyncSender<Vec<u8>>>,
    incoming: Option<Receiver<Event>>,
    workers: Vec<JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}

impl Session {
    /// 直接启动对应语言分析器，工作目录和临时目录均为仓库外 scratch。
    pub(super) fn start(analyzer: &CodeGraphAnalyzer, scratch: &Path) -> Result<Self> {
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
        let (outgoing, writes) = mpsc::sync_channel::<Vec<u8>>(2);
        let mut session = Self {
            process,
            outgoing: Some(outgoing),
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
                .spawn(move || read_stdout(stdout, sender, stop, limit))
                .map_err(failure)?,
        );
        let stop = session.stop.clone();
        let sender = events.clone();
        let limit = analyzer.limits.max_diagnostic_bytes;
        session.workers.push(
            std::thread::Builder::new()
                .name("codegraph-stderr".into())
                .spawn(move || read_stderr(stderr, sender, stop, limit))
                .map_err(failure)?,
        );
        let stop = session.stop.clone();
        session.workers.push(
            std::thread::Builder::new()
                .name("codegraph-stdin".into())
                .spawn(move || write_stdin(stdin, writes, events, stop))
                .map_err(failure)?,
        );
        Ok(session)
    }

    /// 写线程执行阻塞风险操作，主线程仅向有限队列投递。
    pub(super) fn send(&self, value: &Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(value).map_err(failure)?;
        bytes.push(b'\n');
        self.outgoing
            .as_ref()
            .ok_or_else(|| failure("CodeGraph stdin 已关闭"))?
            .try_send(bytes)
            .map_err(failure)
    }

    /// 完成或宿主退出时关闭 stdin，写线程排空已接受消息后释放管道。
    pub(super) fn close_input(&mut self) {
        self.outgoing.take();
    }

    /// 回收过程不观察取消；退出或失败后关闭队列并等待每个线程。
    fn finish(&mut self) -> Result<()> {
        self.close_input();
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
    let mut session = Session::start(analyzer, scratch)?;
    let result = exchange(&mut session, analyzer, request);
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

/// ready → running → terminal → EOF/exit 的唯一状态机，日志不会重设期限。
fn exchange(
    session: &mut Session,
    analyzer: &CodeGraphAnalyzer,
    request: &Value,
) -> Result<CodeGraph> {
    let mut deadline = Instant::now() + analyzer.limits.startup_timeout;
    let mut running = false;
    let mut analyze_written = false;
    let mut terminal: Option<Result<CodeGraph>> = None;
    let mut cancellation_deadline = None;
    let mut stdout_eof = false;
    let mut stderr_eof = false;
    let mut diagnostics = Vec::new();
    loop {
        // 先处理已收到的真实消息；同一轮观察的用户取消不能覆盖后端错误。
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
                let message = protocol::decode(&bytes)?;
                match message {
                    Message::Ready(ready) => {
                        if running {
                            return Err(failure("CodeGraph 重复发送 ready"));
                        }
                        if ready.protocol != protocol::PROTOCOL
                            || ready.message_type != "ready"
                            || ready.engine_version != super::ENGINE_VERSION
                            || ready.pack_id != analyzer.id
                        {
                            return Err(failure("CodeGraph ready 与固定引擎、协议或包身份不匹配"));
                        }
                        session.send(request)?;
                        running = true;
                        deadline = Instant::now() + analyzer.limits.analysis_timeout;
                    }
                    message => {
                        if !running {
                            return Err(failure("CodeGraph 在 analyze 前发送 completed"));
                        }
                        let result = completed(
                            message,
                            cancellation_deadline.is_some() || (analyzer.cancelled)(),
                        )?;
                        session.close_input();
                        deadline = Instant::now() + analyzer.limits.exit_timeout;
                        terminal = Some(result.and_then(|completed| {
                            super::graph::build(completed, analyzer.languages())
                        }));
                    }
                }
            }
            Ok(Event::StdoutEof) => {
                stdout_eof = true;
                // 已发 cancel 的不可协作 CodeGraph 可以主动结束自身域；不能把预期 EOF 伪造为后端错误。
                if terminal.is_none() && cancellation_deadline.is_none() {
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
        if (analyzer.cancelled)() && cancellation_deadline.is_none() {
            cancellation_deadline = Some(Instant::now() + analyzer.limits.cancellation_grace);
            if running && terminal.is_none() {
                // 发送失败仍然进入有界清理；不能因此跳过进程树回收。
                let _ = session.send(&json!({"protocol": protocol::PROTOCOL, "type": "cancel", "id": protocol::REQUEST_ID}));
            }
        }
        if let Some(status) = session.process.status().map_err(failure)? {
            if !status.success() && stdout_eof && stderr_eof {
                return preserve(
                    terminal,
                    if cancellation_deadline.is_some() {
                        cancelled()
                    } else {
                        failure(format!(
                            "CodeGraph 异常退出 {status}：{}",
                            String::from_utf8_lossy(&diagnostics)
                        ))
                    },
                );
            }
            if stdout_eof
                && stderr_eof
                && (analyze_written || terminal.as_ref().is_some_and(|value| value.is_err()))
            {
                return match terminal {
                    Some(Err(error)) => Err(error),
                    Some(Ok(_)) if cancellation_deadline.is_some() => Err(cancelled()),
                    Some(Ok(completed)) => Ok(completed),
                    None if cancellation_deadline.is_some() => Err(cancelled()),
                    None => Err(failure("CodeGraph 缺少终结响应")),
                };
            }
        }
        if cancellation_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return preserve(terminal, cancelled());
        }
        if cancellation_deadline.is_none() && Instant::now() >= deadline {
            let reason = if terminal.is_some() {
                "CodeGraph 退出超时"
            } else if running {
                "CodeGraph 分析超时"
            } else {
                "CodeGraph 启动超时"
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

/// 校验终结消息的请求 ID、互斥分支和后端错误字段。
fn completed(message: Message, cancellation_recorded: bool) -> Result<Result<Completed>> {
    let (version, kind, id, outcome) = match &message {
        Message::Ok(value) => (
            &value.protocol,
            &value.message_type,
            Some(value.id.as_str()),
            &value.outcome,
        ),
        Message::Error(value) => (
            &value.protocol,
            &value.message_type,
            value.id.0.as_deref(),
            &value.outcome,
        ),
        Message::Cancelled(value) => (
            &value.protocol,
            &value.message_type,
            Some(value.id.as_str()),
            &value.outcome,
        ),
        Message::Ready(_) => return Err(failure("终结阶段不能接收 ready")),
    };
    if version != protocol::PROTOCOL
        || kind != "completed"
        || id.is_some_and(|id| id != protocol::REQUEST_ID)
        || outcome.is_empty()
    {
        return Err(failure("completed 请求 ID 或封套无效"));
    }
    Ok(match message {
        Message::Ok(value) => Ok(value),
        Message::Cancelled(_) if cancellation_recorded => Err(cancelled()),
        Message::Cancelled(_) => return Err(failure("未请求取消却收到 cancelled")),
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
            {
                return Err(failure("CodeGraph 错误码或消息无效"));
            }
            Err(failure(format!("{}：{}", error.code, error.message)))
        }
        Message::Ready(_) => unreachable!(),
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

/// 独占 stdin，以可取消分片写入避免 CodeGraph 停止读取导致宿主卡住。
fn write_stdin(
    mut pipe: impl Write,
    receiver: Receiver<Vec<u8>>,
    sender: SyncSender<Event>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Acquire) {
        let bytes = match receiver.recv_timeout(POLL) {
            Ok(bytes) => bytes,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
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
        if !emit(&sender, &stop, Event::Written) {
            return;
        }
    }
}
