//! 固定两个准确进程对象，仅改变嵌套 Job 关联，诊断累计计数与 IOCP 通知的对应关系。

use std::{
    ffi::OsStr,
    fs, io,
    mem::{offset_of, size_of},
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::{Path, PathBuf},
    ptr,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use windows_sys::Win32::{
    Foundation::{
        DuplicateHandle, DUPLICATE_SAME_ACCESS, ERROR_MORE_DATA, HANDLE, INVALID_HANDLE_VALUE,
        WAIT_FAILED, WAIT_OBJECT_0, WAIT_TIMEOUT,
    },
    System::{
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW,
            JobObjectAssociateCompletionPortInformation, JobObjectBasicAccountingInformation,
            JobObjectBasicProcessIdList, JobObjectExtendedLimitInformation,
            QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
            JOBOBJECT_ASSOCIATE_COMPLETION_PORT, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
            JOBOBJECT_BASIC_PROCESS_ID_LIST, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
        SystemServices::{JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO, JOB_OBJECT_MSG_NEW_PROCESS},
        Threading::{
            CreateProcessW, GetCurrentProcess, GetProcessId, ResumeThread, TerminateProcess,
            WaitForSingleObject, CREATE_NO_WINDOW, CREATE_SUSPENDED, PROCESS_INFORMATION,
            STARTUPINFOW,
        },
        IO::{CreateIoCompletionPort, GetQueuedCompletionStatus},
    },
};

/// 仅接管成功返回的原生句柄，避免失败时构造无效 OwnedHandle。
fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: 调用方仅传递新建或 DuplicateHandle 返回的唯一所有权。
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// 独占准确进程对象；错误展开时仍结束并等待自己的夹具，不按 PID 重查。
struct ProcessHandle {
    handle: OwnedHandle,
    pid: u32,
}

impl ProcessHandle {
    /// 真实 HANDLE 的零超时观测与有界等待共用同一接口。
    fn wait(&self, millis: u32) -> io::Result<u32> {
        // SAFETY: 句柄由当前守卫持有，等待不改变对象身份。
        let result = unsafe { WaitForSingleObject(self.handle.as_raw_handle(), millis) };
        if result == WAIT_FAILED {
            Err(io::Error::last_os_error())
        } else {
            Ok(result)
        }
    }

    /// 显式清场只证明诊断对象已退出，不替代任何产品 cleanup 返回屏障。
    fn terminate_and_wait(&self) -> io::Result<()> {
        if self.wait(0)? == WAIT_OBJECT_0 {
            return Ok(());
        }
        // SAFETY: 只结束当前守卫从 CreateProcess/DuplicateHandle 持有的对象。
        let terminated = unsafe { TerminateProcess(self.handle.as_raw_handle(), 1) };
        let failure = (terminated == 0).then(io::Error::last_os_error);
        if self.wait(5_000)? != WAIT_OBJECT_0 {
            return Err(failure.unwrap_or_else(|| {
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "nested probe process cleanup timeout",
                )
            }));
        }
        Ok(())
    }
}

impl Drop for ProcessHandle {
    /// 主动失败后的兜底也等待准确对象；诊断日志不把失败隐藏为成功。
    fn drop(&mut self) {
        if let Err(error) = self.terminate_and_wait() {
            eprintln!(
                "[windows-nested-accounting] disposal pid={} error={error}",
                self.pid
            );
        }
    }
}

/// 原始 CreateProcessW 返回的进程和主线程，恢复之前已可建立准确 Job 所有权。
struct SuspendedProcess {
    process: ProcessHandle,
    thread: OwnedHandle,
}

