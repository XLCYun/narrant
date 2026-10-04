//! 平台进程树场景集中在所有者的 Interface 上，协议调用方只保留各自协议验收。

use super::{
    test_support::{fixture_command, publish, wait_file},
    ProcessTree,
};
use std::{
    fs,
    process::Stdio,
    time::{Duration, Instant},
};

/// 三个公开生命周期入口共享父存活/父已退出以及后代管道继承的场景矩阵。
#[derive(Clone, Copy, Debug)]
enum Cleanup {
    Finish,
    Reap,
    Drop,
}

/// 不依赖进程退出速度：父夹具必须等到观察句柄保存后才获准退出。
fn exercise(cleanup: Cleanup, parent_exits: bool, inherited_pipes: bool) {
    let directory = tempfile::tempdir().unwrap();
    let role = if parent_exits {
        "parent-exits"
    } else {
        "parent-lives"
    };
    let mut command = fixture_command(role, directory.path());
    if inherited_pipes {
        command
            .env("NARRANT_PROCESS_INHERIT_PIPES", "1")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
    }
    let mut process = ProcessTree::spawn(&mut command).unwrap();
    wait_file(&directory.path().join("ready"));
    let child_pid = fs::read_to_string(directory.path().join("child-pid"))
        .unwrap()
        .parse::<u32>()
        .unwrap();
    #[cfg(unix)]
    let root_pid = process.child.id();
    #[cfg(windows)]
    let observed = super::test_support::ObservedProcesses {
        root: super::test_support::ObservedProcess::child(&process.child),
        descendant: super::test_support::ObservedProcess::open(child_pid).unwrap(),
    };
    #[cfg(windows)]
    {
        use windows_sys::Win32::Foundation::WAIT_TIMEOUT;
        assert_eq!(observed.root.status(), WAIT_TIMEOUT);
        assert_eq!(observed.descendant.status(), WAIT_TIMEOUT);
        assert!(process.windows_owner_contains(&observed.root.0).unwrap());
        assert!(process
            .windows_owner_contains(&observed.descendant.0)
            .unwrap());
    }
    if parent_exits {
        publish(&directory.path().join("handles-ready"), "saved").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while process.status().unwrap().is_none() {
            assert!(Instant::now() < deadline, "父夹具未主动退出");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    let result = match cleanup {
        Cleanup::Finish => process.finish(Duration::from_millis(300)),
        Cleanup::Reap => process.reap(),
        Cleanup::Drop => {
            drop(process);
            Ok(())
        }
    };
    #[cfg(windows)]
    let completion = observed.completion();
    #[cfg(unix)]
    let completion = [root_pid, child_pid].map(|pid| {
        // SAFETY: PID 来自本次自有夹具，信号 0 只查询，不用于终止可能复用的对象。
        unsafe { libc::kill(pid as i32, 0) }
    });
    #[cfg(windows)]
    super::test_support::assert_completed(completion);
    #[cfg(unix)]
    assert_eq!(
        completion,
        [-1, -1],
        "{cleanup:?}: parent_exits={parent_exits}, inherited_pipes={inherited_pipes}"
    );
    result.unwrap();
}

/// 所有者集中运行相同平台场景，调用方不再各自维护一份通用矩阵。
fn matrix(cleanup: Cleanup) {
    for parent_exits in [false, true] {
        for inherited_pipes in [false, true] {
            exercise(cleanup, parent_exits, inherited_pipes);
        }
    }
}

/// 正常完成保留 300ms 根退出宽限，之后仍清空全部后代。
#[test]
fn finish_reclaims_parent_and_descendant_matrix() {
    matrix(Cleanup::Finish);
}

/// 协议已处理取消或超时后，立即回收不再增加协议等待。
#[test]
fn reap_reclaims_parent_and_descendant_matrix() {
    matrix(Cleanup::Reap);
}

/// 早退或异常展开使用同一回收屏障。
#[test]
fn drop_reclaims_parent_and_descendant_matrix() {
    matrix(Cleanup::Drop);
}

/// 即使 Job 在真正终止前失败，也必须先回收 direct Child；重试不得清除失败历史。
#[cfg(windows)]
#[test]
fn cleanup_error_still_waits_direct_child() {
    use super::test_support::ObservedProcess;
    use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
    let directory = tempfile::tempdir().unwrap();
    let mut process = ProcessTree::spawn(&mut fixture_command("leaf", directory.path())).unwrap();
    wait_file(&directory.path().join("leaf-ready"));
    let observed = ObservedProcess::child(&process.child);
    process.job.fail_next_cleanup_before_termination();
    let result = process.reap();
    let immediate = observed.status();
    assert_eq!(immediate, WAIT_OBJECT_0);
    assert!(result.is_err());
    assert!(!process.reaped);
    let retried = process.reap();
    let immediate = observed.status();
    assert_eq!(immediate, WAIT_OBJECT_0);
    assert!(retried.is_err());
}
