//! Windows 测试只观察真实共享 owner；不选择实现或替换生产清理路径。

use crate::child_process::windows_job::WindowsJob;
use std::{
    cell::RefCell,
    io,
    os::windows::io::{AsHandle, AsRawHandle, OwnedHandle},
    process::Child,
    sync::{Arc, Mutex},
};
use windows_sys::Win32::System::JobObjects::IsProcessInJob;

/// 复用共享 owner 的一次性故障，验证 Transport 错误路径仍等待直接 Child。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CleanupFault {
    None,
    BeforeTerminationOnce,
}

/// 批次之间只读采样真实共享 owner 的累计覆盖，不刷新账本或补等进程。
#[derive(Clone, Copy, Debug)]
pub(crate) struct OwnerSnapshot {
    pub retained: usize,
    pub total_processes: u32,
    pub active: u32,
}

/// 请求已经写入并 flush 后观察方法名，不伪造服务器响应或改变协议结果。
pub(crate) type RequestObserver = Arc<dyn Fn(&str) + Send + Sync>;

/// 作用域仅配置当前测试线程的观测与故障，不改变实际 owner 类型。
#[derive(Clone)]
pub(crate) struct ValidationOptions {
    pub fault: CleanupFault,
    pub after_request_sent: Option<RequestObserver>,
}

impl Default for ValidationOptions {
    /// 默认不注入故障、不安装请求观察器；产品与测试直接使用同一个 owner。
    fn default() -> Self {
        Self {
            fault: CleanupFault::None,
            after_request_sent: None,
        }
    }
}

/// 每个作用域只保留直接 root 的准确句柄；从不持有 Job 或历史后代句柄。
#[derive(Clone, Default)]
pub(crate) struct SpawnObservations {
    roots: Arc<Mutex<Vec<OwnedHandle>>>,
}

impl SpawnObservations {
    /// 克隆同一组 root 对象句柄，避免按裸 PID 重新打开时发生身份复用。
    pub(crate) fn roots(&self) -> Vec<OwnedHandle> {
        self.roots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|root| root.try_clone().expect("测试 root 句柄克隆失败"))
            .collect()
    }

    /// 检查实际启动数，防止 preflight 提前返回被误认作进程生命周期验收。
    pub(crate) fn spawn_count(&self) -> usize {
        self.roots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// 在真实进程守卫已建立之后记录 root；失败仍由该守卫回收直接 Child。
    fn record(&self, child: &Child) -> io::Result<()> {
        let handle = child.as_handle().try_clone_to_owned()?;
        self.roots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(handle);
        Ok(())
    }
}

/// 当前线程的观察选项与对象集合一起保存，嵌套作用域可按栈恢复。
#[derive(Clone)]
struct ScopeState {
    options: ValidationOptions,
    observations: SpawnObservations,
}

thread_local! {
    /// 不向新线程自动传播；同步 Python Session 与 LspClient 在调用线程读取它。
    static CURRENT_SCOPE: RefCell<Option<ScopeState>> = const { RefCell::new(None) };
}

/// 普通返回和 panic 都恢复上层观察作用域，不影响其他并行测试线程。
struct ScopeGuard(Option<ScopeState>);

impl Drop for ScopeGuard {
    /// 作用域结束只恢复观察配置，已创建的会话继续持有自己的真实 owner。
    fn drop(&mut self) {
        CURRENT_SCOPE.with(|scope| {
            *scope.borrow_mut() = self.0.take();
        });
    }
}

/// 在当前线程观察完整会话；测试应在闭包内显式结束或丢弃创建的 client。
pub(crate) fn with_validation<R>(
    options: ValidationOptions,
    body: impl FnOnce(&SpawnObservations) -> R,
) -> R {
    let observations = SpawnObservations::default();
    let state = ScopeState {
        options,
        observations: observations.clone(),
    };
    let previous = CURRENT_SCOPE.with(|scope| scope.replace(Some(state)));
    let _guard = ScopeGuard(previous);
    body(&observations)
}

/// 只在真实 spawn 和 ServerProcess 守卫建立后观察；不启动进程或提前执行清理。
pub(super) fn observe_spawn(child: &Child, job: &WindowsJob) -> io::Result<()> {
    let scope = CURRENT_SCOPE.with(|scope| scope.borrow().clone());
    if let Some(scope) = scope {
        scope.observations.record(child)?;
        if scope.options.fault == CleanupFault::BeforeTerminationOnce {
            job.fail_next_cleanup_before_termination();
        }
    }
    Ok(())
}

/// 在真实 send 成功之后调用测试观察器；先释放线程局部借用再进入用户闭包。
pub(crate) fn post_send(method: &str) {
    let observer = CURRENT_SCOPE.with(|scope| {
        scope
            .borrow()
            .as_ref()
            .and_then(|state| state.options.after_request_sent.clone())
    });
    if let Some(observer) = observer {
        observer(method);
    }
}

/// 借用准确 Job 和进程句柄，不复制 Job 或改变会话所有权。
pub(super) fn contains(job: &WindowsJob, process: &OwnedHandle) -> io::Result<bool> {
    let mut member = 0;
    // SAFETY: 两个对象均在调用期间被借用，查询不转移句柄所有权。
    if unsafe { IsProcessInJob(process.as_raw_handle(), job.raw(), &mut member) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(member != 0)
}

/// 仅由测试主动采样，不在清理返回后自动查询而掩盖立即 Wait(0) 的竞态。
pub(super) fn snapshot(job: &WindowsJob) -> io::Result<OwnerSnapshot> {
    let (retained, total_processes, active) = job.validation_snapshot()?;
    Ok(OwnerSnapshot {
        retained,
        total_processes,
        active,
    })
}
