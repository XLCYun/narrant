//! Codex App Server 的子进程与 JSONL 字节流连接。
//!
//! 这个模块只处理进程 stdio、逐行 framing、受限 I/O 和生命周期。JSON-RPC
//! request id、notification 队列以及 endpoint 语义由上层 transport 模块负责。

use super::super::{AppServerCommand, RpcFailure, CODEX_PROCESS_REAP_TIMEOUT};
use crate::babysit::CancellationSignal;
use serde_json::Value;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};

/// 单条 JSONL 线的最大字节数，防止无换行进程输出无限增长。
const MAX_JSONL_LINE_BYTES: usize = 8 * 1024 * 1024;
/// 有界 I/O 每次等待的上限，用于及时观察取消信号。
const IO_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// 拥有一个 Codex App Server 子进程及其双向 JSONL stdio。
pub(crate) struct JsonlConnection {
    /// 保留 child handle，确保连接释放时可以 kill 并 wait。
    child: Option<Child>,
    /// 向 App Server 写入 JSONL request 或 notification。
    stdin: tokio::process::ChildStdin,
    /// 从 App Server 读取带换行边界的 JSONL message。
    stdout: BufReader<tokio::process::ChildStdout>,
}

impl JsonlConnection {
    /// 启动 stdio App Server，并在部分建构失败时回收已经创建的 child。
    pub(super) async fn spawn(command: &AppServerCommand) -> Result<Self, RpcFailure> {
        let mut process = Command::new(&command.program);
        process
            .args(&command.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        crate::child_process::configure_tokio_command(&mut process);
        let mut child = process.spawn().map_err(|_| RpcFailure::Io)?;
        let Some(stdin) = child.stdin.take() else {
            reap_child(child);
            return Err(RpcFailure::Io);
        };
        let Some(stdout) = child.stdout.take() else {
            reap_child(child);
            return Err(RpcFailure::Io);
        };
        Ok(Self {
            child: Some(child),
            stdin,
            stdout: BufReader::new(stdout),
        })
    }

    /// 在截止时间内 kill 并 wait 子进程，保证显式 shutdown 不留下僵尸。
    pub(super) async fn shutdown(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_kill();
            if tokio::time::timeout(CODEX_PROCESS_REAP_TIMEOUT, child.wait())
                .await
                .is_err()
            {
                reap_child(child);
            }
        }
    }

    /// 读取一条有大小上限的 JSONL，并在每次等待之间观察取消和 deadline。
    pub(super) async fn read_message(
        &mut self,
        deadline: Instant,
        cancellation: Option<&CancellationSignal>,
    ) -> Result<Value, RpcFailure> {
        read_json_line(&mut self.stdout, deadline, cancellation).await
    }

    /// 以单行 JSONL 写入 message，处理 partial write、flush、deadline 和取消。
    pub(super) async fn write_message(
        &mut self,
        value: Value,
        deadline: Instant,
        cancellation: Option<&CancellationSignal>,
    ) -> Result<(), RpcFailure> {
        check_io_deadline(deadline, cancellation)?;
        let mut encoded = serde_json::to_vec(&value).map_err(|_| RpcFailure::Protocol)?;
        encoded.push(b'\n');
        if encoded.len() > MAX_JSONL_LINE_BYTES {
            return Err(RpcFailure::Protocol);
        }
        write_json_line(&mut self.stdin, &encoded, deadline, cancellation).await
    }
}

impl Drop for JsonlConnection {
    /// 丢弃连接时立即请求 kill，并把 wait 交给 runtime 或短时同步 reaper。
    fn drop(&mut self) {
        if let Some(child) = self.child.take() {
            reap_child(child);
        }
    }
}

/// 在 stdout 上累积一条有限长度的 JSONL，避免 `read_line` 对恶意输出无限分配。
async fn read_json_line<R>(
    reader: &mut R,
    deadline: Instant,
    cancellation: Option<&CancellationSignal>,
) -> Result<Value, RpcFailure>
where
    R: AsyncBufRead + Unpin,
{
    let mut line = Vec::new();
    loop {
        check_io_deadline(deadline, cancellation)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        let wait = remaining.min(IO_POLL_INTERVAL);
        match tokio::time::timeout(wait, reader.fill_buf()).await {
            Ok(Ok(buffer)) if buffer.is_empty() => {
                if line.is_empty() {
                    return Err(RpcFailure::ProcessExited);
                }
                return parse_json_line(&line);
            }
            Ok(Ok(buffer)) => {
                let consumed = buffer
                    .iter()
                    .position(|byte| *byte == b'\n')
                    .map_or(buffer.len(), |index| index + 1);
                let complete = buffer.get(consumed.saturating_sub(1)) == Some(&b'\n');
                if line.len().saturating_add(consumed) > MAX_JSONL_LINE_BYTES {
                    return Err(RpcFailure::InvalidJson);
                }
                line.extend_from_slice(&buffer[..consumed]);
                reader.consume(consumed);
                if complete {
                    return parse_json_line(&line);
                }
            }
            Ok(Err(_)) => return Err(RpcFailure::Io),
            Err(_) => continue,
        }
    }
}

/// 将一条已收集的 JSONL 字节序列解析为无原始内容泄露的协议值。
fn parse_json_line(line: &[u8]) -> Result<Value, RpcFailure> {
    let text = std::str::from_utf8(line).map_err(|_| RpcFailure::InvalidJson)?;
    serde_json::from_str(text.trim()).map_err(|_| RpcFailure::InvalidJson)
}

/// 循环处理 partial write 与 flush，每次最多等待 100ms 以观察取消。
async fn write_json_line<W>(
    writer: &mut W,
    encoded: &[u8],
    deadline: Instant,
    cancellation: Option<&CancellationSignal>,
) -> Result<(), RpcFailure>
where
    W: AsyncWrite + Unpin,
{
    let mut offset = 0;
    while offset < encoded.len() {
        check_io_deadline(deadline, cancellation)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        let wait = remaining.min(IO_POLL_INTERVAL);
        match tokio::time::timeout(wait, writer.write(&encoded[offset..])).await {
            Ok(Ok(0)) => return Err(RpcFailure::ProcessExited),
            Ok(Ok(written)) => offset += written,
            Ok(Err(_)) => return Err(RpcFailure::Io),
            Err(_) => continue,
        }
    }
    loop {
        check_io_deadline(deadline, cancellation)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        let wait = remaining.min(IO_POLL_INTERVAL);
        match tokio::time::timeout(wait, writer.flush()).await {
            Ok(Ok(())) => return Ok(()),
            Ok(Err(_)) => return Err(RpcFailure::Io),
            Err(_) => continue,
        }
    }
}

/// 检查一次 I/O 是否仍处于调用方的取消状态与总 deadline 内。
fn check_io_deadline(
    deadline: Instant,
    cancellation: Option<&CancellationSignal>,
) -> Result<(), RpcFailure> {
    if cancellation.is_some_and(CancellationSignal::is_cancelled) {
        return Err(RpcFailure::Cancelled);
    }
    if Instant::now() >= deadline {
        return Err(RpcFailure::Timeout);
    }
    Ok(())
}

/// partial construction 失败时 kill 并安排 wait，避免丢失 Child handle。
fn reap_child(mut child: Child) {
    let _ = child.start_kill();
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn(async move {
            let _ = child.wait().await;
        });
    } else {
        std::thread::spawn(move || {
            for _ in 0..50 {
                if child.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            let _ = child.start_kill();
            let _ = child.try_wait();
        });
    }
}

#[cfg(test)]
#[path = "../../tests/codex/transport/jsonl_connection.rs"]
mod tests;
