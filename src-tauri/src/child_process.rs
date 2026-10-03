//! 桌面应用外部命令的共享窗口策略与 Windows 进程树所有权。

#[cfg(windows)]
#[path = "child_process/windows_job.rs"]
pub(crate) mod windows_job;

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
