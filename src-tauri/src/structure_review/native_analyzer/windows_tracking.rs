//! Native helper 专属账本：结合准确对象完成与累计计数确认整棵进程树。

use super::owned;
use std::{
    collections::BTreeMap,
    io,
    mem::{offset_of, size_of},
    os::windows::io::{AsRawHandle, OwnedHandle},
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex, MutexGuard,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};
use windows_sys::Win32::{
    Foundation::{
        ERROR_INVALID_PARAMETER, ERROR_MORE_DATA, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0,
        WAIT_TIMEOUT,
    },
    System::{
        JobObjects::{
            IsProcessInJob, JobObjectAssociateCompletionPortInformation,
            JobObjectBasicProcessIdList, QueryInformationJobObject, SetInformationJobObject,
            JOBOBJECT_ASSOCIATE_COMPLETION_PORT, JOBOBJECT_BASIC_PROCESS_ID_LIST,
        },
        SystemServices::JOB_OBJECT_MSG_NEW_PROCESS,
        Threading::{
            OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SYNCHRONIZE,
        },
        IO::{CreateIoCompletionPort, GetQueuedCompletionStatus},
    },
};

/// 记录整个会话的唯一对象，避免长期服务器无限保留句柄。
const MAX_PROCESSES: usize = 65_536;
/// 私有端口只接收本 Job 通知；异常通知流也必须有界。
const MAX_EVENTS: usize = MAX_PROCESSES * 8;
/// 私有完成端口的固定归属键，不接受其他来源的消息。
const JOB_KEY: usize = 1;

/// 持续持有已确认对象，signaled 后也不删除，以免 PID 重用破坏累计对账。
#[derive(Default)]
struct Ledger {
    processes: BTreeMap<u32, OwnedHandle>,
    failure: Option<String>,
}

/// 端口监听线程与句柄账本的唯一所有者，停止后才允许最终完成判定。
pub(super) struct JobTracker {
    ledger: Arc<Mutex<Ledger>>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
    #[cfg(test)]
    hooks: Arc<TestHooks>,
}

/// 仅测试用确定性同步点，冻结 dequeue 到捕获之间的时序，不改变生产监听协议。
#[cfg(test)]
#[derive(Default)]
struct TestHooks {
    before_capture: Mutex<Option<CaptureHook>>,
    before_coverage_wait: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    fail_stop_once: AtomicBool,
    waits: std::sync::atomic::AtomicUsize,
}

/// 测试可读取停止信号后放行当前消息，证明 stop 不会丢弃已取出的通知。
#[cfg(test)]
type CaptureHook = Arc<dyn Fn(u32, &AtomicBool) + Send + Sync>;

/// 锁中毒也是粘性错误，仍取回已有句柄完成尽可能多的回收。
fn lock(ledger: &Mutex<Ledger>) -> MutexGuard<'_, Ledger> {
    match ledger.lock() {
        Ok(guard) => guard,
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            guard
                .failure
                .get_or_insert_with(|| "Windows Job 跟踪状态锁中毒".into());
            guard
        }
    }
}

/// 第一项异常保留到 Job 结束，第二次清理不能把漏捕获改称成功。
fn remember(ledger: &Mutex<Ledger>, error: impl std::fmt::Display) {
    lock(ledger)
        .failure
        .get_or_insert_with(|| error.to_string());
}

/// 只把真实句柄且属于准确 Job 的唯一对象纳入账本。
fn insert(ledger: &Mutex<Ledger>, job: HANDLE, pid: u32, process: OwnedHandle) -> io::Result<()> {
    let mut member = 0;
    // SAFETY: 两个句柄均由调用方持有，输出指针在调用期间有效。
    if unsafe { IsProcessInJob(process.as_raw_handle(), job, &mut member) } == 0 {
        return Err(io::Error::last_os_error());
    }
    if member == 0 {
        return Err(io::Error::other("Windows Job 捕获对象不属于当前 Job"));
    }
    let mut ledger = lock(ledger);
    if ledger.processes.contains_key(&pid) {
        return Ok(());
    }
    if ledger.processes.len() >= MAX_PROCESSES {
        return Err(io::Error::other("Windows Job 生命周期进程数超限"));
    }
    ledger.processes.insert(pid, process);
    Ok(())
}

