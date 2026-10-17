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

/// 作用域仅配置当前测试线程的观测与故障，不改变实际 owner 类型。
#[derive(Clone)]
pub(crate) struct ValidationOptions {
    pub fault: CleanupFault,
}

impl Default for ValidationOptions {
    /// 默认不注入故障；产品与测试直接使用同一个 owner。
    fn default() -> Self {
        Self {
            fault: CleanupFault::None,
        }
    }
}

/// 每个作用域只保留直接 root 的准确句柄；从不持有 Job 或历史后代句柄。
#[derive(Clone, Default)]
pub(crate) struct SpawnObservations {
    roots: Arc<Mutex<Vec<OwnedHandle>>>,
}

impl SpawnObservations {
    /// 同步入口返回后先采样全部原始对象，调用方再断言或检查目录。
    pub(crate) fn root_statuses(&self) -> Vec<u32> {
        self.roots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(super::test_support::retained_status)
            .collect()
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
    /// 不向新线程自动传播；Native Host 的同步入口在调用线程读取它。
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

/// 只在真实 spawn 和 ProcessTree 守卫建立后观察；不启动进程或提前执行清理。
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

/// 借用准确 Job 和进程句柄，不复制 Job 或改变会话所有权。
pub(super) fn contains(job: &WindowsJob, process: &OwnedHandle) -> io::Result<bool> {
    let mut member = 0;
    // SAFETY: 两个对象均在调用期间被借用，查询不转移句柄所有权。
    if unsafe { IsProcessInJob(process.as_raw_handle(), job.raw(), &mut member) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(member != 0)
}
