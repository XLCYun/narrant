//! 共享 Windows 进程树所有权：LSP、Python 和原生 helper 使用同一准确对象完成屏障。
#[path = "windows_tracking.rs"]
mod tracking;

use std::{
    io,
    mem::size_of,
    os::windows::{
        io::{AsHandle, AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    process::{Child, Command},
    ptr,
    sync::{Mutex, TryLockError},
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE},
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        },
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicAccountingInformation,
            JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
            TerminateJobObject, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
            JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK,
        },
        Threading::{
            GetCurrentProcess, OpenThread, ResumeThread, CREATE_NO_WINDOW, CREATE_SUSPENDED,
            THREAD_SUSPEND_RESUME,
        },
    },
};

/// 每个会话独占 Job；准确对象一直保留到最终累计覆盖判定结束。
pub(crate) struct WindowsJob {
    handle: OwnedHandle,
    tracker: Mutex<tracking::JobTracker>,
    #[cfg(test)]
    fail_before_termination: std::sync::atomic::AtomicBool,
}

/// 启动中途失败仍终止并等待直接子进程，不能留下永远挂起的 helper。
struct SuspendedChild(Option<Child>);

impl Drop for SuspendedChild {
    /// 根进程在成功恢复后才转交给 ProcessTree。
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl WindowsJob {
    /// 恢复 helper 之前完成 Job 归属、端口绑定和根身份保留。
    pub(crate) fn spawn(command: &mut Command) -> io::Result<(Child, Self)> {
        let (mut pending, job) = Self::spawn_suspended(command)?;
        resume_initial_thread(pending.0.as_ref().expect("挂起进程尚未转交").id())?;
        Ok((pending.0.take().expect("挂起进程只转交一次"), job))
    }

    /// 挂起阶段完成准确 Job 所有权，所有调用方共用同一启动原语。
    fn spawn_suspended(command: &mut Command) -> io::Result<(SuspendedChild, Self)> {
        // SAFETY: 私有未命名 Job 不继承，不关联其他会话。
        let handle = create_kill_on_close_job()?;
        let tracker = tracking::JobTracker::start(&handle)?;
        let job = Self {
            handle,
            tracker: Mutex::new(tracker),
            #[cfg(test)]
            fail_before_termination: std::sync::atomic::AtomicBool::new(false),
        };
        command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
        let pending = SuspendedChild(Some(command.spawn()?));
        let child = pending.0.as_ref().expect("刚创建的挂起进程");
        // SAFETY: 根进程仍挂起；端口已绑定，分配失败不会执行 helper 代码。
        if unsafe { AssignProcessToJobObject(job.raw(), child.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        job.tracker
            .lock()
            .map_err(|_| io::Error::other("Native Job 启动锁中毒"))?
            .primary(
                job.raw(),
                child.id(),
                child.as_handle().try_clone_to_owned()?,
            )?;
        Ok((pending, job))
    }

    /// 借用准确 Job 供只读诊断，不转移或开放继承所有权。
    pub(crate) fn raw(&self) -> HANDLE {
        self.handle.as_raw_handle()
    }

    /// 活跃计数只用于诊断，不能单独证明进程对象已经完成。
    #[allow(dead_code)] // 独立 Windows 探针直接引用本模块，产品路径不以此作为完成条件。
    pub(crate) fn active_processes(&self) -> io::Result<u32> {
        Ok(self.accounting()?.ActiveProcesses)
    }

    /// 在真实 HANDLE 完成后重新读取累计总数，覆盖已退出历史和失败关联。
    fn accounting(&self) -> io::Result<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION> {
        // SAFETY: POD 输出结构与查询类别、宽度一致。
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
        Ok(info)
    }

    /// 只在清理开始时禁止创建新成员，正常运行期间 helper 保留后代能力。
    fn seal_admission(&self) -> io::Result<()> {
        // SAFETY: POD 输出结构和读写类别、宽度一致。
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        let bytes = size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
        if unsafe {
            QueryInformationJobObject(
                self.raw(),
                JobObjectExtendedLimitInformation,
                (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                bytes,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        limits.BasicLimitInformation.ActiveProcessLimit = 0;
        // SAFETY: 修改当前 helper 的私有 Job，保留其现有限制。
        if unsafe {
            SetInformationJobObject(
                self.raw(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                bytes,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: 读回准确 Job，不能把未生效的门禁当作成功。
        if unsafe {
            QueryInformationJobObject(
                self.raw(),
                JobObjectExtendedLimitInformation,
                (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                bytes,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let flags = limits.BasicLimitInformation.LimitFlags;
        if limits.BasicLimitInformation.ActiveProcessLimit != 0
            || flags & JOB_OBJECT_LIMIT_ACTIVE_PROCESS == 0
            || flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE == 0
            || flags & (JOB_OBJECT_LIMIT_BREAKAWAY_OK | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK) != 0
        {
            return Err(io::Error::other("Native Job 清理门禁读回无效"));
        }
        Ok(())
    }

    /// 终止后继续消费已排队的真实成员通知；累计缺口不能靠提前停止监听变成永久缺口。
    fn wait_for_coverage(
        &self,
        tracker: &tracking::JobTracker,
        deadline: Instant,
    ) -> io::Result<()> {
        loop {
            let info = self.accounting()?;
            if info.ActiveProcesses == 0 && tracker.covers(info.TotalProcesses) {
                return Ok(());
            }
            // 已停止的监听器没有新通知可处理；交给最终 verify 保留准确的覆盖错误。
            if tracker.finished() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Windows Job 累计覆盖等待超时",
                ));
            }
            #[cfg(test)]
            tracker.coverage_wait_started();
            tracker.supplement(self.raw(), deadline)?;
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    /// 共享截止时间等待固定真实对象集合；累计缺口或任何粘性错误均拒绝成功。
    pub(crate) fn terminate_and_wait(&self) -> io::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut tracker = match self.tracker.try_lock() {
            Ok(tracker) => tracker,
            Err(TryLockError::Poisoned(error)) => {
                let tracker = error.into_inner();
                tracker.failure("Native Job 清理锁中毒");
                tracker
            }
            Err(TryLockError::WouldBlock) => {
                return Err(io::Error::other("Native Job 清理已在执行"));
            }
        };
        #[cfg(test)]
        if self
            .fail_before_termination
            .swap(false, std::sync::atomic::Ordering::AcqRel)
        {
            tracker.failure("fixture failure before termination");
            return Err(io::Error::other("fixture failure before termination"));
        }
        if let Err(error) = self.seal_admission() {
            tracker.failure(error);
        }
        if let Err(error) = tracker.supplement(self.raw(), deadline) {
            tracker.failure(error);
        }
        // SAFETY: 只终止此 helper 拥有的准确 Job，不根据外部裸 PID 终止任何对象。
        if unsafe { TerminateJobObject(self.raw(), 1) } == 0 {
            tracker.failure(io::Error::last_os_error());
        }
        if let Err(error) = self.wait_for_coverage(&tracker, deadline) {
            tracker.failure(error);
        }
        if let Err(error) = tracker.stop(deadline) {
            tracker.failure(error);
        }
        if let Err(error) = tracker.supplement(self.raw(), deadline) {
            tracker.failure(error);
        }
        if let Err(error) = tracker.wait(deadline) {
            tracker.failure(error);
        }
        let info = match self.accounting() {
            Ok(info) => info,
            Err(error) => {
                tracker.failure(&error);
                return Err(tracker.first_failure().unwrap_or(error));
            }
        };
        tracker.verify(info.TotalProcesses, info.ActiveProcesses)
    }

    /// 仅测试一次性在终止前失败，区分 ProcessTree 是否实际执行 direct Child 兜底。
    #[cfg(test)]
    pub(crate) fn fail_next_cleanup_before_termination(&self) {
        self.fail_before_termination
            .store(true, std::sync::atomic::Ordering::Release);
    }
}

impl Drop for WindowsJob {
    /// 错误和 panic 同样尝试完成回收；最终关句柄仍有内核 KILL_ON_JOB_CLOSE 兜底。
    fn drop(&mut self) {
        let _ = self.terminate_and_wait();
    }
}

/// 拒绝 Win32 两种无效句柄值，只接管成功创建或打开的唯一所有权。
fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: 调用方仅传入返回所有权的 API 句柄，此处只接管一次。
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// 稳定 Rust 不暴露主线程句柄；只查询刚创建的自有挂起 PID 的唯一线程。
fn resume_initial_thread(pid: u32) -> io::Result<()> {
    // SAFETY: 只读线程快照不继承，不用于枚举或终止其他进程。
    let snapshot = owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) })?;
    // SAFETY: POD 初始化，大小遵循 Win32 要求。
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = size_of::<THREADENTRY32>() as u32;
    let mut available = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
    while available != 0 {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: 仅恢复已归组、根 HANDLE 已保留的自有挂起进程。
            let thread =
                owned(unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) })?;
            return match unsafe { ResumeThread(thread.as_raw_handle()) } {
                1 => Ok(()),
                u32::MAX => Err(io::Error::last_os_error()),
                _ => Err(io::Error::other("Native helper 主线程挂起计数异常")),
            };
        }
        // SAFETY: 继续读取有效快照，不修改其他线程。
        available = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
    }
    Err(io::Error::other("无法找到 Native helper 的挂起主线程"))
}

#[cfg(test)]
#[path = "tests/windows_job.rs"]
pub(crate) mod tests;

/// 共用私有 Job 创建与 kill-on-close 策略，不复制后代枚举或准确对象账本。
fn create_kill_on_close_job() -> io::Result<OwnedHandle> {
    // SAFETY: 私有未命名、不可继承的 Job，不允许后代 breakaway。
    let handle = owned(unsafe { CreateJobObjectW(ptr::null(), ptr::null()) })?;
    // SAFETY: Windows POD 可清零；只开启关闭时终止，不允许 breakaway。
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    if unsafe {
        SetInformationJobObject(
            handle.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(handle)
}

/// 一次性 helper 自身的嵌套 Job；外层 Host 仍持有全部后代的准确退出屏障。
#[allow(dead_code)]
pub(crate) struct CurrentProcessJob(OwnedHandle);

#[allow(dead_code)]
impl CurrentProcessJob {
    /// 在原生工作前把当前进程放入自身 Job，保留已有外层 Job 归属。
    pub(crate) fn attach() -> io::Result<Self> {
        let handle = create_kill_on_close_job()?;
        // SAFETY: GetCurrentProcess 的伪句柄只借用；新 Job 没有 breakaway 标志。
        if unsafe { AssignProcessToJobObject(handle.as_raw_handle(), GetCurrentProcess()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self(handle))
    }

    /// 不等待自身退出；活着的外层 Host 负责原有准确对象完成屏障。
    pub(crate) fn terminate(&self) -> io::Result<()> {
        // SAFETY: 仅终止私有、已包含自身的 Job 及其后代。
        if unsafe { TerminateJobObject(self.0.as_raw_handle(), 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// 所有原生工作结束后解除关闭自杀策略，随后自然退出保留 exit 0。
    pub(crate) fn completed(&self) -> io::Result<()> {
        // SAFETY: POD 输出结构宽度匹配；只更改本 Job 的 kill-on-close 标志。
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        let bytes = size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32;
        if unsafe {
            QueryInformationJobObject(
                self.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&mut limits as *mut JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                bytes,
                ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        limits.BasicLimitInformation.LimitFlags &= !JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if unsafe {
            SetInformationJobObject(
                self.0.as_raw_handle(),
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                bytes,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }
}