impl SuspendedProcess {
    /// 只执行当前诊断程序，无 Node、shell、可继承句柄或额外控制台。
    fn spawn(role: &str, directory: &Path) -> io::Result<Self> {
        let executable = std::env::current_exe()?;
        let application = executable
            .as_os_str()
            .encode_wide()
            .chain([0])
            .collect::<Vec<_>>();
        let mut command = Vec::new();
        for argument in [
            executable.as_os_str(),
            OsStr::new(role),
            directory.as_os_str(),
        ] {
            if !command.is_empty() {
                command.push(b' ' as u16);
            }
            quoted_argument(&mut command, argument);
        }
        command.push(0);
        // SAFETY: 两个 Win32 POD 结构按完整大小初始化；字符串均保持有效且 NUL 结尾。
        let mut startup: STARTUPINFOW = unsafe { std::mem::zeroed() };
        startup.cb = size_of::<STARTUPINFOW>() as u32;
        let mut information: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe {
            CreateProcessW(
                application.as_ptr(),
                command.as_mut_ptr(),
                ptr::null(),
                ptr::null(),
                0,
                CREATE_SUSPENDED | CREATE_NO_WINDOW,
                ptr::null(),
                ptr::null(),
                &startup,
                &mut information,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let process = ProcessHandle {
            handle: owned(information.hProcess)?,
            pid: information.dwProcessId,
        };
        Ok(Self {
            process,
            thread: owned(information.hThread)?,
        })
    }

    /// 根进程只恢复一次；叶进程始终不恢复，杜绝它自行创建隐藏后代。
    fn resume(&self) -> io::Result<()> {
        // SAFETY: 主线程由 CREATE_SUSPENDED 创建且仍由当前对象持有。
        match unsafe { ResumeThread(self.thread.as_raw_handle()) } {
            1 => Ok(()),
            u32::MAX => Err(io::Error::last_os_error()),
            _ => Err(io::Error::other("nested probe unexpected suspend count")),
        }
    }
}

/// 按 Windows argv 规则转义 UTF-16 参数，保留空格、反斜杠与非 Unicode 路径。
fn quoted_argument(output: &mut Vec<u16>, argument: &OsStr) {
    output.push(b'"' as u16);
    let mut slashes = 0;
    for unit in argument.encode_wide() {
        if unit == b'\\' as u16 {
            slashes += 1;
            continue;
        }
        let escaped = if unit == b'"' as u16 {
            slashes * 2 + 1
        } else {
            slashes
        };
        output.extend(std::iter::repeat(b'\\' as u16).take(escaped));
        output.push(unit);
        slashes = 0;
    }
    output.extend(std::iter::repeat(b'\\' as u16).take(slashes * 2));
    output.push(b'"' as u16);
}

/// 每个 Job 在首个关联前绑定自己的私有完成端口，消息只作为阶段诊断。
struct ProbeJob {
    handle: OwnedHandle,
    port: OwnedHandle,
    key: usize,
}

impl ProbeJob {
    /// 仅设置关闭时终止，避免 active/time/memory 限制造成本实验以外的计数变化。
    fn new(key: usize) -> io::Result<Self> {
        // SAFETY: 私有未命名对象不继承，均由此守卫接管。
        let handle = owned(unsafe { CreateJobObjectW(ptr::null(), ptr::null()) })?;
        let port =
            owned(unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, ptr::null_mut(), 0, 1) })?;
        let job = Self { handle, port, key };
        let association = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
            CompletionKey: key as *mut _,
            CompletionPort: job.port.as_raw_handle(),
        };
        // SAFETY: 结构和信息类别匹配，Job 尚未关联任何进程。
        if unsafe {
            SetInformationJobObject(
                job.raw(),
                JobObjectAssociateCompletionPortInformation,
                (&association as *const JOBOBJECT_ASSOCIATE_COMPLETION_PORT).cast(),
                size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: POD 清零后只设置非继承 Job 的故障清场策略。
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                job.raw(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    /// 借用本次准确 Job，查询不使用 NULL 所代表的调用者 immediate Job。
    fn raw(&self) -> HANDLE {
        self.handle.as_raw_handle()
    }

    /// 立即保存每次关联返回；成功时不把陈旧 GetLastError 当成此次结果。
    fn assign(&self, process: &ProcessHandle) -> Assignment {
        // SAFETY: Job 和进程句柄均在当前作用域持续持有。
        let success =
            unsafe { AssignProcessToJobObject(self.raw(), process.handle.as_raw_handle()) } != 0;
        Assignment {
            success,
            raw_error: (!success)
                .then(|| io::Error::last_os_error().raw_os_error())
                .flatten(),
        }
    }

    /// 完整保存累计、活跃和限制终止数量，并读取准确 Job 的当前 PID 集合。
    fn snapshot(&self) -> io::Result<JobSnapshot> {
        // SAFETY: POD 缓冲区类别与宽度匹配当前 Job。
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        if unsafe {
            QueryInformationJobObject(
                self.raw(),
                JobObjectBasicAccountingInformation,
                (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(JobSnapshot {
            total: info.TotalProcesses,
            active: info.ActiveProcesses,
            terminated_by_limit: info.TotalTerminatedProcesses,
            pids: self.pids()?,
        })
    }

    /// 有界读取当前成员列表；原进程 HANDLE 仍单独保留，列表不充当完成证明。
    fn pids(&self) -> io::Result<Vec<u32>> {
        const HEADER: usize =
            offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList) / size_of::<usize>();
        let mut buffer = vec![0usize; HEADER + 8];
        loop {
            // SAFETY: usize 缓冲区满足 Win32 结构对齐，并为可变 PID 数组预留空间。
            let success = unsafe {
                QueryInformationJobObject(
                    self.raw(),
                    JobObjectBasicProcessIdList,
                    buffer.as_mut_ptr().cast(),
                    (buffer.len() * size_of::<usize>()) as u32,
                    ptr::null_mut(),
                )
            };
            let error = (success == 0).then(io::Error::last_os_error);
            // SAFETY: 输出结构的固定头位于正确对齐的有效缓冲区内。
            let info = unsafe { &*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
            if let Some(error) = error {
                if error.raw_os_error() != Some(ERROR_MORE_DATA as i32) {
                    return Err(error);
                }
            }
            if success == 0 || info.NumberOfProcessIdsInList < info.NumberOfAssignedProcesses {
                let capacity = (info.NumberOfAssignedProcesses as usize).max(buffer.len() * 2);
                if capacity > 1024 {
                    return Err(io::Error::other(
                        "nested probe Job PID list exceeds diagnostic bound",
                    ));
                }
                buffer.resize(HEADER + capacity, 0);
                continue;
            }
            let count = info.NumberOfProcessIdsInList as usize;
            if count > buffer.len() - HEADER {
                return Err(io::Error::other("nested probe invalid PID list length"));
            }
            let mut pids = buffer[HEADER..HEADER + count]
                .iter()
                .map(|pid| u32::try_from(*pid).map_err(io::Error::other))
                .collect::<io::Result<Vec<_>>>()?;
            pids.sort_unstable();
            return Ok(pids);
        }
    }

    /// 最多50ms/256条读取当前可见消息；缺消息或重复消息均不作为进程完成判据。
    fn drain(
        &self,
        mode: &str,
        phase: &str,
        label: &str,
        outer: &ProbeJob,
        root: Option<&ProcessHandle>,
        child: Option<&ProcessHandle>,
    ) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_millis(50);
        let mut count = 0;
        let mut new_processes = Vec::new();
        let mut empty = false;
        while count < 256 && Instant::now() < deadline {
            let mut code = 0;
            let mut key = 0;
            let mut value = ptr::null_mut();
            // SAFETY: 端口独占，value只是通知负载，绝不把它当OVERLAPPED解引用。
            let success = unsafe {
                GetQueuedCompletionStatus(
                    self.port.as_raw_handle(),
                    &mut code,
                    &mut key,
                    &mut value,
                    5,
                )
            };
            if success == 0 {
                let error = io::Error::last_os_error();
                if error.raw_os_error() == Some(WAIT_TIMEOUT as i32) {
                    empty = true;
                    break;
                }
                return Err(error);
            }
            if key != self.key {
                return Err(io::Error::other("nested probe unexpected IOCP key"));
            }
            if code == JOB_OBJECT_MSG_ACTIVE_PROCESS_ZERO {
                let root_wait = root.map(|process| process.wait(0)).transpose()?;
                let child_wait = child.map(|process| process.wait(0)).transpose()?;
                let outer_now = outer.snapshot()?;
                println!("[windows-nested-accounting] mode={mode} phase={phase} zero_received_on={label} key={key} immediate_root_wait={root_wait:?} immediate_child_wait={child_wait:?} outer_active_after_wait_samples={} outer_total={} outer_pids={:?}; notification_source_job_not_identified=true", outer_now.active, outer_now.total, outer_now.pids);
            }
            count += 1;
            let is_new = code == JOB_OBJECT_MSG_NEW_PROCESS;
            if is_new {
                new_processes.push(value as usize);
            }
            println!("[windows-nested-accounting] mode={mode} phase={phase} job={label} event_code={code} key={key} pid_or_value={} new_process={is_new}", value as usize);
        }
        println!("[windows-nested-accounting] mode={mode} phase={phase} job={label} sampled_events={count} new_process_pids={new_processes:?} observed_queue_empty={empty}; notifications_are_not_completion_evidence=true");
        Ok(())
    }

    /// 显式结束诊断Job后，调用方仍须等待每个已持有的真实进程对象。
    fn terminate(&self) -> io::Result<()> {
        // SAFETY: 只终止本实验准确且非继承的 Job。
        if unsafe { TerminateJobObject(self.raw(), 1) } == 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

impl Drop for ProbeJob {
    /// 异常退出仍终止本Job，句柄关闭时KILL_ON_JOB_CLOSE再提供系统兜底。
    fn drop(&mut self) {
        if let Err(error) = self.terminate() {
            eprintln!(
                "[windows-nested-accounting] job_disposal key={} error={error}",
                self.key
            );
        }
    }
}

/// 每次关联的结果独立记录，重复加入同一Job的失败也保留原始错误码。
#[derive(Debug)]
struct Assignment {
    success: bool,
    raw_error: Option<i32>,
}

impl Assignment {
    /// 建立初始/嵌套层级必须成功；重复同Job的结果则只记录不预设。
    fn require_success(&self) -> io::Result<()> {
        if self.success {
            Ok(())
        } else {
            Err(self
                .raw_error
                .map(io::Error::from_raw_os_error)
                .unwrap_or_else(|| {
                    io::Error::other("nested probe assignment failed without OS code")
                }))
        }
    }
}

/// 这些数值来自同一个明确Job，不推定累计计数已按唯一进程去重。
#[derive(Debug)]
struct JobSnapshot {
    total: u32,
    active: u32,
    terminated_by_limit: u32,
    pids: Vec<u32>,
}

/// 根夹具在失败路径先收到释放信号，以便它自行回收尚未交给controller的叶对象。
struct RootFixture {
    spawned: SuspendedProcess,
    directory: PathBuf,
}

impl Drop for RootFixture {
    /// 优先让根执行叶守卫析构；失败时嵌套字段守卫与外层Job仍负责兜底。
    fn drop(&mut self) {
        let _ = publish(&self.directory.join("release"), "release");
        let _ = self.spawned.process.wait(5_000);
    }
}

/// 临时目录最后释放，确保根进程的错误/结束同步不因路径先删除而失效。
struct ProbeDirectory(PathBuf);

impl Drop for ProbeDirectory {
    /// 删除的路径仅由当前探针创建，不操作任何产品目录。
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// 原子发布完整描述符，controller不会读到半个PID或半个句柄数值。
fn publish(path: &Path, value: &str) -> io::Result<()> {
    let temporary = path.with_extension("tmp");
    fs::write(&temporary, value)?;
    fs::rename(temporary, path)
}

/// 根仅创建一个挂起叶对象并保持原句柄，直到controller已完成所有计数观察。
fn root_worker(directory: &Path) -> io::Result<()> {
    let child = SuspendedProcess::spawn("--nested-accounting-leaf", directory)?;
    publish(
        &directory.join("child"),
        &format!(
            "{} {}",
            child.process.pid,
            child.process.handle.as_raw_handle() as usize
        ),
    )?;
    super::wait_file(&directory.join("release"))?;
    child.process.terminate_and_wait()
}

/// 在原句柄仍由根持有时跨进程复制同一对象，避免按退出后的PID重查身份。
fn observe_child(root: &RootFixture) -> io::Result<ProcessHandle> {
    let deadline = Instant::now() + Duration::from_secs(10);
    let marker = root.directory.join("child");
    while !marker.exists() {
        if let Ok(error) = fs::read_to_string(root.directory.join("root-error")) {
            return Err(io::Error::other(format!("nested root failed: {error}")));
        }
        if root.spawned.process.wait(0)? != WAIT_TIMEOUT || Instant::now() >= deadline {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "nested root exited or failed to publish child",
            ));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let descriptor = fs::read_to_string(marker)?;
    let mut fields = descriptor.split_whitespace();
    let pid = fields
        .next()
        .ok_or_else(|| io::Error::other("missing child pid"))?
        .parse::<u32>()
        .map_err(io::Error::other)?;
    let source = fields
        .next()
        .ok_or_else(|| io::Error::other("missing child handle"))?
        .parse::<usize>()
        .map_err(io::Error::other)?;
    if fields.next().is_some() || source == 0 || pid == root.spawned.process.pid {
        return Err(io::Error::other("invalid child identity descriptor"));
    }
    let mut duplicated = ptr::null_mut();
    // SAFETY: 根正保持发布的原句柄且等待release；复制得到同一个挂起叶进程对象。
    if unsafe {
        DuplicateHandle(
            root.spawned.process.handle.as_raw_handle(),
            source as HANDLE,
            GetCurrentProcess(),
            &mut duplicated,
            0,
            0,
            DUPLICATE_SAME_ACCESS,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let process = ProcessHandle {
        handle: owned(duplicated)?,
        pid,
    };
    // SAFETY: 查询复制所得对象，确认发布身份与真实对象一致。
    if unsafe { GetProcessId(process.handle.as_raw_handle()) } != pid {
        return Err(io::Error::other(
            "duplicated child handle identity mismatch",
        ));
    }
    Ok(process)
}

/// 同时报告准确Job归属与原HANDLE状态，持有对象数量不从IOCP消息计数推导。
fn process_snapshot(
    label: &str,
    process: &ProcessHandle,
    outer: &ProbeJob,
    inner: &ProbeJob,
) -> io::Result<(bool, bool, u32)> {
    let in_outer = super::belongs(process.handle.as_raw_handle(), outer.raw())?;
    let in_inner = super::belongs(process.handle.as_raw_handle(), inner.raw())?;
    let wait = process.wait(0)?;
    println!("[windows-nested-accounting] object={label} pid={} in_outer={in_outer} in_inner={in_inner} retained_wait={wait}", process.pid);
    Ok((in_outer, in_inner, wait))
}

/// 先固定accounting/PID列表和对象状态，再读取有界消息，避免混淆两个证据来源。
fn phase(
    mode: &str,
    stage: &str,
    outer: &ProbeJob,
    inner: &ProbeJob,
    root: Option<&ProcessHandle>,
    child: Option<&ProcessHandle>,
) -> io::Result<JobSnapshot> {
    let outer_info = outer.snapshot()?;
    let inner_info = inner.snapshot()?;
    println!("[windows-nested-accounting] mode={mode} phase={stage} unique_retained={} outer_total={} outer_active={} outer_terminated_by_limit={} outer_pids={:?} inner_total={} inner_active={} inner_terminated_by_limit={} inner_pids={:?}", usize::from(root.is_some()) + usize::from(child.is_some()), outer_info.total, outer_info.active, outer_info.terminated_by_limit, outer_info.pids, inner_info.total, inner_info.active, inner_info.terminated_by_limit, inner_info.pids);
    if let Some(root) = root {
        process_snapshot("root", root, outer, inner)?;
    }
    if let Some(child) = child {
        process_snapshot("child", child, outer, inner)?;
    }
    outer.drain(mode, stage, "outer", outer, root, child)?;
    inner.drain(mode, stage, "inner", outer, root, child)?;
    Ok(outer_info)
}

/// 正在观察关联计数时两对象必须都活着，且叶仍只在规定的嵌套层级中。
fn assert_members(
    root: &ProcessHandle,
    child: &ProcessHandle,
    outer: &ProbeJob,
    inner: &ProbeJob,
    nested: bool,
) -> io::Result<()> {
    let root_state = process_snapshot("verify-root", root, outer, inner)?;
    let child_state = process_snapshot("verify-child", child, outer, inner)?;
    let mut expected = vec![root.pid, child.pid];
    expected.sort_unstable();
    if root_state != (true, false, WAIT_TIMEOUT)
        || child_state != (true, nested, WAIT_TIMEOUT)
        || outer.pids()? != expected
        || inner.pids()? != if nested { vec![child.pid] } else { Vec::new() }
    {
        return Err(io::Error::other(
            "nested probe exact two-object membership invariant failed",
        ));
    }
    Ok(())
}

/// flat控制轮与nested实验轮只有两个关联调用不同，且叶主线程始终保持挂起。
fn run_case(directory: &Path, nested: bool) -> io::Result<()> {
    fs::create_dir(directory)?;
    let mode = if nested { "nested" } else { "flat" };
    let outer = ProbeJob::new(101)?;
    let inner = ProbeJob::new(202)?;
    phase(mode, "S0-empty", &outer, &inner, None, None)?;
    let root = RootFixture {
        spawned: SuspendedProcess::spawn("--nested-accounting-root", directory)?,
        directory: directory.to_owned(),
    };
    let assignment = outer.assign(&root.spawned.process);
    println!(
        "[windows-nested-accounting] mode={mode} operation=assign_root_outer result={assignment:?}"
    );
    phase(
        mode,
        "S1-root-suspended",
        &outer,
        &inner,
        Some(&root.spawned.process),
        None,
    )?;
    assignment.require_success()?;
    root.spawned.resume()?;
    let child = observe_child(&root)?;
    let before = phase(
        mode,
        "S2-child-inherited",
        &outer,
        &inner,
        Some(&root.spawned.process),
        Some(&child),
    )?;
    assert_members(&root.spawned.process, &child, &outer, &inner, false)?;
    if nested {
        let assignment = inner.assign(&child);
        println!("[windows-nested-accounting] mode={mode} operation=assign_child_inner result={assignment:?}; no_process_creation_since_S2=true");
        let after = phase(
            mode,
            "S3-child-nested",
            &outer,
            &inner,
            Some(&root.spawned.process),
            Some(&child),
        )?;
        assignment.require_success()?;
        assert_members(&root.spawned.process, &child, &outer, &inner, true)?;
        let classification = match after.total.cmp(&before.total) {
            std::cmp::Ordering::Greater => "outer_total_grew_on_nested_assignment",
            std::cmp::Ordering::Equal => "outer_total_unchanged_on_nested_assignment",
            std::cmp::Ordering::Less => "outer_total_decreased_on_nested_assignment",
        };
        println!("[windows-nested-accounting] classification={classification} before_total={} after_total={} unique_objects=2; production_cleanup_not_asserted=true", before.total, after.total);
        let repeated = inner.assign(&child);
        println!("[windows-nested-accounting] mode={mode} operation=assign_child_same_inner_again result={repeated:?}; no_process_creation_since_S2=true");
        let repeat = phase(
            mode,
            "S4-repeat-same-job",
            &outer,
            &inner,
            Some(&root.spawned.process),
            Some(&child),
        )?;
        assert_members(&root.spawned.process, &child, &outer, &inner, true)?;
        println!("[windows-nested-accounting] same_job_repeat_before_total={} same_job_repeat_after_total={} unique_objects=2", after.total, repeat.total);
        inner.terminate()?;
        outer.drain(
            mode,
            "S5-before-child-wait",
            "outer",
            &outer,
            Some(&root.spawned.process),
            Some(&child),
        )?;
        inner.drain(
            mode,
            "S5-before-child-wait",
            "inner",
            &outer,
            Some(&root.spawned.process),
            Some(&child),
        )?;
    } else {
        println!("[windows-nested-accounting] classification=flat_control total={} unique_objects=2; production_cleanup_not_asserted=true", before.total);
        child.terminate_and_wait()?;
    }
    if child.wait(5_000)? != WAIT_OBJECT_0 {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "nested child did not finish during explicit disposal",
        ));
    }
    phase(
        mode,
        "S5-child-disposed",
        &outer,
        &inner,
        Some(&root.spawned.process),
        Some(&child),
    )?;
    outer.terminate()?;
    outer.drain(
        mode,
        "S6-before-root-wait",
        "outer",
        &outer,
        Some(&root.spawned.process),
        Some(&child),
    )?;
    inner.drain(
        mode,
        "S6-before-root-wait",
        "inner",
        &outer,
        Some(&root.spawned.process),
        Some(&child),
    )?;
    if root.spawned.process.wait(5_000)? != WAIT_OBJECT_0 {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "nested root did not finish during explicit disposal",
        ));
    }
    phase(
        mode,
        "S6-root-disposed",
        &outer,
        &inner,
        Some(&root.spawned.process),
        Some(&child),
    )?;
    if directory.join("leaf-ran").exists() {
        return Err(io::Error::other(
            "nested leaf unexpectedly executed before disposal",
        ));
    }
    println!("[windows-nested-accounting] mode={mode} observation_complete=true root_wait={} child_wait={}; disposal_waits_are_not_product_cleanup_evidence=true", root.spawned.process.wait(0)?, child.wait(0)?);
    Ok(())
}

/// 在默认早期probe和独立--nested-accounting模式都执行两个有限对照轮。
pub(crate) fn run() -> io::Result<()> {
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let directory = ProbeDirectory(std::env::temp_dir().join(format!(
        "narrant-nested-accounting-{}-{suffix}",
        std::process::id()
    )));
    fs::create_dir(&directory.0)?;
    // SAFETY: 伪句柄只查询controller是否处于runner的已有Job，不用于本实验计数。
    let ambient = super::belongs(unsafe { GetCurrentProcess() }, ptr::null_mut())?;
    println!("[windows-nested-accounting] begin arch={} controller_in_ambient_job={ambient} runner_image={:?}", std::env::consts::ARCH, std::env::var_os("ImageVersion"));
    let mut failures = Vec::new();
    for (mode, nested) in [("flat", false), ("nested", true)] {
        if let Err(error) = run_case(&directory.0.join(mode), nested) {
            eprintln!("[windows-nested-accounting] mode={mode} observation_failed={error}");
            failures.push(format!("{mode}: {error}"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(failures.join("; ")))
    }
}

/// 子角色必须在Node环境检查之前分派；叶角色若运行立即留下违规标记。
pub(crate) fn dispatch() -> Option<io::Result<()>> {
    let mut arguments = std::env::args_os().skip(1);
    let role = arguments.next()?;
    if role == "--nested-accounting" {
        return Some(run());
    }
    if role != "--nested-accounting-root" && role != "--nested-accounting-leaf" {
        return None;
    }
    Some((|| {
        let directory = PathBuf::from(
            arguments
                .next()
                .ok_or_else(|| io::Error::other("nested child role requires marker directory"))?,
        );
        if role == "--nested-accounting-leaf" {
            publish(&directory.join("leaf-ran"), "unexpected resume")?;
            return Err(io::Error::other("nested leaf must remain suspended"));
        }
        let result = root_worker(&directory);
        if let Err(error) = &result {
            let _ = publish(&directory.join("root-error"), &error.to_string());
        }
        result
    })())
}
