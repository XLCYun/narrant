use super::*;
use crate::lsp::transport::Transport;
use std::{fs, path::Path, process::Stdio};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        JobObjects::IsProcessInJob,
        Threading::{
            OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
            PROCESS_TERMINATE,
        },
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

/// 原子发布夹具标记，观察者不会读到半写入的 PID 或错误提前放行。
fn publish(file: &Path, value: &str) {
    let temporary = file.with_extension("tmp");
    fs::write(&temporary, value).unwrap();
    fs::rename(temporary, file).unwrap();
}

/// 严格断言之外保留失败兜底，避免回归失败后自有叶进程留到后续测试。
struct FixtureProcess(OwnedHandle);

impl FixtureProcess {
    /// 根进程在 handles-ready 前不能退出，叶进程由本次根进程发布 PID。
    fn observe(pid: u32) -> Self {
        // SAFETY: PID 仅来自仍在握手等待中的本测试夹具，句柄只接管一次。
        Self(
            owned(unsafe { OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_TERMINATE, 0, pid) }).unwrap(),
        )
    }

    /// 查询准确进程对象，超时 0 不通过补等放宽清理返回合同。
    fn status(&self) -> u32 {
        // SAFETY: 当前守卫持有有效同步句柄。
        unsafe { WaitForSingleObject(self.0.as_raw_handle(), 0) }
    }
}

impl Drop for FixtureProcess {
    /// 必须先保存断言状态，随后才允许对仍存活的自有夹具执行兜底清理。
    fn drop(&mut self) {
        if self.status() == WAIT_TIMEOUT {
            // SAFETY: retained HANDLE 固定本次夹具身份，不按 PID 重新查找其他进程。
            unsafe {
                TerminateProcess(self.0.as_raw_handle(), 1);
                WaitForSingleObject(self.0.as_raw_handle(), 5_000);
            }
        }
    }
}

/// 独立测试程序角色：父进程等待准确句柄已保存再退出；叶进程等待 Job 回收。
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
        publish(&directory.join("child-pid"), &child.id().to_string());
        wait_for_file(&directory.join("leaf-ready"));
        wait_for_file(&directory.join("handles-ready"));
        std::process::exit(0);
    }
    publish(&directory.join("leaf-ready"), "ran");
    // 这里只是宿主异常时的最终兜底，不作为任何退出断言的等待证据。
    std::thread::sleep(Duration::from_secs(60));
}

/// 两种回收路径共用夹具，但分别由独立测试报告，避免首个红灯屏蔽另一分支。
fn parent_exited_transport() -> (tempfile::TempDir, Transport, FixtureProcess, FixtureProcess) {
    let directory = tempfile::tempdir().unwrap();
    let command = fixture_command("parent", directory.path());
    let mut transport = Transport::start(command).unwrap();
    let root = FixtureProcess::observe(transport._process.child.id());
    wait_for_file(&directory.path().join("child-pid"));
    let pid = fs::read_to_string(directory.path().join("child-pid"))
        .unwrap()
        .parse::<u32>()
        .unwrap();
    let child = FixtureProcess::observe(pid);
    wait_for_file(&directory.path().join("leaf-ready"));
    assert_eq!(root.status(), WAIT_TIMEOUT);
    assert_eq!(child.status(), WAIT_TIMEOUT);
    publish(&directory.path().join("handles-ready"), "saved");
    assert!(transport._process.child.wait().unwrap().success());
    assert_eq!(root.status(), WAIT_OBJECT_0);
    assert!(transport._process.job.active_processes().unwrap() > 0);
    (directory, transport, root, child)
}

/// finish 返回后的第一项观察就是同一个后代 HANDLE 的非阻塞完成状态。
#[test]
fn windows_job_finish_reclaims_child_after_parent_exit() {
    let (_directory, mut transport, root, child) = parent_exited_transport();
    let result = transport.finish();
    let child_status = child.status();
    let root_status = root.status();
    assert_eq!(child_status, WAIT_OBJECT_0);
    assert_eq!(root_status, WAIT_OBJECT_0);
    result.unwrap();
}

/// Drop 独立验证相同的同步完成合同，不让 finish 的失败提前跳过本分支。
#[test]
fn windows_job_drop_reclaims_child_after_parent_exit() {
    let (_directory, transport, root, child) = parent_exited_transport();
    drop(transport);
    let child_status = child.status();
    let root_status = root.status();
    assert_eq!(child_status, WAIT_OBJECT_0);
    assert_eq!(root_status, WAIT_OBJECT_0);
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
    let result = job.terminate_and_wait();
    // SAFETY: 只查询已回收的自有挂起进程。
    let immediate = unsafe { WaitForSingleObject(observed.as_raw_handle(), 0) };
    assert_eq!(immediate, WAIT_OBJECT_0);
    result.unwrap();
    assert_eq!(job.active_processes().unwrap(), 0);
    assert!(!directory.path().join("leaf-ready").exists());
}
