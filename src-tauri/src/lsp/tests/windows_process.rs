use super::*;
use crate::lsp::transport::Transport;
use std::{fs, path::Path, process::Stdio};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        JobObjects::IsProcessInJob,
        Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
    },
};

const FIXTURE: &str = "lsp::transport::windows_process::tests::windows_process_fixture";

/// 使用当前测试程序作子进程夹具，不依赖 PowerShell、Node 或第三方工具安装。
fn fixture_command(role: &str, directory: &Path) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", FIXTURE, "--ignored", "--nocapture"])
        .env("NARRANT_LSP_WINDOWS_TEST_ROLE", role)
        .env("NARRANT_LSP_WINDOWS_TEST_DIR", directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

/// 只等待当前夹具写出的同步标记，超时直接失败，不接触其他进程。
fn wait_for_file(file: &Path) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !file.exists() {
        assert!(
            Instant::now() < deadline,
            "夹具标记超时：{}",
            file.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// 独立测试程序角色：父进程创建长期存活子进程后立即退出；叶进程等待 Job 回收。
#[test]
#[ignore = "仅由 Windows 进程所有权测试作为子进程执行"]
fn windows_process_fixture() {
    let Ok(role) = std::env::var("NARRANT_LSP_WINDOWS_TEST_ROLE") else {
        return;
    };
    let directory =
        std::path::PathBuf::from(std::env::var_os("NARRANT_LSP_WINDOWS_TEST_DIR").unwrap());
    if role == "parent" {
        let child = fixture_command("leaf", &directory).spawn().unwrap();
        fs::write(directory.join("child-pid"), child.id().to_string()).unwrap();
        wait_for_file(&directory.join("leaf-ready"));
        std::process::exit(0);
    }
    fs::write(directory.join("leaf-ready"), "ran").unwrap();
    std::thread::sleep(Duration::from_secs(60));
}

/// 即使直接父进程已退出，共享传输的正常 finish 和 Drop 仍同步回收真实后代。
#[test]
fn windows_job_reclaims_child_after_parent_exit() {
    for normal_finish in [true, false] {
        let directory = tempfile::tempdir().unwrap();
        let command = fixture_command("parent", directory.path());
        let mut transport = Transport::start(command).unwrap();
        assert!(transport._process.child.wait().unwrap().success());
        let pid = fs::read_to_string(directory.path().join("child-pid"))
            .unwrap()
            .parse::<u32>()
            .unwrap();
        // SAFETY: PID 来自本次夹具，保留同步句柄避免 PID 重用影响退出断言。
        let child = owned(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, pid) }).unwrap();
        // SAFETY: 超时 0 只查询当前夹具子进程是否仍存活。
        assert_eq!(
            unsafe { WaitForSingleObject(child.as_raw_handle(), 0) },
            WAIT_TIMEOUT
        );
        assert!(transport._process.job.active_processes().unwrap() > 0);
        if normal_finish {
            transport.finish().unwrap();
        } else {
            drop(transport);
        }
        // SAFETY: 测试不额外轮询，返回即必须表明 Job 中的真实子进程已经结束。
        assert_eq!(
            unsafe { WaitForSingleObject(child.as_raw_handle(), 0) },
            WAIT_OBJECT_0
        );
    }
}

/// 服务器第一条指令之前已经加入 Job；启动中途丢弃也不能留下挂起进程。
#[test]
fn windows_job_owns_suspended_startup_before_execution() {
    let directory = tempfile::tempdir().unwrap();
    let mut command = fixture_command("leaf", directory.path());
    let (pending, job) = WindowsJob::spawn_suspended(&mut command).unwrap();
    let process = pending.0.as_ref().unwrap();
    let mut in_job = 0;
    // SAFETY: 两个句柄均由当前测试持有，只查询准确的 Job 归属。
    assert_ne!(
        unsafe { IsProcessInJob(process.as_raw_handle(), job.raw(), &mut in_job) },
        0
    );
    assert_eq!(in_job, 1);
    std::thread::sleep(Duration::from_millis(50));
    assert!(!directory.path().join("leaf-ready").exists());
    // SAFETY: 保存自有进程的同步句柄，在 pending 被丢弃后检查它是否确已停止。
    let observed = owned(unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, process.id()) }).unwrap();
    drop(pending);
    job.terminate_and_wait().unwrap();
    assert_eq!(job.active_processes().unwrap(), 0);
    // SAFETY: 只查询已回收的自有挂起进程。
    assert_eq!(
        unsafe { WaitForSingleObject(observed.as_raw_handle(), 0) },
        WAIT_OBJECT_0
    );
    assert!(!directory.path().join("leaf-ready").exists());
}
