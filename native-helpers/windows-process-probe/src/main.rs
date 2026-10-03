//! 直接调用生产 WindowsJob 的短诊断入口；不编译 Tauri，也不复制回收实现。

#[cfg(windows)]
mod admission_worker;
#[cfg(windows)]
#[path = "../../../src-tauri/src/structure_review/native_analyzer/windows_process.rs"]
mod windows_process;

#[cfg(windows)]
use std::{
    fs, io,
    io::Read,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{ERROR_MORE_DATA, HANDLE, WAIT_OBJECT_0},
    System::{
        JobObjects::{
            IsProcessInJob, JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation,
            QueryInformationJobObject, SetInformationJobObject, JOBOBJECT_BASIC_PROCESS_ID_LIST,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
            JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
        },
        Threading::{
            OpenProcess, TerminateProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
        },
    },
};

/// 诊断失败也回收已确认来自自有夹具的叶进程，防止故障探针留下进程。
#[cfg(windows)]
struct ObservedProcess(OwnedHandle);

#[cfg(windows)]
impl Drop for ObservedProcess {
    /// 探针断言之前已记录立即状态；兜底终止不参与生产成功判据。
    fn drop(&mut self) {
        // SAFETY: 句柄在子进程仍存活时从本次夹具 PID 打开，之后一直持有。
        unsafe {
            if WaitForSingleObject(self.0.as_raw_handle(), 0) != WAIT_OBJECT_0 {
                TerminateProcess(self.0.as_raw_handle(), 1);
                WaitForSingleObject(self.0.as_raw_handle(), 2000);
            }
        }
    }
}

/// 在叶进程已发布就绪标记后固定真实进程对象，不靠回收后的 PID 重查证明退出。
#[cfg(windows)]
fn observe(pid: u32) -> io::Result<ObservedProcess> {
    open_process(
        pid,
        PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE | PROCESS_TERMINATE,
    )
    .map(ObservedProcess)
}

/// 只请求明确权限并接管句柄；重查 PID 时不附加终止行为。
#[cfg(windows)]
fn open_process(pid: u32, access: u32) -> io::Result<OwnedHandle> {
    // SAFETY: 只打开本次夹具的 PID；OwnedHandle 接管成功返回的非空句柄。
    let handle = unsafe { OpenProcess(access, 0, pid) };
    if handle.is_null() {
        return Err(io::Error::last_os_error());
    }
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// 查询精确 Job 归属，排除“属于某个 Job”而非 Host 自有 Job 的歧义。
#[cfg(windows)]
fn belongs(process: HANDLE, job: HANDLE) -> io::Result<bool> {
    let mut member = 0;
    // SAFETY: 两个句柄在调用期间均由外部守卫持有；输出指针有效。
    if unsafe { IsProcessInJob(process, job, &mut member) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(member != 0)
}

/// 输出同一被保留句柄和回收后按 PID 重开的状态，以区分真实存活与 PID 重用。
#[cfg(windows)]
fn state(label: &str, pid: u32, handle: HANDLE) -> u32 {
    // SAFETY: 保留的句柄在调用期间有效；超时 0 只观察，不等待或终止。
    let retained = unsafe { WaitForSingleObject(handle, 0) };
    let reopened = open_process(pid, PROCESS_SYNCHRONIZE)
        .map(|handle| {
            // SAFETY: 重开对象仅查询，绝不终止可能被重用的 PID。
            unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) }.to_string()
        })
        .unwrap_or_else(|error| format!("open_error:{:?}", error.raw_os_error()));
    println!("[windows-process-probe] {label} pid={pid} retained_wait={retained} reopened_wait={reopened}");
    retained
}

