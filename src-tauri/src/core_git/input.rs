//! Core Git 标准输入的非阻塞写入与确定性回收。

use super::*;

/// 管理标准输入写线程；子进程正常退出或终止后必须回收线程。
pub(super) struct InputWriter {
    worker: Option<thread::JoinHandle<std::io::Result<()>>>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl InputWriter {
    /// 让输入等待留在可取消的进程循环内，避免成功退出后无限阻塞于 join。
    pub(super) fn is_finished(&self) -> bool {
        self.worker
            .as_ref()
            .is_none_or(thread::JoinHandle::is_finished)
    }

    /// 独立写管道，避免输入与输出缓冲区同时填满时互相等待。
    pub(super) fn start(
        mut stdin: std::process::ChildStdin,
        input: Vec<u8>,
        events: mpsc::Sender<ReaderEvent>,
    ) -> Self {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_stop = stop.clone();
        let worker = thread::spawn(move || {
            let result = write_input(&mut stdin, &input, &worker_stop);
            if let Err(error) = &result {
                let _ = events.send(ReaderEvent::InputFailed(error.to_string()));
            }
            result
        });
        Self {
            worker: Some(worker),
            stop,
        }
    }

    /// 在进程结束后检查输入是否完整写入，线程 panic 也保留为稳定失败类别。
    pub(super) fn finish(mut self) -> Result<(), CoreGitFailure> {
        self.worker
            .take()
            .expect("input writer is joined once")
            .join()
            .map_err(|_| {
                CoreGitFailure::from_kind(
                    CoreGitFailureKind::InputWriteFailed,
                    "Core Git 输入写入线程异常",
                )
            })?
            .map_err(|error| {
                CoreGitFailure::from_kind(
                    CoreGitFailureKind::InputWriteFailed,
                    format!("写入 Core Git 输入失败：{error}"),
                )
            })
    }
}

impl Drop for InputWriter {
    /// 请求非阻塞写循环停止，回收自己持有的管道；不依赖后代进程是否仍持有读端。
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Release);
        if let Some(writer) = self.worker.take() {
            let _ = writer.join();
        }
    }
}

/// 非阻塞地推进有限输入，满管道时短暂让出线程并持续观察本执行器的停止信号。
fn write_input(
    stdin: &mut std::process::ChildStdin,
    mut input: &[u8],
    stop: &std::sync::atomic::AtomicBool,
) -> std::io::Result<()> {
    use std::io::{ErrorKind, Write};
    set_input_nonblocking(stdin)?;
    while !input.is_empty() {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Err(std::io::Error::new(
                ErrorKind::Interrupted,
                "Core Git 输入写入已停止",
            ));
        }
        match stdin.write(input) {
            Ok(0) => thread::sleep(Duration::from_millis(10)),
            Ok(count) => input = &input[count..],
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(10))
            }
            Err(error) if error.kind() == ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// 只改变父进程持有的 Unix 写端，避免逃离进程组的后代把写线程永久阻塞。
#[cfg(unix)]
fn set_input_nonblocking(stdin: &std::process::ChildStdin) -> std::io::Result<()> {
    use std::os::fd::AsRawFd;
    let flags = unsafe { libc::fcntl(stdin.as_raw_fd(), libc::F_GETFL) };
    if flags == -1
        || unsafe { libc::fcntl(stdin.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) } == -1
    {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Windows 的匿名管道同样可通过 SetNamedPipeHandleState 切换成立即返回的字节写模式。
#[cfg(windows)]
fn set_input_nonblocking(stdin: &std::process::ChildStdin) -> std::io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Pipes::{SetNamedPipeHandleState, PIPE_NOWAIT};
    let mode = PIPE_NOWAIT; // 保留默认的 PIPE_READMODE_BYTE。
    let result = unsafe {
        SetNamedPipeHandleState(
            stdin.as_raw_handle(),
            &mode,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
