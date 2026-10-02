//! stdio 帧传输和子进程回收，不解释任何语言或 LSP 扩展。

use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant},
};

use serde_json::Value;

use super::{failure, CancellationCheck, Result};

#[cfg(windows)]
#[path = "windows_process.rs"]
pub(crate) mod windows_process;

const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;
const MAX_HEADER_BYTES: usize = 16 * 1024;

/// 写线程回传发送结果，避免服务器停止读取时卡住分析调用线程。
type Outgoing = (Vec<u8>, mpsc::Sender<Result<()>>);

/// 回收服务器及其启动的构建进程，覆盖初始化失败等提前返回路径。
struct ServerProcess {
    child: Child,
    reaped: bool,
    #[cfg(windows)]
    job: windows_process::WindowsJob,
}

impl ServerProcess {
    /// 正常退出给服务器短暂时间处理 exit；最终仍由守卫检查并清空进程组。
    fn finish(&mut self) -> Result<()> {
        #[cfg(unix)]
        let members = group_members(self.child.id() as i32);
        let deadline = Instant::now() + Duration::from_millis(300);
        while self.child.try_wait().map_err(failure)?.is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        #[cfg(unix)]
        {
            self.reap_members(members)
        }
        #[cfg(not(unix))]
        {
            self.reap()
        }
    }

    /// 直接子进程退出不代表后代已退出，等待整组消失后才允许释放项目目录。
    fn reap(&mut self) -> Result<()> {
        if self.reaped {
            return Ok(());
        }
        #[cfg(unix)]
        {
            self.reap_members(group_members(self.child.id() as i32))
        }
        #[cfg(not(unix))]
        {
            self.reap_members(Vec::new())
        }
    }

    /// 保存退出前的后代 PID；macOS 的僵尸进程可能已离开进程组但尚未被系统回收。
    fn reap_members(&mut self, _members: Vec<i32>) -> Result<()> {
        if self.reaped {
            return Ok(());
        }
        #[cfg(unix)]
        // SAFETY: 本次会话以 process_group(0) 创建，负 PID 只指向自有进程组。
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        #[cfg(windows)]
        self.job.terminate_and_wait().map_err(failure)?;
        let _ = self.child.kill();
        self.child.wait().map_err(failure)?;
        #[cfg(unix)]
        {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                // SAFETY: 信号 0 仅检查自有进程组是否仍存在，不发送终止信号。
                let exists = unsafe { libc::kill(-(self.child.id() as i32), 0) } == 0;
                // SAFETY: 这些 PID 来自关闭前的自有进程组快照，只查询退出状态。
                let descendants_exist = _members
                    .iter()
                    .any(|pid| unsafe { libc::kill(*pid, 0) } == 0);
                if !exists && !descendants_exist {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(failure("LSP 进程组回收超时"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        self.reaped = true;
        Ok(())
    }
}

impl Drop for ServerProcess {
    /// 取消、错误和 panic 也等待全部后代停止，避免临时视图先于 tsserver 被释放。
    fn drop(&mut self) {
        let _ = self.reap();
    }
}

/// 一次进程连接的有界读写通道，生命周期由上层会话独占。
pub(super) struct Transport {
    _process: ServerProcess,
    outgoing: SyncSender<Outgoing>,
    incoming: Receiver<Result<Value>>,
}

impl Transport {
    /// 等待正常关闭并确认整个服务器组退出，关闭确认本身不能代替进程回收。
    pub fn finish(&mut self) -> Result<()> {
        self._process.finish()
    }

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
        #[cfg(windows)]
        let (child, job) = windows_process::WindowsJob::spawn(&mut command)
            .map_err(|error| failure(format!("无法建立 Windows LSP Job 所有权：{error}")))?;
        #[cfg(not(windows))]
        let child = command
            .spawn()
            .map_err(|error| failure(format!("无法启动 LSP 服务器：{error}")))?;
        let mut process = ServerProcess {
            child,
            reaped: false,
            #[cfg(windows)]
            job,
        };
        let mut stdin = process
            .child
            .stdin
            .take()
            .ok_or_else(|| failure("LSP 服务器缺少输入流"))?;
        let stdout = process
            .child
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
    pub fn send(
        &mut self,
        message: Value,
        timeout: Duration,
        cancelled: &CancellationCheck,
    ) -> Result<()> {
        if cancelled() {
            return Err(super::LspError::cancelled());
        }
        let bytes = serde_json::to_vec(&message).map_err(failure)?;
        let (complete, completed) = mpsc::channel();
        self.outgoing.try_send((bytes, complete)).map_err(failure)?;
        wait(&completed, timeout, cancelled)?
    }

    /// 等待下一帧；EOF 和超时都通过同一个错误出口交给会话。
    pub fn receive(&mut self, timeout: Duration, cancelled: &CancellationCheck) -> Result<Value> {
        wait(&self.incoming, timeout, cancelled)?
    }
}

/// 短等待片段响应取消，同时保留整个读写阶段的固定截止时间。
fn wait<T>(receiver: &Receiver<T>, timeout: Duration, cancelled: &CancellationCheck) -> Result<T> {
    let deadline = Instant::now() + timeout;
    loop {
        if cancelled() {
            return Err(super::LspError::cancelled());
        }
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| failure("等待 LSP 消息超时"))?;
        match receiver.recv_timeout(remaining.min(Duration::from_millis(50))) {
            Ok(value) => return Ok(value),
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(error) => return Err(failure(error)),
        }
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

/// macOS 原生进程枚举保留关闭前的组成员，避免启动 ps 辅助进程参与回收。
#[cfg(target_os = "macos")]
pub(crate) fn group_members(group: i32) -> Vec<i32> {
    let mut capacity = 64usize;
    loop {
        let mut pids = vec![0i32; capacity];
        // SAFETY: PROC_PGRP_ONLY=2 来自系统 proc_info.h；缓冲区按 PID 宽度分配。
        let bytes = unsafe {
            libc::proc_listpids(
                2,
                group as u32,
                pids.as_mut_ptr().cast(),
                (pids.len() * std::mem::size_of::<i32>()) as i32,
            )
        };
        if bytes <= 0 {
            return Vec::new();
        }
        let count = bytes as usize / std::mem::size_of::<i32>();
        if count < capacity {
            pids.truncate(count);
            pids.retain(|pid| *pid > 0);
            return pids;
        }
        capacity *= 2;
    }
}

/// Linux 从 procfs 查询同组 PID，不读取任何被分析项目数据。
#[cfg(target_os = "linux")]
pub(crate) fn group_members(group: i32) -> Vec<i32> {
    std::fs::read_dir("/proc")
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse::<i32>().ok()?;
            // SAFETY: getpgid 只读取候选 PID 的进程组。
            (unsafe { libc::getpgid(pid) } == group).then_some(pid)
        })
        .collect()
}

/// 其他 Unix 平台仍以进程组信号检查作为回收依据。
#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
pub(crate) fn group_members(_group: i32) -> Vec<i32> {
    Vec::new()
}
