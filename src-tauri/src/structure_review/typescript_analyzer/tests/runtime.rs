use super::*;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// 失败状态带出 stderr，不会被当作一个空的助手响应。
#[test]
#[cfg(unix)]
fn helper_errors_are_explicit() {
    let mut command = Command::new("/bin/sh");
    command.args(["-c", "echo helper-failed >&2; exit 9"]);
    let error = run(
        command,
        Duration::from_secs(1),
        &(Arc::new(|| false) as CancellationCheck),
    )
    .unwrap_err();
    assert!(error.to_string().contains("helper-failed"));
}

/// 超时和取消在返回前都回收直接可信助手，无需等待整个分析任务析构。
#[test]
#[cfg(unix)]
fn helper_timeout_and_cancel_reclaim_process() {
    for cancel in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let pid_file = directory.path().join("pid");
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "echo $$ > \"$1\"; while :; do :; done", "helper"])
            .arg(&pid_file);
        let flag = Arc::new(AtomicBool::new(false));
        let observed = flag.clone();
        let check: CancellationCheck = Arc::new(move || observed.load(Ordering::SeqCst));
        let cancellation = cancel.then(|| {
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                flag.store(true, Ordering::SeqCst);
            })
        });
        let error = run(
            command,
            if cancel {
                Duration::from_secs(2)
            } else {
                Duration::from_millis(100)
            },
            &check,
        )
        .unwrap_err();
        if let Some(thread) = cancellation {
            thread.join().unwrap();
        }
        assert_eq!(error.is_cancelled(), cancel);
        let pid = std::fs::read_to_string(pid_file)
            .unwrap()
            .trim()
            .parse::<i32>()
            .unwrap();
        // SAFETY: 信号 0 只查询本测试助手的退出状态。
        assert_eq!(unsafe { libc::kill(pid, 0) }, -1);
    }
}
