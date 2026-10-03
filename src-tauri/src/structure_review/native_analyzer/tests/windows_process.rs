use super::*;
use std::{fs, path::Path, process::Stdio};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
};

const FIXTURE: &str = "structure_review::native_analyzer::process::process_tree::windows_process::tests::native_windows_fixture";

/// 用当前测试程序作原生夹具，避免 Node 自身 Job 干扰所有权检查。
fn fixture_command(role: &str, directory: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", FIXTURE, "--ignored", "--nocapture"])
        .env("NARRANT_NATIVE_JOB_ROLE", role)
        .env("NARRANT_NATIVE_JOB_DIR", directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

/// 文件仅作夹具同步信号，有界等待不会把实际退出断言替换为轮询。
fn wait_file(path: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        assert!(
            Instant::now() < deadline,
            "fixture marker timeout: {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 根/叶/短命三角色用于真实生命周期覆盖，不接触其他进程。
#[test]
#[ignore = "仅由 Native Job 所有权测试启动"]
fn native_windows_fixture() {
    let Ok(role) = std::env::var("NARRANT_NATIVE_JOB_ROLE") else {
        return;
    };
    let directory = std::path::PathBuf::from(std::env::var_os("NARRANT_NATIVE_JOB_DIR").unwrap());
    if role == "short" {
        return;
    }
    if role == "gap-parent" {
        assert!(fixture_command("short", &directory)
            .status()
            .unwrap()
            .success());
        let child = fixture_command("leaf", &directory).spawn().unwrap();
        fs::write(directory.join("child-pid"), child.id().to_string()).unwrap();
        wait_file(&directory.join("leaf-ready"));
        fs::write(directory.join("ready"), "ready").unwrap();
    } else {
        fs::write(directory.join("leaf-ready"), "ready").unwrap();
    }
    std::thread::sleep(Duration::from_secs(60));
}

/// Resume 之前根对象已登记；通知或重复登记不能把一个对象记作两个。
#[test]
fn native_windows_suspended_root_is_owned_and_deduplicated() {
    let directory = tempfile::tempdir().unwrap();
    let (pending, job) =
        NativeWindowsJob::spawn_suspended(&mut fixture_command("leaf", directory.path())).unwrap();
    let child = pending.0.as_ref().unwrap();
    let observed = child.as_handle().try_clone_to_owned().unwrap();
    {
        let tracker = job.tracker.lock().unwrap();
        tracker
            .primary(
                job.raw(),
                child.id(),
                child.as_handle().try_clone_to_owned().unwrap(),
            )
            .unwrap();
        assert_eq!(tracker.recorded(), 1);
    }
    assert!(!directory.path().join("leaf-ready").exists());
    // SAFETY: 真实根 HANDLE 从挂起阶段持续保留；0 超时只观察。
    assert_eq!(
        unsafe { WaitForSingleObject(observed.as_raw_handle(), 0) },
        WAIT_TIMEOUT
    );
    drop(pending);
    job.terminate_and_wait().unwrap();
    assert_eq!(
        unsafe { WaitForSingleObject(observed.as_raw_handle(), 0) },
        WAIT_OBJECT_0
    );
    assert!(!directory.path().join("leaf-ready").exists());
}

/// 确实错过短命对象时，累计缺口必须失败且重试不能擦掉失败历史。
#[test]
fn native_windows_missing_history_is_sticky() {
    let directory = tempfile::tempdir().unwrap();
    let (pending, job) =
        NativeWindowsJob::spawn_suspended(&mut fixture_command("gap-parent", directory.path()))
            .unwrap();
    job.tracker
        .lock()
        .unwrap()
        .stop(Instant::now() + Duration::from_secs(2))
        .unwrap();
    let parent = pending.0.as_ref().unwrap();
    let observed_parent = parent.as_handle().try_clone_to_owned().unwrap();
    resume_initial_thread(parent.id()).unwrap();
    wait_file(&directory.path().join("ready"));
    let pid = fs::read_to_string(directory.path().join("child-pid"))
        .unwrap()
        .parse()
        .unwrap();
    // SAFETY: 本次夹具仍活着时保留句柄，不在清理后依赖裸 PID 身份。
    let observed_child = owned(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) }).unwrap();
    let error = job.terminate_and_wait().unwrap_err();
    let immediate_parent = unsafe { WaitForSingleObject(observed_parent.as_raw_handle(), 0) };
    let immediate_child = unsafe { WaitForSingleObject(observed_child.as_raw_handle(), 0) };
    assert!(error.to_string().contains("完成覆盖不足"), "{error}");
    assert_eq!(immediate_parent, WAIT_OBJECT_0);
    assert_eq!(immediate_child, WAIT_OBJECT_0);
    assert!(job.terminate_and_wait().is_err());
    drop(pending);
}

/// Job 错误不能跳过已持有 direct Child 的回收，也不能把树标记为已确认完成。
#[test]
fn native_windows_tree_error_still_waits_direct_child() {
    let directory = tempfile::tempdir().unwrap();
    let mut tree =
        super::super::ProcessTree::spawn(&mut fixture_command("leaf", directory.path())).unwrap();
    wait_file(&directory.path().join("leaf-ready"));
    let observed = tree.child.as_handle().try_clone_to_owned().unwrap();
    tree.job.fail_next_cleanup_before_termination();
    assert!(tree.reap().is_err());
    let immediate = unsafe { WaitForSingleObject(observed.as_raw_handle(), 0) };
    assert_eq!(immediate, WAIT_OBJECT_0);
    assert!(!tree.reaped);
    assert!(tree.reap().is_err());
}
