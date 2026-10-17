//! 外部命令的共享进程树所有权；协议、超时和取消仍由调用方决定。

use std::{
    io,
    process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, ExitStatus},
    time::{Duration, Instant},
};

#[cfg(unix)]
#[path = "child_process/unix_group.rs"]
mod unix_group;

#[cfg(windows)]
#[path = "child_process/windows_job.rs"]
pub(crate) mod windows_job;

#[cfg(test)]
#[path = "child_process/tests/support.rs"]
pub(crate) mod test_support;
#[cfg(all(test, windows))]
#[path = "child_process/tests/windows_validation.rs"]
pub(crate) mod windows_validation;

/// 同时拥有直接 Child 与平台进程树；根已退出也不会释放后代所有权。
pub(crate) struct ProcessTree {
    child: Child,
    reaped: bool,
    failure: Option<io::Error>,
    #[cfg(unix)]
    group: unix_group::ProcessGroup,
    #[cfg(windows)]
    job: windows_job::WindowsJob,
}

impl ProcessTree {
    /// 调用方配置 stdio；helper 第一条指令执行前先建立独立进程组或 Job。
    pub(crate) fn spawn(command: &mut Command) -> io::Result<Self> {
        configure_std_command(command);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        #[cfg(windows)]
        let (child, job) = windows_job::WindowsJob::spawn(command)?;
        #[cfg(not(windows))]
        let child = command.spawn()?;
        let process = Self {
            #[cfg(unix)]
            group: unix_group::ProcessGroup::new(child.id()),
            child,
            reaped: false,
            failure: None,
            #[cfg(windows)]
            job,
        };
        #[cfg(all(test, windows))]
        windows_validation::observe_spawn(&process.child, &process.job)?;
        Ok(process)
    }

    /// 仅转交 stdin 管道，调用方不能绕过进程树所有者释放 Child。
    pub(crate) fn take_stdin(&mut self) -> Option<ChildStdin> {
        self.child.stdin.take()
    }

    /// stdout 读取策略属于协议层，管道转交不会改变进程所有权。
    pub(crate) fn take_stdout(&mut self) -> Option<ChildStdout> {
        self.child.stdout.take()
    }

    /// stderr 的排空与上限由调用方选择，进程回收始终由本守卫负责。
    pub(crate) fn take_stderr(&mut self) -> Option<ChildStderr> {
        self.child.stderr.take()
    }

    /// 只读取根退出状态，不把根已退出标记为整棵树已回收。
    pub(crate) fn status(&mut self) -> io::Result<Option<ExitStatus>> {
        self.child.try_wait()
    }

    /// 给根进程明确的退出宽限；Unix 必须先保存快照再等待，以覆盖离组僵尸。
    pub(crate) fn finish(&mut self, grace: Duration) -> io::Result<()> {
        if !self.reaped {
            #[cfg(unix)]
            self.group.snapshot();
            let deadline = Instant::now() + grace;
            while Instant::now() < deadline {
                match self.status() {
                    Ok(Some(_)) => break,
                    Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                    Err(error) => {
                        self.failure.get_or_insert(error);
                        break;
                    }
                }
            }
        }
        self.reap()
    }

    /// 终止遗留成员并同步等待；平台清理报错也必须执行直接 Child 的 kill/wait。
    pub(crate) fn reap(&mut self) -> io::Result<()> {
        if !self.reaped {
            #[cfg(unix)]
            {
                self.group.snapshot();
                if let Err(error) = self.group.terminate() {
                    self.failure.get_or_insert(error);
                }
            }
            #[cfg(windows)]
            let tree_result = self.job.terminate_and_wait();
            let _ = self.child.kill();
            let child_result = self.child.wait();
            #[cfg(unix)]
            let tree_result = self.group.wait();
            #[cfg(not(any(unix, windows)))]
            let tree_result: io::Result<()> = Ok(());
            self.reaped = tree_result.is_ok() && child_result.is_ok();
            for result in [tree_result, child_result.map(|_| ())] {
                if let Err(error) = result {
                    self.failure.get_or_insert(error);
                }
            }
        }
        match &self.failure {
            Some(error) => Err(io::Error::new(error.kind(), error.to_string())),
            None => Ok(()),
        }
    }

    /// 测试借用准确 owner 检查归属，不向产品调用方开放平台句柄。
    #[cfg(all(test, windows))]
    pub(crate) fn windows_owner_contains(
        &self,
        process: &std::os::windows::io::OwnedHandle,
    ) -> io::Result<bool> {
        windows_validation::contains(&self.job, process)
    }
}

impl Drop for ProcessTree {
    /// 提前返回和 panic 均同步回收整棵树，再允许调用方释放临时目录。
    fn drop(&mut self) {
        let _ = self.reap();
    }
}

#[cfg(test)]
#[path = "child_process/tests/process_tree.rs"]
mod tests;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/** 配置同步子进程；Windows GUI 应用不得为命令行工具创建闪烁的控制台窗口。 */
pub(crate) fn configure_std_command(command: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    #[cfg(not(windows))]
    let _ = command;
}

/** 配置 Tokio 子进程，并与同步命令保持相同的 Windows 无窗口策略。 */
pub(crate) fn configure_tokio_command(command: &mut tokio::process::Command) {
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    #[cfg(not(windows))]
    let _ = command;
}

/// 当前一次性 helper 的专属隔离域；保留外层 Host 所有权，不生成脱离外层的 worker。
#[allow(dead_code)] // 独立 Rust helper 直接复用，宿主只使用 ProcessTree。
pub(crate) struct CurrentProcessDomain {
    #[cfg(unix)]
    group: unix_group::ProcessGroup,
    #[cfg(windows)]
    job: windows_job::CurrentProcessJob,
}

#[allow(dead_code)]
impl CurrentProcessDomain {
    /// ready 之前建立专属域；失败时不能开始原生分析或终止继承的 shell 域。
    pub(crate) fn establish() -> io::Result<Self> {
        Ok(Self {
            #[cfg(unix)]
            group: unix_group::ProcessGroup::current()?,
            #[cfg(windows)]
            job: windows_job::CurrentProcessJob::attach()?,
        })
    }

    /// 只用于不可协作后端的 cancel/EOF；终止自身和所有后代，不声称自己已等待退出。
    pub(crate) fn terminate(&self) -> io::Result<()> {
        #[cfg(unix)]
        self.group.terminate()?;
        #[cfg(windows)]
        self.job.terminate()?;
        Ok(())
    }

    /// 原生工作已结束才允许正常 exit 0；Windows 不在关闭自身 Job 时重新杀死当前进程。
    pub(crate) fn completed(&self) -> io::Result<()> {
        #[cfg(windows)]
        self.job.completed()?;
        Ok(())
    }
}
