//! Windows LSP 专属 Job 所有权：服务器执行前入组，父进程退出后仍能回收全部后代。
use std::{
    io,
    mem::size_of,
    os::windows::{
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
        process::CommandExt,
    },
    process::{Child, Command},
    ptr,
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
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        },
        Threading::{
            OpenThread, ResumeThread, CREATE_NO_WINDOW, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME,
        },
    },
};

/// 非继承 Job 句柄持有整个会话；不允许 breakaway，后代自动保留在同一 Job 中。
pub(super) struct WindowsJob(OwnedHandle);

/// 尚未交给共享传输的子进程守卫，覆盖分配 Job 或恢复执行失败的启动窗口。
struct SuspendedChild(Option<Child>);
impl Drop for SuspendedChild {
    /// 未转交时无论是否已入组都先终止直接子进程，避免遗留永远挂起的进程。
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl WindowsJob {
    /// 使用稳定 Win32 接口建立所有权后才恢复主线程，不依赖 Rust nightly 扩展。
    pub(super) fn spawn(command: &mut Command) -> io::Result<(Child, Self)> {
        let (mut pending, job) = Self::spawn_suspended(command)?;
        resume_initial_thread(pending.0.as_ref().expect("挂起进程尚未转交").id())?;
        Ok((pending.0.take().expect("挂起进程只转交一次"), job))
    }

    /// CREATE_SUSPENDED 关闭 spawn 到 AssignProcessToJobObject 之间的后代逃逸窗口。
    fn spawn_suspended(command: &mut Command) -> io::Result<(SuspendedChild, Self)> {
        // SAFETY: 不命名也不继承 Job，返回句柄仅由当前守卫管理。
        let job = Self(owned(unsafe {
            CreateJobObjectW(ptr::null(), ptr::null())
        })?);
        // SAFETY: Windows POD 结构可清零，只显式开启关闭句柄时杀死成员的限制。
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: 结构指针、信息类别和长度一致，Job 句柄在整个调用期间有效。
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
        command.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
        let pending = SuspendedChild(Some(command.spawn()?));
        let child = pending.0.as_ref().expect("刚创建的挂起进程");
        // SAFETY: 子进程仍挂起且句柄由 pending 持有，分配失败不会执行服务器代码。
        if unsafe { AssignProcessToJobObject(job.raw(), child.as_raw_handle()) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok((pending, job))
    }

    /// 返回当前 Job 句柄，不允许调用方接管或继承其生命周期。
    fn raw(&self) -> HANDLE {
        self.0.as_raw_handle()
    }

    /// ActiveProcesses 来自拥有的 Job，父 PID 是否仍存在不影响成员统计。
    fn active_processes(&self) -> io::Result<u32> {
        // SAFETY: Windows POD 信息结构可清零，查询按确切结构宽度填写。
        let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: 输出缓冲区和类别匹配，当前对象仍持有 Job 句柄。
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
        Ok(info.ActiveProcesses)
    }

    /// 终止和有界等待全部 Job 成员；成功返回必须表示父子进程均已停止。
    pub(super) fn terminate_and_wait(&self) -> io::Result<()> {
        // SAFETY: Job 为当前会话专有，不会结束其他应用或其他 LSP 会话。
        if unsafe { TerminateJobObject(self.raw(), 1) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let deadline = Instant::now() + Duration::from_secs(2);
        while self.active_processes()? != 0 {
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Windows LSP Job 回收超时",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}
impl Drop for WindowsJob {
    /// 错误或 panic 仍尝试等待；句柄最终关闭时 KILL_ON_JOB_CLOSE 提供系统级兜底。
    fn drop(&mut self) {
        let _ = self.terminate_and_wait();
    }
}

/// Win32 的两类无效句柄都不能进入 OwnedHandle，防止失败路径误关句柄。
fn owned(handle: HANDLE) -> io::Result<OwnedHandle> {
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: 句柄来自返回所有权的 Win32 创建/打开调用，只在这里接管一次。
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

/// 稳定 Rust 未公开主线程句柄；原生线程快照仅用于定位自有挂起 PID 的唯一线程。
/// 不枚举进程树、不启动 shell，不接触其他进程的线程句柄。
fn resume_initial_thread(pid: u32) -> io::Result<()> {
    // SAFETY: 只创建只读线程快照，不继承句柄。
    let snapshot = owned(unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) })?;
    // SAFETY: THREADENTRY32 是 POD，dwSize 按 Win32 文档要求设置。
    let mut entry: THREADENTRY32 = unsafe { std::mem::zeroed() };
    entry.dwSize = size_of::<THREADENTRY32>() as u32;
    // SAFETY: 快照和结构缓冲区均在迭代期间有效。
    let mut available = unsafe { Thread32First(snapshot.as_raw_handle(), &mut entry) };
    while available != 0 {
        if entry.th32OwnerProcessID == pid {
            // SAFETY: 仅为刚刚创建、尚未运行的自有子进程打开恢复线程所需权限。
            let thread =
                owned(unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID) })?;
            // SAFETY: 子进程已入 Job 才执行此处；恢复计数必须从 1 变为 0。
            return match unsafe { ResumeThread(thread.as_raw_handle()) } {
                1 => Ok(()),
                u32::MAX => Err(io::Error::last_os_error()),
                _ => Err(io::Error::other("Windows LSP 主线程挂起计数异常")),
            };
        }
        // SAFETY: 继续读取同一个有效快照，不改变任何线程状态。
        available = unsafe { Thread32Next(snapshot.as_raw_handle(), &mut entry) };
    }
    Err(io::Error::other("无法找到 Windows LSP 的挂起主线程"))
}

#[cfg(test)]
#[path = "tests/windows_process.rs"]
mod tests;
