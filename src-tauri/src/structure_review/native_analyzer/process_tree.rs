//! 自有进程组或 Windows Job 的回收守卫，父进程退出不能释放后代所有权。

use super::{failure, Result};
use std::process::{Child, Command, ExitStatus, Stdio};
#[cfg(windows)]
#[path = "windows_process.rs"]
mod windows_process;
#[cfg(unix)]
use std::time::{Duration, Instant};
#[cfg(all(test, windows))]
pub(crate) use windows_process::NativeWindowsJob as ValidationWindowsJob;

/// 持有 helper 及其后代直到显式回收；Drop 覆盖 panic 和早退。
pub(super) struct ProcessTree {
    pub child: Child,
    reaped: bool,
    #[cfg(windows)]
    job: windows_process::NativeWindowsJob,
}

impl ProcessTree {
    /// 在 helper 执行前建立组所有权，关闭创建和归组之间的逃逸窗口。
    pub fn spawn(command: &mut Command) -> Result<Self> {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        let (child, job) = windows_process::NativeWindowsJob::spawn(command).map_err(failure)?;
        #[cfg(not(windows))]
        let child = command.spawn().map_err(failure)?;
        Ok(Self {
            child,
            reaped: false,
            #[cfg(windows)]
            job,
        })
    }

    /// 查询父进程退出状态，不因它退出而忘记所属进程树。
    pub fn status(&mut self) -> Result<Option<ExitStatus>> {
        self.child.try_wait().map_err(failure)
    }

    /// 无论终结结果如何都终止遗留后代并确认组消失，然后才允许线程和 scratch 释放。
    pub fn reap(&mut self) -> Result<()> {
        if self.reaped {
            return Ok(());
        }
        #[cfg(unix)]
        let members = crate::lsp::native_group_members(self.child.id() as i32);
        #[cfg(unix)]
        // SAFETY: child 由 process_group(0) 创建，负 PID 只指向自有进程组。
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        #[cfg(windows)]
        let job_result = self.job.terminate_and_wait().map_err(failure);
        let _ = self.child.kill();
        let child_result = self.child.wait().map_err(failure);
        #[cfg(windows)]
        job_result?;
        child_result?;
        #[cfg(unix)]
        {
            let deadline = Instant::now() + Duration::from_secs(2);
            loop {
                // SAFETY: 信号 0 仅查询自有组及回收前记录的后代 PID，不发出终止信号。
                let alive = unsafe { libc::kill(-(self.child.id() as i32), 0) } == 0
                    || members
                        .iter()
                        .any(|pid| unsafe { libc::kill(*pid, 0) } == 0);
                if !alive {
                    break;
                }
                if Instant::now() >= deadline {
                    return Err(failure("helper 进程树回收超时"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        self.reaped = true;
        Ok(())
    }
}

impl Drop for ProcessTree {
    /// panic 或启动中途失败仍同步终止自有进程树。
    fn drop(&mut self) {
        let _ = self.reap();
    }
}
