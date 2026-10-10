//! 进程生命周期测试共用的文件握手；标记仅协调时序，不能证明进程退出。

use std::{
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

#[cfg(windows)]
#[path = "windows_support.rs"]
mod windows;
#[cfg(windows)]
pub(crate) use windows::*;

#[path = "fixture.rs"]
mod fixture;
pub(crate) use fixture::fixture_command;

/// 使用原子发布的完整标记协调测试与夹具，提前退出时可取消等待。
pub(crate) fn wait_for_marker(path: &Path, stop: &AtomicBool) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        if stop.load(Ordering::Acquire) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "夹具观察已停止"));
        }
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("等待夹具标记超时：{}", path.display()),
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

/// 无后台观察线程的测试共用相同就绪期限，不复制标记轮询。
pub(crate) fn wait_file(path: &Path) {
    wait_for_marker(path, &AtomicBool::new(false)).unwrap();
}

/// 先写私有临时文件再 rename，观察者不会把半个 PID 或错误码当成完整结果。
pub(crate) fn publish(path: &Path, value: &str) -> io::Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, value)?;
    fs::rename(temporary, path)
}

/// 固定开发/CI 的 Node 绝对路径；语言与传输夹具不重复实现工具发现。
pub(crate) fn node_executable() -> PathBuf {
    static NODE: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
    NODE.get_or_init(|| {
        let path = match std::env::var_os("NARRANT_TEST_NODE") {
            Some(path) => PathBuf::from(path),
            None => {
                let output = std::process::Command::new("node")
                    .args(["--print", "process.execPath"])
                    .output()
                    .expect("生命周期测试需要 Node 或 NARRANT_TEST_NODE 绝对路径");
                assert!(output.status.success(), "无法从 PATH 解析 Node 可执行文件");
                PathBuf::from(String::from_utf8(output.stdout).unwrap().trim())
            }
        };
        assert!(
            path.is_absolute() && path.is_file(),
            "Node 路径无效：{}",
            path.display()
        );
        path.canonicalize().unwrap()
    })
    .clone()
}