/// 只读准确 Job 的完整 PID 列表；有界扩容，既不按父 PID 扫描也不修改成员。
#[cfg(windows)]
fn job_members(job: &windows_process::NativeWindowsJob) -> io::Result<Vec<u32>> {
    const HEADER: usize = std::mem::offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList)
        / std::mem::size_of::<usize>();
    let mut buffer = vec![0usize; HEADER + 16];
    loop {
        // SAFETY: usize 数组满足该结构的对齐，缓冲区长度包含完整头部和 PID 空间。
        let success = unsafe {
            QueryInformationJobObject(
                job.raw(),
                JobObjectBasicProcessIdList,
                buffer.as_mut_ptr().cast(),
                (buffer.len() * std::mem::size_of::<usize>()) as u32,
                std::ptr::null_mut(),
            )
        };
        let error = io::Error::last_os_error();
        // SAFETY: 固定头部位于已分配且对齐的缓冲区内。
        let info = unsafe { &*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
        if success == 0 && error.raw_os_error() != Some(ERROR_MORE_DATA as i32) {
            return Err(error);
        }
        if success == 0 || info.NumberOfProcessIdsInList < info.NumberOfAssignedProcesses {
            let capacity = (info.NumberOfAssignedProcesses as usize).max(buffer.len() * 2);
            if capacity > 4096 {
                return Err(io::Error::other("probe Job member limit exceeded"));
            }
            buffer.resize(HEADER + capacity, 0);
            continue;
        }
        let count = info.NumberOfProcessIdsInList as usize;
        if count > buffer.len() - HEADER {
            return Err(io::Error::other("probe invalid Job member count"));
        }
        return buffer[HEADER..HEADER + count]
            .iter()
            .map(|pid| u32::try_from(*pid).map_err(io::Error::other))
            .collect();
    }
}

/// 固定原始失败判据之后采样退出窗口；诊断中的等待不能改变已记录的成功/失败。
#[cfg(windows)]
fn trace_membership_after_cleanup(
    job: &windows_process::NativeWindowsJob,
    pid: u32,
    handle: HANDLE,
) {
    let mut observations = Vec::new();
    for sample in 0..16 {
        // SAFETY: 同一个保留句柄贯穿采样；超时 0 只观察。
        let before = unsafe { WaitForSingleObject(handle, 0) };
        let members = job_members(job);
        let after = unsafe { WaitForSingleObject(handle, 0) };
        let member = belongs(handle, job.raw());
        let active = job.active_processes();
        observations.push((sample, before, members, after, member, active));
        if after == WAIT_OBJECT_0 {
            break;
        }
        // 仅诊断允许最多 16 次 1ms 采样；绝不拿这里的最终状态替代立即退出断言。
        unsafe {
            WaitForSingleObject(handle, 1);
        }
    }
    for (sample, before, members, after, member, active) in observations {
        println!("[windows-process-probe] completion_snapshot sample={sample} pid={pid} wait_before={before} members={members:?} wait_after={after} in_exact_job={member:?} active={active:?}");
    }
}

/// 有界等待由夹具发布的文件；超时直接返回错误并触发已有 Job 守卫。
#[cfg(windows)]
fn wait_file(path: &Path) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                format!("probe marker timeout: {}", path.display()),
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

/// 独立验证零上限能封闭新进程入口；不改变生产回收实现，也不宣称清理通过。
#[cfg(windows)]
fn verify_admission_gate(
    job: &windows_process::NativeWindowsJob,
    directory: &Path,
) -> io::Result<()> {
    seal_admission(job)?;
    fs::write(directory.join("attempt-spawn"), "attempt")?;
    println!(
        "[windows-process-probe] admission_gate attempt-spawn published; waiting attempt-result"
    );
    wait_file(&directory.join("attempt-result"))?;
    let outcome = fs::read_to_string(directory.join("attempt-result"))?;
    println!("[windows-process-probe] admission_gate spawn_outcome={outcome}");
    if !outcome.starts_with("rejected:") {
        return Err(io::Error::other(format!(
            "admission gate result requires diagnosis: {outcome}"
        )));
    }
    Ok(())
}