/// 已消失 PID 不计作完成；累计 TotalProcesses 会在最终判定发现缺口。
fn capture(ledger: &Mutex<Ledger>, job: HANDLE, pid: u32) -> io::Result<()> {
    if lock(ledger).processes.contains_key(&pid) {
        return Ok(());
    }
    // SAFETY: 仅请求查询和同步权限，绝不按通知中的裸 PID 终止进程。
    let handle = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if handle.is_null() {
        let error = io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_INVALID_PARAMETER as i32) {
            Ok(())
        } else {
            Err(error)
        };
    }
    insert(ledger, job, pid, owned(handle)?)
}

impl JobTracker {
    /// 在首个进程入组前绑定私有端口，并准备通知捕获线程。
    pub fn start(job: &OwnedHandle) -> io::Result<Self> {
        // SAFETY: 无关联文件的私有端口，不命名、不继承，句柄由守卫接管。
        let port =
            owned(unsafe { CreateIoCompletionPort(INVALID_HANDLE_VALUE, ptr::null_mut(), 0, 1) })?;
        let association = JOBOBJECT_ASSOCIATE_COMPLETION_PORT {
            CompletionKey: JOB_KEY as *mut _,
            CompletionPort: port.as_raw_handle(),
        };
        // SAFETY: 结构类别和大小一致，Job 与端口都在调用期间有效。
        if unsafe {
            SetInformationJobObject(
                job.as_raw_handle(),
                JobObjectAssociateCompletionPortInformation,
                (&association as *const JOBOBJECT_ASSOCIATE_COMPLETION_PORT).cast(),
                size_of::<JOBOBJECT_ASSOCIATE_COMPLETION_PORT>() as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let ledger = Arc::new(Mutex::new(Ledger::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker_ledger = ledger.clone();
        let worker_stop = stop.clone();
        #[cfg(test)]
        let hooks = Arc::new(TestHooks::default());
        #[cfg(test)]
        let worker_hooks = hooks.clone();
        let job = job.try_clone()?;
        let worker = std::thread::Builder::new()
            .name("windows-job-members".into())
            .spawn(move || {
                watch(
                    job,
                    port,
                    &worker_ledger,
                    &worker_stop,
                    #[cfg(test)]
                    &worker_hooks,
                );
            })?;
        Ok(Self {
            ledger,
            stop,
            worker: Some(worker),
            #[cfg(test)]
            hooks,
        })
    }

    /// 根进程在恢复执行之前直接登记，不依赖异步 NEW_PROCESS 到达。
    pub fn primary(&self, job: HANDLE, pid: u32, process: OwnedHandle) -> io::Result<()> {
        insert(&self.ledger, job, pid, process)
    }

    /// 主动登记错误后仍继续自有 Job 的终止流程。
    pub fn failure(&self, error: impl std::fmt::Display) {
        remember(&self.ledger, error);
    }

    /// 后续查询也失败时返回第一项清理错误，避免把原始原因覆盖成次生错误。
    pub fn first_failure(&self) -> Option<io::Error> {
        lock(&self.ledger)
            .failure
            .as_ref()
            .map(|error| io::Error::other(error.clone()))
    }

    /// 已经停止或退出的监听器不能再补齐历史，调用方应立即做最终严格核对。
    pub fn finished(&self) -> bool {
        self.worker.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// 只查询真实唯一对象覆盖，不把 PID 列表或通知数充当已完成证明。
    pub fn covers(&self, total: u32) -> bool {
        lock(&self.ledger).processes.len() == total as usize
    }

    /// 仅测试在第一次等待累计覆盖前放行已冻结的真实消息。
    #[cfg(test)]
    pub fn coverage_wait_started(&self) {
        let hook = self.hooks.before_coverage_wait.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
    }

    /// 精确 Job 列表补捕通知尚未覆盖的对象；查不到的历史成员仍由累计计数发现。
    pub fn supplement(&self, job: HANDLE, deadline: Instant) -> io::Result<()> {
        const HEADER: usize =
            offset_of!(JOBOBJECT_BASIC_PROCESS_ID_LIST, ProcessIdList) / size_of::<usize>();
        let mut buffer = vec![0usize; HEADER + 64];
        loop {
            if Instant::now() >= deadline {
                return Err(timeout());
            }
            // SAFETY: usize 缓冲区满足结构对齐，包含固定头部和可变长 PID 空间。
            let success = unsafe {
                QueryInformationJobObject(
                    job,
                    JobObjectBasicProcessIdList,
                    buffer.as_mut_ptr().cast(),
                    (buffer.len() * size_of::<usize>()) as u32,
                    ptr::null_mut(),
                )
            };
            let error = io::Error::last_os_error();
            // SAFETY: 固定头部位于有效且正确对齐的缓冲区内。
            let info = unsafe { &*buffer.as_ptr().cast::<JOBOBJECT_BASIC_PROCESS_ID_LIST>() };
            if success == 0 && error.raw_os_error() != Some(ERROR_MORE_DATA as i32) {
                return Err(error);
            }
            if success == 0 || info.NumberOfProcessIdsInList < info.NumberOfAssignedProcesses {
                let capacity = (info.NumberOfAssignedProcesses as usize).max(buffer.len() * 2);
                if capacity > MAX_PROCESSES {
                    return Err(io::Error::other("Windows Job 成员列表超限"));
                }
                buffer.resize(HEADER + capacity, 0);
                continue;
            }
            let count = info.NumberOfProcessIdsInList as usize;
            if count > buffer.len() - HEADER {
                return Err(io::Error::other("Windows Job 成员列表长度无效"));
            }
            for raw_pid in &buffer[HEADER..HEADER + count] {
                if Instant::now() >= deadline {
                    return Err(timeout());
                }
                capture(
                    &self.ledger,
                    job,
                    u32::try_from(*raw_pid).map_err(io::Error::other)?,
                )?;
            }
            return Ok(());
        }
    }

    /// 覆盖等待结束后才请求停止；已经取出的消息必须处理完才能退出。
    pub fn stop(&mut self, deadline: Instant) -> io::Result<()> {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = &self.worker {
            while !worker.is_finished() {
                if Instant::now() >= deadline {
                    self.failure("Windows Job 跟踪线程停止超时");
                    return Err(timeout());
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        if self
            .worker
            .take()
            .is_some_and(|worker| worker.join().is_err())
        {
            self.failure("Windows Job 跟踪线程 panic");
        }
        #[cfg(test)]
        if self.hooks.fail_stop_once.swap(false, Ordering::AcqRel) {
            return Err(io::Error::other("fixture failure after listener stop"));
        }
        Ok(())
    }

    /// 等待当前已持有的全部对象；stop 失败时仍尽力等待，外层保留该失败且禁止成功。
    pub fn wait(&self, deadline: Instant) -> io::Result<()> {
        #[cfg(test)]
        self.hooks.waits.fetch_add(1, Ordering::AcqRel);
        let ledger = lock(&self.ledger);
        let mut failure = None;
        for process in ledger.processes.values() {
            let millis = deadline
                .saturating_duration_since(Instant::now())
                .as_millis()
                .min(u32::MAX as u128) as u32;
            // SAFETY: 同一个对象句柄持续持有，所有等待共用一个截止时间。
            match unsafe { WaitForSingleObject(process.as_raw_handle(), millis) } {
                WAIT_OBJECT_0 => {}
                WAIT_TIMEOUT => {
                    failure.get_or_insert_with(timeout);
                }
                _ => {
                    failure.get_or_insert_with(io::Error::last_os_error);
                }
            }
        }
        failure.map_or(Ok(()), Err)
    }

    /// 最终累计计数不匹配或曾经发生捕获异常，都不能宣称全部完成。
    pub fn verify(&self, total: u32, active: u32) -> io::Result<()> {
        let mut ledger = lock(&self.ledger);
        if let Some(error) = &ledger.failure {
            return Err(io::Error::other(error.clone()));
        }
        if total as usize != ledger.processes.len() || active != 0 {
            let error = format!(
                "Windows Job 完成覆盖不足：保留 {}，累计 {total}，活跃 {active}",
                ledger.processes.len()
            );
            ledger.failure = Some(error.clone());
            return Err(io::Error::other(error));
        }
        Ok(())
    }

    /// 只读展示已持有的真实唯一对象数，绝不按收到的通知数量计数。
    #[cfg(test)]
    pub fn recorded(&self) -> usize {
        lock(&self.ledger).processes.len()
    }

    /// 安装一次真实消息同步点，仅受控测试可以使用。
    #[cfg(test)]
    pub fn on_capture(&self, hook: CaptureHook) {
        *self.hooks.before_capture.lock().unwrap() = Some(hook);
    }

    /// 安装累计覆盖等待同步点，不用定时 sleep 决定何时放行消息。
    #[cfg(test)]
    pub fn on_coverage_wait(&self, hook: impl FnOnce() + Send + 'static) {
        *self.hooks.before_coverage_wait.lock().unwrap() = Some(Box::new(hook));
    }

    /// 已实际停止监听之后注入错误，验证外层不能跳过准确对象等待。
    #[cfg(test)]
    pub fn fail_next_stop(&self) {
        self.hooks.fail_stop_once.store(true, Ordering::Release);
    }

    /// 仅测试区分显式错误路径执行了等待，还是随后 Drop 才补做等待。
    #[cfg(test)]
    pub fn waits(&self) -> usize {
        self.hooks.waits.load(Ordering::Acquire)
    }
}

impl Drop for JobTracker {
    /// 错误路径也收拢线程，不把仍持自有 Job 的线程悄悄 detach。
    fn drop(&mut self) {
        let _ = self.stop(Instant::now() + Duration::from_secs(2));
        if let Some(worker) = self.worker.take() {
            // 停止信号已发布，线程不排空队列，也不再开始下一次捕获。
            // 极端调度或 API 停滞时兜底 join 不提供硬墙钟上限，超时仍是错误。
            let _ = worker.join();
        }
    }
}

/// 只处理自有端口的 NEW_PROCESS，退出消息从不删除已有身份。
fn watch(
    job: OwnedHandle,
    port: OwnedHandle,
    ledger: &Mutex<Ledger>,
    stop: &AtomicBool,
    #[cfg(test)] hooks: &TestHooks,
) {
    let mut events = 0usize;
    loop {
        if stop.load(Ordering::Acquire) {
            break;
        }
        let mut code = 0;
        let mut key = 0;
        let mut value = ptr::null_mut();
        // SAFETY: 端口由线程持有，输出参数有效；Job 消息将 value 作为 PID，绝不解引用。
        let success = unsafe {
            GetQueuedCompletionStatus(port.as_raw_handle(), &mut code, &mut key, &mut value, 10)
        };
        if success == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(WAIT_TIMEOUT as i32) {
                if stop.load(Ordering::Acquire) {
                    break;
                }
                continue;
            }
            remember(ledger, error);
            break;
        }
        // 同步点位于任何 dequeue 后停止判断之前，回归必须能冻结“已取出但尚未处理”。
        #[cfg(test)]
        if key == JOB_KEY && code == JOB_OBJECT_MSG_NEW_PROCESS {
            let hook = hooks.before_capture.lock().unwrap().clone();
            if let Some(hook) = hook {
                hook(value as usize as u32, stop);
            }
        }
        events += 1;
        if events > MAX_EVENTS || key != JOB_KEY {
            remember(ledger, "Windows Job 通知数量或来源无效");
            break;
        }
        if code == JOB_OBJECT_MSG_NEW_PROCESS {
            match u32::try_from(value as usize)
                .map_err(io::Error::other)
                .and_then(|pid| capture(ledger, job.as_raw_handle(), pid))
            {
                Ok(()) => {}
                Err(error) => remember(ledger, error),
            }
        }
    }
}

/// 所有阶段使用同一错误类型表达清理截止时间耗尽。
fn timeout() -> io::Error {
    io::Error::new(io::ErrorKind::TimedOut, "Windows Job 完成等待超时")
}
