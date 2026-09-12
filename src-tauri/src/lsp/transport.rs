//! stdio 帧传输和子进程回收，不解释任何语言或 LSP 扩展。

use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender},
    time::Duration,
};

use serde_json::Value;

use super::{failure, Result};

const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 16 * 1024;

/// 写线程回传发送结果，避免服务器停止读取时卡住分析调用线程。
type Outgoing = (Vec<u8>, mpsc::Sender<Result<()>>);

/// 回收服务器及其启动的构建进程，覆盖初始化失败等提前返回路径。
struct ServerProcess(Child);

impl Drop for ServerProcess {
    /// 回收本次服务器的进程树，并等待直接子进程退出。
    fn drop(&mut self) {
        #[cfg(unix)]
        // SAFETY: 该子进程由下方 process_group(0) 创建，负 PID 只指向本次会话的进程组。
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        #[cfg(windows)]
        {
            let mut command = Command::new("taskkill");
            command
                .args(["/PID", &self.0.id().to_string(), "/T", "/F"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            crate::child_process::configure_std_command(&mut command);
            let _ = command.status();
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// 一次进程连接的有界读写通道，生命周期由上层会话独占。
pub(super) struct Transport {
    _process: ServerProcess,
    outgoing: SyncSender<Outgoing>,
    incoming: Receiver<Result<Value>>,
}

impl Transport {
    /// 接收调用方配置好的命令，统一接管 stdio 和进程组。
    pub fn start(mut command: Command) -> Result<Self> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        crate::child_process::configure_std_command(&mut command);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut process = ServerProcess(
            command
                .spawn()
                .map_err(|error| failure(format!("无法启动 LSP 服务器：{error}")))?,
        );
        let mut stdin = process
            .0
            .stdin
            .take()
            .ok_or_else(|| failure("LSP 服务器缺少输入流"))?;
        let stdout = process
            .0
            .stdout
            .take()
            .ok_or_else(|| failure("LSP 服务器缺少输出流"))?;
        let (sender, incoming) = mpsc::sync_channel(64);
        std::thread::Builder::new()
            .name("lsp-read".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let message = read_message(&mut reader);
                    let failed = message.is_err();
                    if sender.send(message).is_err() || failed {
                        break;
                    }
                }
            })
            .map_err(failure)?;
        let (outgoing, writes) = mpsc::sync_channel::<Outgoing>(1);
        std::thread::Builder::new()
            .name("lsp-write".into())
            .spawn(move || {
                while let Ok((bytes, complete)) = writes.recv() {
                    let result = (|| {
                        write!(stdin, "Content-Length: {}\r\n\r\n", bytes.len())?;
                        stdin.write_all(&bytes)?;
                        stdin.flush()
                    })()
                    .map_err(failure);
                    let failed = result.is_err();
                    if complete.send(result).is_err() || failed {
                        break;
                    }
                }
            })
            .map_err(failure)?;
        Ok(Self {
            _process: process,
            outgoing,
            incoming,
        })
    }

    /// Content-Length 按 UTF-8 字节计数，避免中文和 emoji 导致协议错帧。
    pub fn send(&mut self, message: Value, timeout: Duration) -> Result<()> {
        let bytes = serde_json::to_vec(&message).map_err(failure)?;
        let (complete, completed) = mpsc::channel();
        self.outgoing.try_send((bytes, complete)).map_err(failure)?;
        completed
            .recv_timeout(timeout)
            .map_err(|error| failure(format!("向 LSP 服务器发送消息失败：{error}")))?
    }

    /// 等待下一帧；EOF 和超时都通过同一个错误出口交给会话。
    pub fn receive(&mut self, timeout: Duration) -> Result<Value> {
        self.incoming.recv_timeout(timeout).map_err(failure)?
    }
}

/// 读取有明确大小上限的 LSP 消息，拒绝缺失、重复或畸形的长度头。
fn read_message(reader: &mut impl BufRead) -> Result<Value> {
    let mut length = None;
    let mut header_bytes = 0;
    loop {
        let mut line = String::new();
        let count = (&mut *reader)
            .take((MAX_HEADER_BYTES - header_bytes + 1) as u64)
            .read_line(&mut line)
            .map_err(failure)?;
        if count == 0 {
            return Err(failure("LSP 服务器关闭了输出流"));
        }
        header_bytes += count;
        if header_bytes > MAX_HEADER_BYTES {
            return Err(failure("LSP 消息头过长"));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| failure("LSP 消息头格式错误"))?;
        if key.eq_ignore_ascii_case("Content-Length") {
            if length.is_some() {
                return Err(failure("LSP Content-Length 重复"));
            }
            length = Some(value.trim().parse::<usize>().map_err(failure)?);
        }
    }
    let length = length
        .filter(|length| *length <= MAX_MESSAGE_BYTES)
        .ok_or_else(|| failure("LSP 消息缺少有效 Content-Length 或消息过大"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).map_err(failure)?;
    serde_json::from_slice(&body).map_err(failure)
}