/// 只封闭当前隔离 probe 的新进程入口，供候选与拒绝诊断共用。
#[cfg(windows)]
fn seal_admission(job: &windows_process::NativeWindowsJob) -> io::Result<()> {
    // SAFETY: Windows 信息结构为 POD，读写大小和信息类完全匹配。
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    let bytes = std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
    // SAFETY: 只读取当前 probe 独占的 Job。
    if unsafe {
        QueryInformationJobObject(
            job.raw(),
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            bytes,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
    limits.BasicLimitInformation.ActiveProcessLimit = 0;
    // SAFETY: 仅修改本次隔离 probe 的 Job；保留现有限制，不允许后代 breakaway。
    if unsafe {
        SetInformationJobObject(
            job.raw(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            bytes,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    println!("[windows-process-probe] admission_gate SetInformationJobObject succeeded");
    // SAFETY: 从同一自有 Job 读回，证明零值及限制 flags 已被内核接受。
    if unsafe {
        QueryInformationJobObject(
            job.raw(),
            JobObjectExtendedLimitInformation,
            (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            bytes,
            std::ptr::null_mut(),
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let flags = limits.BasicLimitInformation.LimitFlags;
    println!("[windows-process-probe] admission_gate readback flags={flags:#x} active_limit={} active_now={:?}", limits.BasicLimitInformation.ActiveProcessLimit, job.active_processes());
    assert_eq!(limits.BasicLimitInformation.ActiveProcessLimit, 0);
    assert_ne!(flags & JOB_OBJECT_LIMIT_ACTIVE_PROCESS, 0);
    assert_ne!(flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, 0);
    assert_eq!(
        flags & (JOB_OBJECT_LIMIT_BREAKAWAY_OK | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK),
        0
    );
    Ok(())
}

/// 只读取本次夹具写出的有限诊断文件，不等可能被后代继承的管道 EOF。
#[cfg(windows)]
fn print_fixture_diagnostic(directory: &Path, name: &str) {
    let mut bytes = Vec::new();
    let result = fs::File::open(directory.join(name))
        .and_then(|file| file.take(16 * 1024).read_to_end(&mut bytes));
    println!(
        "[windows-process-probe] {name} read={result:?} text={:?}",
        String::from_utf8_lossy(&bytes)
    );
}

/// 一个场景同时验证 Job 继承、已有清理函数和立即退出信号。
#[cfg(windows)]
fn run_case(node: &Path, directory: &Path, mode: &str) -> io::Result<()> {
    fs::create_dir(directory)?;
    let system_root = std::env::var_os("SystemRoot")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::other("Windows probe requires SystemRoot"))?;
    let mut command = Command::new(node);
    let stderr = if mode == "admission_gate" {
        Stdio::from(fs::File::create(directory.join("fixture-stderr"))?)
    } else {
        Stdio::piped()
    };
    command
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("fixture.mjs"))
        .arg(directory)
        .arg(mode)
        .env_clear()
        .env("SystemRoot", system_root)
        .env("TMPDIR", directory)
        .env("TMP", directory)
        .env("TEMP", directory)
        .current_dir(directory)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr);
    let (mut parent, job) = windows_process::NativeWindowsJob::spawn(&mut command)?;
    wait_file(&directory.join("ready"))?;
    let pid: u32 = fs::read_to_string(directory.join("descendant"))?
        .parse()
        .map_err(io::Error::other)?;
    let descendant = observe(pid)?;
    let member = belongs(descendant.0.as_raw_handle(), job.raw())?;
    println!("[windows-process-probe] mode={mode} parent={} descendant={pid} parent_in_exact_job={:?} descendant_in_exact_job={member} active_before={}", parent.id(), belongs(parent.as_raw_handle(), job.raw()), job.active_processes()?);
    state("before", pid, descendant.0.as_raw_handle());
    if mode == "admission_gate" {
        if let Err(error) = verify_admission_gate(&job, directory) {
            println!("[windows-process-probe] admission_gate failed={error} parent_exit={:?} active_at_failure={:?}", parent.try_wait(), job.active_processes());
            state("gate_failure_parent", parent.id(), parent.as_raw_handle());
            state("gate_failure_descendant", pid, descendant.0.as_raw_handle());
            print_fixture_diagnostic(directory, "attempt-trace");
            print_fixture_diagnostic(directory, "fixture-stderr");
            return Err(error);
        }
        // SAFETY: 两个进程仍由当前守卫持有；仅验证封闭入口没有终止既有成员。
        assert_eq!(
            unsafe { WaitForSingleObject(parent.as_raw_handle(), 0) },
            windows_sys::Win32::Foundation::WAIT_TIMEOUT
        );
        assert_eq!(
            unsafe { WaitForSingleObject(descendant.0.as_raw_handle(), 0) },
            windows_sys::Win32::Foundation::WAIT_TIMEOUT
        );
        println!("[windows-process-probe] PASS: admission gate only; production cleanup correctness remains unverified");
        // 该隔离场景只证明 gate。兜底显式终止夹具，原生产红灯场景未被替换。
        job.terminate_and_wait()?;
        parent.wait()?;
        drop(descendant);
        drop(job);
        fs::remove_dir_all(directory)?;
        return Ok(());
    }
    if mode != "parent_alive" {
        let deadline = Instant::now() + Duration::from_secs(10);
        while parent.try_wait()?.is_none() {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "probe parent exit timeout",
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    println!(
        "[windows-process-probe] members_before_cleanup={:?}",
        job_members(&job)
    );
    let started = Instant::now();
    let cleanup = job.terminate_and_wait();
    // SAFETY: 清理返回后立刻固定准确句柄状态，后续诊断采样不能掩盖原失败。
    let signaled = unsafe { WaitForSingleObject(descendant.0.as_raw_handle(), 0) };
    let parent_signaled = unsafe { WaitForSingleObject(parent.as_raw_handle(), 0) };
    let elapsed = started.elapsed().as_millis();
    trace_membership_after_cleanup(&job, pid, descendant.0.as_raw_handle());
    println!("[windows-process-probe] immediate_after_cleanup descendant_wait={signaled} parent_wait={parent_signaled}");
    state(
        "after_diagnostic_samples",
        pid,
        descendant.0.as_raw_handle(),
    );
    println!(
        "[windows-process-probe] mode={mode} cleanup={cleanup:?} elapsed_ms={} active_after={:?}",
        elapsed,
        job.active_processes()
    );
    cleanup?;
    parent.wait()?;
    assert!(member, "descendant escaped exact Host Job");
    assert_eq!(
        signaled, WAIT_OBJECT_0,
        "cleanup returned with a live descendant: {mode}"
    );
    assert_eq!(
        parent_signaled, WAIT_OBJECT_0,
        "cleanup returned with a live parent: {mode}"
    );
    drop(descendant);
    drop(job);
    fs::remove_dir_all(directory)?;
    Ok(())
}

/// 三个短场景直接使用真实 WindowsJob，不加载 Tauri 或下载语言工具链。
#[cfg(windows)]
fn main() -> io::Result<()> {
    if let Some(result) = admission_worker::dispatch() {
        return result;
    }
    let node = PathBuf::from(std::env::var_os("NARRANT_TEST_NODE").ok_or_else(|| {
        io::Error::other("set NARRANT_TEST_NODE to the absolute Node 24 executable")
    })?);
    if !node.is_absolute() || !node.is_file() {
        return Err(io::Error::other(
            "NARRANT_TEST_NODE must be an existing absolute executable",
        ));
    }
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "narrant-process-probe-{}-{suffix}",
        std::process::id()
    ));
    fs::create_dir(&root)?;
    let modes: &[&str] = match std::env::args().nth(1).as_deref() {
        Some("--admission-only") => &["admission_gate"],
        _ => &[
            "parent_exited_open",
            "parent_exited_closed",
            "parent_alive",
            "admission_gate",
        ],
    };
    for mode in modes {
        if *mode == "admission_gate" {
            admission_worker::run(&root.join(mode))?;
        } else {
            run_case(&node, &root.join(mode), mode)?;
        }
    }
    fs::remove_dir(root)?;
    println!("[windows-process-probe] PASS: requested scenarios {modes:?}");
    Ok(())
}

/// 非 Windows 不伪造平台通过，显式报告该诊断入口的平台要求。
#[cfg(not(windows))]
fn main() {
    eprintln!("windows-process-probe requires Windows");
    std::process::exit(2);
}
