use super::*;
use std::{fs, path::Path, process::Stdio};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        JobObjects::IsProcessInJob,
        Threading::{OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE},
    },
};

const FIXTURE: &str = concat!(module_path!(), "::native_windows_fixture");

/// 用当前测试程序作原生夹具，避免 Node 自身 Job 干扰所有权检查。
pub(crate) fn fixture_command(role: &str, directory: &Path) -> Command {
    // libtest 的完整名称不带 crate 前缀；原应用和轻量 harness 可共用同一夹具。
    let (_, fixture) = FIXTURE.split_once("::").expect("夹具模块包含 crate 前缀");
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", fixture, "--ignored", "--nocapture"])
        .env("NARRANT_NATIVE_JOB_ROLE", role)
        .env("NARRANT_NATIVE_JOB_DIR", directory)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}

/// 文件仅作夹具同步信号，有界等待不会把实际退出断言替换为轮询。
pub(crate) fn wait_file(path: &Path) {
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
        WindowsJob::spawn_suspended(&mut fixture_command("leaf", directory.path())).unwrap();
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
        WindowsJob::spawn_suspended(&mut fixture_command("gap-parent", directory.path())).unwrap();
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

/// 从测试控制进程创建第二个挂起对象，避免夹具自行创建额外进程影响累计断言。
fn suspended_test_member(directory: &Path) -> SuspendedChild {
    let mut command = fixture_command("short", directory);
    command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
    SuspendedChild(Some(command.spawn().unwrap()))
}

/// 在监听同步点安装之后关联第二个准确对象，确保 NEW_PROCESS 必定经过该同步点。
fn assign_test_member(job: &WindowsJob, pending: &SuspendedChild) {
    let child = pending.0.as_ref().unwrap();
    // SAFETY: 此对象由测试刚创建且仍挂起，只把它加入本测试的私有 Job。
    assert_ne!(
        unsafe { AssignProcessToJobObject(job.raw(), child.as_raw_handle()) },
        0
    );
}

/// 放行守卫保证断言失败时也释放监听线程；同步基于条件变量而不是退出速度。
struct CaptureRelease(std::sync::Arc<(Mutex<bool>, std::sync::Condvar)>);

impl CaptureRelease {
    /// 唤醒已经停在 dequeue 之后、尚未捕获 HANDLE 的监听线程。
    fn release(&self) {
        let (ready, changed) = &*self.0;
        *ready.lock().unwrap() = true;
        changed.notify_all();
    }
}

impl Drop for CaptureRelease {
    /// 错误展开不得把测试监听线程永久留在同步闸门中。
    fn drop(&mut self) {
        self.release();
    }
}

/// 已 dequeue 的短命对象在 stop 发布后仍必须捕获，不能依赖 PID 列表事后补救。
#[test]
fn native_windows_stop_preserves_dequeued_member() {
    use std::sync::{atomic::Ordering, mpsc, Arc};

    let directory = tempfile::tempdir().unwrap();
    let (_root, job) =
        WindowsJob::spawn_suspended(&mut fixture_command("leaf", directory.path())).unwrap();
    let mut member = suspended_test_member(directory.path());
    let pid = member.0.as_ref().unwrap().id();
    let (arrived, arrival) = mpsc::channel();
    job.tracker
        .lock()
        .unwrap()
        .on_capture(Arc::new(move |observed, stop| {
            if observed != pid {
                return;
            }
            arrived.send(()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(5);
            // 等待具体 stop 信号建立先后关系；期限仅防止坏实现导致测试永久挂起。
            while !stop.load(Ordering::Acquire) {
                assert!(Instant::now() < deadline, "stop 未发布到已取出通知的监听器");
                std::thread::yield_now();
            }
        }));
    assign_test_member(&job, &member);
    arrival.recv_timeout(Duration::from_secs(5)).unwrap();
    // 保留 Child 内的准确 HANDLE，使对象退出后仍可被已排队通知捕获。
    let child = member.0.as_mut().unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    let mut tracker = job.tracker.lock().unwrap();
    tracker
        .supplement(job.raw(), Instant::now() + Duration::from_secs(2))
        .unwrap();
    assert_eq!(tracker.recorded(), 1, "退出对象应已离开当前 Job PID 列表");
    tracker
        .stop(Instant::now() + Duration::from_secs(2))
        .unwrap();
    assert_eq!(tracker.recorded(), 2, "stop 不得丢弃已 dequeue 的真实成员");
}

/// cleanup 等累计覆盖时仍保留监听，按明确同步点释放最后一个历史成员通知。
#[test]
fn native_windows_cleanup_waits_for_queued_history() {
    use std::sync::{mpsc, Arc, Condvar};

    let directory = tempfile::tempdir().unwrap();
    let (_root, job) =
        WindowsJob::spawn_suspended(&mut fixture_command("leaf", directory.path())).unwrap();
    let mut member = suspended_test_member(directory.path());
    let pid = member.0.as_ref().unwrap().id();
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let release = CaptureRelease(Arc::clone(&gate));
    let (arrived, arrival) = mpsc::channel();
    let waiting = Arc::clone(&gate);
    job.tracker
        .lock()
        .unwrap()
        .on_capture(Arc::new(move |observed, _| {
            if observed != pid {
                return;
            }
            arrived.send(()).unwrap();
            let (ready, timeout) = waiting
                .1
                .wait_timeout_while(waiting.0.lock().unwrap(), Duration::from_secs(5), |ready| {
                    !*ready
                })
                .unwrap();
            assert!(
                *ready && !timeout.timed_out(),
                "累计覆盖阶段未放行真实成员通知"
            );
        }));
    assign_test_member(&job, &member);
    arrival.recv_timeout(Duration::from_secs(5)).unwrap();
    let child = member.0.as_mut().unwrap();
    child.kill().unwrap();
    child.wait().unwrap();
    {
        let tracker = job.tracker.lock().unwrap();
        tracker
            .supplement(job.raw(), Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert_eq!(tracker.recorded(), 1);
        tracker.on_coverage_wait(move || {
            *gate.0.lock().unwrap() = true;
            gate.1.notify_all();
        });
    }
    let result = job.terminate_and_wait();
    // 清理返回后第一步观测准确对象，不让断言输出或后续 Drop 帮助完成。
    let immediate = unsafe { WaitForSingleObject(child.as_raw_handle(), 0) };
    assert_eq!(immediate, WAIT_OBJECT_0);
    result.unwrap();
    assert_eq!(job.tracker.lock().unwrap().recorded(), 2);
    drop(release);
}

/// stop 报错之后仍进入真实 HANDLE 等待；断言在 Job 未 Drop 时完成，避免析构掩盖早退。
#[test]
fn native_windows_stop_error_still_waits_recorded_members() {
    let directory = tempfile::tempdir().unwrap();
    let (root, job) =
        WindowsJob::spawn_suspended(&mut fixture_command("leaf", directory.path())).unwrap();
    let member = suspended_test_member(directory.path());
    assign_test_member(&job, &member);
    let root_handle = root
        .0
        .as_ref()
        .unwrap()
        .as_handle()
        .try_clone_to_owned()
        .unwrap();
    let member_handle = member
        .0
        .as_ref()
        .unwrap()
        .as_handle()
        .try_clone_to_owned()
        .unwrap();
    {
        let tracker = job.tracker.lock().unwrap();
        tracker
            .supplement(job.raw(), Instant::now() + Duration::from_secs(2))
            .unwrap();
        assert_eq!(tracker.recorded(), 2);
        assert_eq!(tracker.waits(), 0);
        tracker.fail_next_stop();
    }
    let error = job.terminate_and_wait().unwrap_err();
    // SAFETY: 两个独立观察句柄从挂起阶段持续持有，0 超时不会补等。
    let immediate_root = unsafe { WaitForSingleObject(root_handle.as_raw_handle(), 0) };
    let immediate_member = unsafe { WaitForSingleObject(member_handle.as_raw_handle(), 0) };
    assert_eq!(immediate_root, WAIT_OBJECT_0);
    assert_eq!(immediate_member, WAIT_OBJECT_0);
    assert!(error
        .to_string()
        .contains("fixture failure after listener stop"));
    assert_eq!(
        job.tracker.lock().unwrap().waits(),
        1,
        "错误分支也必须执行等待"
    );
    assert!(job
        .terminate_and_wait()
        .unwrap_err()
        .to_string()
        .contains("fixture failure after listener stop"));
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
