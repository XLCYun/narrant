//! 有界 stdio 与阶段状态机；不等待无限 EOF，也不把 completed 提前交付。

#[path = "process_tree.rs"]
mod process_tree;

use super::{
    cancelled, failure,
    input::{self, CheckedInput},
    protocol::{self, Completed, Message},
    NativeLanguageAnalyzer, Result,
};
use crate::structure_review::code_graph::CodeGraph;
use process_tree::ProcessTree;
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    path::Path,
    process::Command,
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
    /// 启动清空环境的固定工具，工作目录和临时目录均为仓库外 scratch。
    pub(super) fn start(
        analyzer: &NativeLanguageAnalyzer,
        input: &CheckedInput,
        scratch: &Path,
    ) -> Result<Self> {
        let mut command = Command::new(&input.executable);
        command
            .args(&analyzer.command.args)
            .env_clear()
            .envs(&analyzer.command.environment)
            .current_dir(scratch)
            .env("TMPDIR", scratch)
            .env("TMP", scratch)
            .env("TEMP", scratch);
        let process = ProcessTree::spawn(&mut command)?;
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
            .child
            .stdout
            .take()
            .ok_or_else(|| failure("helper 缺少 stdout"))?;
        let stderr = session
            .process
            .child
            .stderr
            .take()
            .ok_or_else(|| failure("helper 缺少 stderr"))?;
        let stdin = session
            .process
            .child
            .stdin
            .take()
            .ok_or_else(|| failure("helper 缺少 stdin"))?;
        nonblocking(&stdout)?;
        nonblocking(&stderr)?;
        nonblocking(&stdin)?;
        let stop = session.stop.clone();
        let sender = events.clone();
        let limit = analyzer.limits.max_result_bytes;
        session.workers.push(
            std::thread::Builder::new()
                .name("native-stdout".into())
                .spawn(move || read_stdout(stdout, sender, stop, limit))
                .map_err(failure)?,
        );
        let stop = session.stop.clone();
        let sender = events.clone();
        let limit = analyzer.limits.max_diagnostic_bytes;
        session.workers.push(
            std::thread::Builder::new()
                .name("native-stderr".into())
                .spawn(move || read_stderr(stderr, sender, stop, limit))
                .map_err(failure)?,
        );
        let stop = session.stop.clone();
        session.workers.push(
            std::thread::Builder::new()
                .name("native-stdin".into())
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
            .ok_or_else(|| failure("helper stdin 已关闭"))?
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
        let cleanup = self.process.reap();
        self.stop.store(true, Ordering::Release);
        self.incoming.take();
        let mut panicked = false;
        for worker in self.workers.drain(..) {
            panicked |= worker.join().is_err();
        }
        cleanup?;
        if panicked {
            return Err(failure("helper 读写线程异常退出"));
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
    analyzer: &NativeLanguageAnalyzer,
    input: &CheckedInput,
    request: &Value,
    scratch: &Path,
) -> Result<CodeGraph> {
    let mut session = Session::start(analyzer, input, scratch)?;
    let result = exchange(&mut session, analyzer, input, request);
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
    analyzer: &NativeLanguageAnalyzer,
    input: &CheckedInput,
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
                            return Err(failure("helper 重复发送 ready"));
                        }
                        let languages = input::unique(&ready.languages, "ready 语言")?;
                        let profiles = input::unique(&ready.profiles, "ready profile")?;
                        if ready.protocol != protocol::PROTOCOL
                            || ready.message_type != "ready"
                            || ready.helper_id != analyzer.identity.helper_id
                            || ready.helper_version != analyzer.identity.helper_version
                            || ready.projection_revision != analyzer.identity.projection_revision
                            || languages != input::unique(&analyzer.identity.languages, "预期语言")?
                            || !profiles.contains(analyzer.identity.profile.as_str())
                        {
                            return Err(failure("helper ready 与固定支持包身份或政策不匹配"));
                        }
                        session.send(request)?;
                        running = true;
                        deadline = Instant::now() + analyzer.limits.analysis_timeout;
                    }
                    message => {
                        if !running {
                            return Err(failure("helper 在 analyze 前发送 completed"));
                        }
                        let result = completed(
                            message,
                            cancellation_deadline.is_some() || (analyzer.cancelled)(),
                        )?;
                        session.close_input();
                        deadline = Instant::now() + analyzer.limits.exit_timeout;
                        terminal = Some(result.and_then(|completed| {
                            super::graph::build(completed, input, &analyzer.identity)
                        }));
                    }
                }
            }
            Ok(Event::StdoutEof) => {
                stdout_eof = true;
                if terminal.is_none() {
                    return Err(failure("helper 在完整 completed 前关闭 stdout"));
                }
            }
            Ok(Event::StderrEof(bytes)) => {
                stderr_eof = true;
                diagnostics = bytes;
            }
            Ok(Event::Error(error)) => return preserve(terminal, failure(error)),
            Ok(Event::Written) => analyze_written = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) if !stdout_eof || !stderr_eof => {
                return preserve(terminal, failure("helper 读写线程意外关闭"))
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
        if let Some(status) = session.process.status()? {
            if !status.success() && stdout_eof && stderr_eof {
                return preserve(
                    terminal,
                    failure(format!(
                        "helper 异常退出 {status}：{}",
                        String::from_utf8_lossy(&diagnostics)
                    )),
                );
            }
            if stdout_eof && stderr_eof && analyze_written {
                return match terminal {
                    Some(Err(error)) => Err(error),
                    Some(Ok(_)) if cancellation_deadline.is_some() => Err(cancelled()),
                    Some(Ok(completed)) => Ok(completed),
                    None => Err(failure("helper 缺少终结响应")),
                };
            }
        }
        if cancellation_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            return preserve(terminal, cancelled());
        }
        if cancellation_deadline.is_none() && Instant::now() >= deadline {
            let reason = if terminal.is_some() {
                "helper 退出超时"
            } else if running {
                "helper 分析超时"
            } else {
                "helper 启动超时"
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
            &value.id,
            &value.outcome,
        ),
        Message::Error(value) => (
            &value.protocol,
            &value.message_type,
            &value.id,
            &value.outcome,
        ),
        Message::Cancelled(value) => (
            &value.protocol,
            &value.message_type,
            &value.id,
            &value.outcome,
        ),
        Message::Ready(_) => return Err(failure("终结阶段不能接收 ready")),
    };
    if version != protocol::PROTOCOL
        || kind != "completed"
        || id != protocol::REQUEST_ID
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
                "unsupported_configuration",
                "target_unavailable",
                "object_extraction_failed",
                "union_conflict",
                "resource_limit",
                "internal_failure",
            ]
            .contains(&error.code.as_str())
                || error.message.is_empty()
            {
                return Err(failure("helper 错误码或消息无效"));
            }
            for path in &error.paths {
                input::relative(path)?;
            }
            Err(failure(format!(
                "{}：{} [{}]",
                error.code,
                error.message,
                error.paths.join(", ")
            )))
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
                        Event::Error("helper stdout 半行 EOF".into())
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
                                Event::Error("helper 结果超过字节上限".into()),
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
                        Event::Error("helper 诊断超过字节上限".into()),
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

/// 独占 stdin，以可取消分片写入避免 helper 停止读取导致宿主卡住。
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
                        Event::Error("helper stdin 写入零字节".into()),
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
