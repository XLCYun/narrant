//! 内嵌纯配置助手及有界子进程调用，不在运行时依赖仓库中的 JS 文件。
use super::{failure, Result};
use crate::lsp::CancellationCheck;
use std::{
    fs::File,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

pub(super) const HELPERS: &[(&str, &str)] = &[
    ("prepare.mjs", include_str!("runtime/prepare.mjs")),
    ("view.mjs", include_str!("runtime/view.mjs")),
    ("config.mjs", include_str!("runtime/config.mjs")),
];

/// 助手进程只运行可信代码；任何提前返回都先回收进程再释放临时文件。
struct HelperProcess(Child);
impl Drop for HelperProcess {
    /// 终止本助手专属进程组并等待直接子进程；助手自身不会创建子进程。
    fn drop(&mut self) {
        #[cfg(unix)]
        // SAFETY: process_group(0) 为此助手创建独立进程组。
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

/// 用磁盘输出避免管道填满死锁，并用固定截止时间响应取消。
pub(super) fn run(
    mut command: Command,
    timeout: Duration,
    cancelled: &CancellationCheck,
) -> Result<String> {
    if cancelled() {
        return Err(super::super::code_graph::AnalysisError::cancelled(
            "TypeScript 分析已取消",
        ));
    }
    let output = tempfile::tempdir().map_err(failure)?;
    let stdout = output.path().join("stdout");
    let stderr = output.path().join("stderr");
    command
        .stdin(Stdio::null())
        .stdout(File::create(&stdout).map_err(failure)?)
        .stderr(File::create(&stderr).map_err(failure)?);
    crate::child_process::configure_std_command(&mut command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut process = HelperProcess(command.spawn().map_err(|error| {
        failure(format!(
            "无法启动 TypeScript 工具：{error}；请运行 pnpm setup:typescript-analyzer"
        ))
    })?);
    let deadline = Instant::now() + timeout;
    loop {
        if cancelled() {
            return Err(super::super::code_graph::AnalysisError::cancelled(
                "TypeScript 分析已取消",
            ));
        }
        if let Some(status) = process.0.try_wait().map_err(failure)? {
            if !status.success() {
                let error = std::fs::read_to_string(&stderr).map_err(failure)?;
                return Err(failure(format!(
                    "TypeScript 工具失败 {status}：{}",
                    error.chars().take(8192).collect::<String>()
                )));
            }
            return std::fs::read_to_string(stdout).map_err(failure);
        }
        if Instant::now() >= deadline {
            return Err(failure("TypeScript 工具调用超时"));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
#[path = "tests/runtime.rs"]
mod tests;
