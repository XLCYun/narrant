use super::*;
use crate::child_process::test_support::{
    fixture_command, publish, retained_status, wait_file, ObservedProcess,
};
use std::{fs, path::Path};
use windows_sys::Win32::{
    Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{
        JobObjects::{
            IsProcessInJob, JobObjectBasicProcessIdList, JOBOBJECT_BASIC_PROCESS_ID_LIST,
        },
        Threading::{OpenProcess, PROCESS_SYNCHRONIZE},
    },
};

/// 小型夹具只读查询当前准确 Job 的成员，绝不补捕或修改生产账本。
fn current_members(job: &WindowsJob) -> io::Result<Vec<u32>> {
    const HEADER: usize =
        std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList) / size_of::<usize>();
    let mut buffer = [0usize; HEADER + 16];
    // SAFETY: usize 缓冲区满足对齐，包含完整头部与 16 个 PID 的空间。
    if unsafe {
        QueryInformationJobObject(
            job.raw(),
            JobObjectBasicProcessIdList,
            buffer.as_mut_ptr().cast(),
            std::mem::size_of_val(&buffer) as u32,
            ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: 固定头部完全落在上面的对齐缓冲区内。
    let info = unsafe { &*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
    let count = info.NumberOfProcessIdsInList as usize;
    if count > 16 || info.NumberOfAssignedProcesses != info.NumberOfProcessIdsInList {
        return Err(io::Error::other("小型 Job 夹具的成员列表不完整"));
    }
    buffer[HEADER..HEADER + count]
        .iter()
        .map(|pid| u32::try_from(*pid).map_err(io::Error::other))
        .collect()
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
    assert_eq!(retained_status(&observed), WAIT_TIMEOUT);
    drop(pending);
    job.terminate_and_wait().unwrap();
    assert_eq!(retained_status(&observed), WAIT_OBJECT_0);
    assert!(!directory.path().join("leaf-ready").exists());
}

/// 在带工作线程的 Node 根退出期间反复查询，覆盖 Native Helper 的实际退出模式。
#[test]
fn native_windows_member_snapshot_tolerates_root_exit() {
    use crate::child_process::test_support::node_executable;

    for iteration in 0..16 {
        let mut command = Command::new(node_executable());
        command.args([
            "--eval",
            "globalThis.heap = Buffer.alloc(128 * 1024 * 1024, 1); const { Worker } = require('node:worker_threads'); new Worker('', { eval: true });",
        ]);
        let (mut pending, job) = WindowsJob::spawn_suspended(&mut command).unwrap();
        let child = pending.0.as_mut().unwrap();
        let observed = child.as_handle().try_clone_to_owned().unwrap();
        resume_initial_thread(child.id()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        if iteration % 2 == 0 {
            // 覆盖 Host 收到终结后仍需强制回收的路径；另一半保留自然退出。
            std::thread::sleep(Duration::from_millis(100));
            job.terminate_and_wait()
                .unwrap_or_else(|error| panic!("iteration {iteration}: {error}"));
        }
        loop {
            job.tracker
                .lock()
                .unwrap()
                .supplement(job.raw(), deadline)
                .unwrap_or_else(|error| panic!("iteration {iteration}: {error}"));
            if child.try_wait().unwrap().is_some() {
                break;
            }
        }
        job.terminate_and_wait().unwrap();
        assert_eq!(retained_status(&observed), WAIT_OBJECT_0);
        drop(pending);
    }
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
    let immediate_parent = retained_status(&observed_parent);
    let immediate_child = retained_status(&observed_child);
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
    // stop 没有 supplement 路径，冻结的真实通知是登记第二个对象的唯一来源。
    assert_eq!(tracker.recorded(), 1, "冻结通知尚未登记真实成员");
    tracker
        .stop(Instant::now() + Duration::from_secs(2))
        .unwrap();
    let immediate = retained_status(child);
    assert_eq!(immediate, WAIT_OBJECT_0);
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
    // Child signaled 不保证 Job 列表同时更新；清理前先建立不能被 supplement 补捕的前提。
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let members = current_members(&job).unwrap();
        if !members.contains(&pid) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "已退出夹具仍在 Job 当前列表：{members:?}"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    {
        let tracker = job.tracker.lock().unwrap();
        assert_eq!(tracker.recorded(), 1, "只读前置等待不能补捕冻结成员");
        tracker.on_coverage_wait(move || {
            *gate.0.lock().unwrap() = true;
            gate.1.notify_all();
        });
    }
    let result = job.terminate_and_wait();
    // 清理返回后第一步观测准确对象，不让断言输出或后续 Drop 帮助完成。
    let immediate = retained_status(child);
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
    let immediate_root = retained_status(&root_handle);
    let immediate_member = retained_status(&member_handle);
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
    let immediate = retained_status(&observed);
    assert_eq!(immediate, WAIT_OBJECT_0);
    result.unwrap();
    assert_eq!(job.active_processes().unwrap(), 0);
    assert!(!directory.path().join("leaf-ready").exists());
}

/// 零进程上限必须以原始 1816 拒绝新成员，同时保留现有对象；直接验收生产门禁。
#[test]
fn admission_gate_rejects_new_process_with_exact_quota_error() {
    use windows_sys::Win32::Foundation::ERROR_NOT_ENOUGH_QUOTA;

    let directory = tempfile::tempdir().unwrap();
    let (mut root, job) =
        WindowsJob::spawn(&mut fixture_command("admission", directory.path())).unwrap();
    let observed_root = ObservedProcess::child(&root);
    wait_file(&directory.path().join("ready"));
    let descendant_pid = fs::read_to_string(directory.path().join("child-pid"))
        .unwrap()
        .parse::<u32>()
        .unwrap();
    let descendant = ObservedProcess::open(descendant_pid).unwrap();
    // 系统可能创建隐藏 conhost；先封闭入口，再核对全部真实对象，而非猜测成员数。
    job.seal_admission().unwrap();
    job.tracker
        .lock()
        .unwrap()
        .supplement(job.raw(), Instant::now() + Duration::from_secs(2))
        .unwrap();
    let info = job.accounting().unwrap();
    let retained = job.tracker.lock().unwrap().recorded();
    assert_eq!(
        retained, info.TotalProcesses as usize,
        "尝试新建之前必须完整覆盖历史成员"
    );
    let members = current_members(&job).unwrap();
    assert!(members.contains(&root.id()));
    assert!(members.contains(&descendant_pid));
    let mut observed = vec![(root.id(), observed_root), (descendant_pid, descendant)];
    for pid in members {
        if pid != root.id() && pid != descendant_pid {
            observed.push((pid, ObservedProcess::open(pid).unwrap()));
        }
    }
    for (pid, process) in &observed {
        let mut member = 0;
        // SAFETY: 根、叶和系统额外成员均持续持有准确 HANDLE，只查询本测试 Job 的归属。
        assert_ne!(
            unsafe { IsProcessInJob(process.0.as_raw_handle(), job.raw(), &mut member) },
            0
        );
        assert_eq!(member, 1, "观察对象 {pid} 必须属于准确 Job");
    }
    assert_eq!(observed[0].1.status(), WAIT_TIMEOUT);
    assert_eq!(observed[1].1.status(), WAIT_TIMEOUT);
    publish(&directory.path().join("attempt-spawn"), "attempt").unwrap();
    wait_file(&directory.path().join("attempt-result"));
    assert_eq!(
        fs::read_to_string(directory.path().join("attempt-result")).unwrap(),
        format!("rejected:Some({ERROR_NOT_ENOUGH_QUOTA})"),
        "其他创建错误不能冒充门禁生效",
    );
    assert_eq!(observed[0].1.status(), WAIT_TIMEOUT);
    assert_eq!(observed[1].1.status(), WAIT_TIMEOUT);
    let mut immediate = vec![WAIT_TIMEOUT; observed.len()];
    let result = job.terminate_and_wait();
    // 返回后先对全部原始 HANDLE 零等待采样，再做断言、日志或 fixture 兜底。
    for (state, (_, process)) in immediate.iter_mut().zip(&observed) {
        *state = process.status();
    }
    assert!(
        immediate.iter().all(|state| *state == WAIT_OBJECT_0),
        "准确成员未同步完成：{immediate:?}"
    );
    root.wait().unwrap();
    let info = job.accounting().unwrap();
    assert_eq!(info.ActiveProcesses, 0);
    if info.TotalProcesses as usize > job.tracker.lock().unwrap().recorded() {
        assert!(result.is_err(), "拒绝关联造成的累计缺口不能宣称清理成功");
        assert!(
            job.terminate_and_wait().is_err(),
            "累计缺口在重试时仍须保留"
        );
    } else {
        result.unwrap();
    }
}
