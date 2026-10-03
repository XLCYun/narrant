//! 直接调用生产 WindowsJob 的短诊断入口；不编译 Tauri，也不复制回收实现。

#[cfg(windows)]
#[path = "../../../src-tauri/src/lsp/windows_process.rs"]
mod windows_process;

#[cfg(windows)]
use std::{
    fs, io,
    os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
#[cfg(windows)]
use windows_sys::Win32::{
    Foundation::{HANDLE, WAIT_OBJECT_0},
    System::{
        JobObjects::{
            IsProcessInJob, JobObjectExtendedLimitInformation, QueryInformationJobObject,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_BREAKAWAY_OK,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
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

/// 有界等待由夹具发布的文件；超时直接返回错误并触发已有 Job 守卫。
#[cfg(windows)]
fn wait_file(path: &Path) -> io::Result<()> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !path.exists() {
        if Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "probe ready timeout",
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

/// 独立验证零上限能封闭新进程入口；不改变生产回收实现，也不宣称清理通过。
#[cfg(windows)]
fn verify_admission_gate(job: &windows_process::WindowsJob, directory: &Path) -> io::Result<()> {
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
    assert_eq!(limits.BasicLimitInformation.ActiveProcessLimit, 0);
    assert_ne!(flags & JOB_OBJECT_LIMIT_ACTIVE_PROCESS, 0);
    assert_ne!(flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, 0);
    assert_eq!(
        flags & (JOB_OBJECT_LIMIT_BREAKAWAY_OK | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK),
        0
    );
    fs::write(directory.join("attempt-spawn"), "attempt")?;
    wait_file(&directory.join("attempt-result"))?;
    let outcome = fs::read_to_string(directory.join("attempt-result"))?;
    println!("[windows-process-probe] admission_gate flags={flags:#x} active_limit=0 spawn_outcome={outcome}");
    assert!(
        outcome.starts_with("rejected:"),
        "sealed Job admitted a new process"
    );
    Ok(())
}

/// 一个场景同时验证 Job 继承、已有清理函数和立即退出信号。
#[cfg(windows)]
fn run_case(node: &Path, directory: &Path, mode: &str) -> io::Result<()> {
    fs::create_dir(directory)?;
    let system_root = std::env::var_os("SystemRoot")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::other("Windows probe requires SystemRoot"))?;
    let mut command = Command::new(node);
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
        .stderr(Stdio::piped());
    let (mut parent, job) = windows_process::WindowsJob::spawn(&mut command)?;
    wait_file(&directory.join("ready"))?;
    let pid: u32 = fs::read_to_string(directory.join("descendant"))?
        .parse()
        .map_err(io::Error::other)?;
    let descendant = observe(pid)?;
    let member = belongs(descendant.0.as_raw_handle(), job.raw())?;
    println!("[windows-process-probe] mode={mode} parent={} descendant={pid} parent_in_exact_job={:?} descendant_in_exact_job={member} active_before={}", parent.id(), belongs(parent.as_raw_handle(), job.raw()), job.active_processes()?);
    state("before", pid, descendant.0.as_raw_handle());
    if mode == "admission_gate" {
        verify_admission_gate(&job, directory)?;
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
    let started = Instant::now();
    let cleanup = job.terminate_and_wait();
    let signaled = state("after_cleanup", pid, descendant.0.as_raw_handle());
    let parent_signaled = state("parent_after_cleanup", parent.id(), parent.as_raw_handle());
    println!(
        "[windows-process-probe] mode={mode} cleanup={cleanup:?} elapsed_ms={} active_after={:?}",
        started.elapsed().as_millis(),
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
    let modes: &[&str] = if std::env::args().nth(1).as_deref() == Some("--admission-only") {
        &["admission_gate"]
    } else {
        &[
            "admission_gate",
            "parent_exited_open",
            "parent_exited_closed",
            "parent_alive",
        ]
    };
    for mode in modes {
        run_case(&node, &root.join(mode), mode)?;
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
