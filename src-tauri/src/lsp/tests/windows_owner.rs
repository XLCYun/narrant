//! 仅 Windows 单元测试使用的 owner seam；产品构建仍直接使用共享 WindowsJob。

use super::windows_process::WindowsJob;
use crate::structure_review::native_analyzer::ValidationWindowsJob;
use std::{
    cell::RefCell,
    io,
    os::windows::io::{AsHandle, OwnedHandle},
    process::{Child, Command},
    sync::{Arc, Mutex},
};

/// 明确标识测试实际创建的 owner，不把旧共享路径的结果记到候选名下。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OwnerKind {
    Baseline,
    NativeCandidate,
}

/// 仅候选支持已有的一次性故障，用来暴露共享 Transport 的错误兜底行为。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CleanupFault {
    None,
    BeforeTerminationOnce,
}

/// 批次之间读取候选累计覆盖；旧 owner 没有账本，因此对应字段明确留空。
#[derive(Clone, Copy, Debug)]
pub(crate) struct OwnerSnapshot {
    pub owner: OwnerKind,
    pub retained: Option<usize>,
    pub total_processes: Option<u32>,
    pub active_processes: u32,
}

/// 请求已经写入并 flush 后观察方法名，不伪造服务器响应或改变协议结果。
pub(crate) type RequestObserver = Arc<dyn Fn(&str) + Send + Sync>;

/// 作用域配置仅存在当前测试线程，不通过环境变量或产品选项传播。
#[derive(Clone)]
pub(crate) struct ValidationOptions {
    pub owner: OwnerKind,
    pub fault: CleanupFault,
    pub after_request_sent: Option<RequestObserver>,
}

impl Default for ValidationOptions {
    /// 未显式选择时所有 Windows 单元测试仍使用旧共享 owner。
    fn default() -> Self {
        Self {
            owner: OwnerKind::Baseline,
            fault: CleanupFault::None,
            after_request_sent: None,
        }
    }
}

/// 只保存直接 root 的准确对象句柄；观察者从不持有 Job 或历史后代句柄。
struct ObservedRoot {
    owner: OwnerKind,
    handle: OwnedHandle,
}

/// 每个作用域独立收集真实启动对象，允许测试确认选择器确实生效。
#[derive(Clone, Default)]
pub(crate) struct SpawnObservations {
    roots: Arc<Mutex<Vec<ObservedRoot>>>,
}

impl SpawnObservations {
    /// 克隆同一组 root 对象句柄，避免按裸 PID 重新打开时发生身份复用。
    pub(crate) fn roots(&self) -> Vec<OwnedHandle> {
        self.roots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|root| root.handle.try_clone().expect("测试 root 句柄克隆失败"))
            .collect()
    }

    /// 按启动顺序报告实际 owner，便于拒绝作用域未生效的候选测试。
    pub(crate) fn owners(&self) -> Vec<OwnerKind> {
        self.roots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|root| root.owner)
            .collect()
    }

    /// 必须检查实际启动数，防止 preflight 提前返回被误认作进程生命周期验收。
    pub(crate) fn spawn_count(&self) -> usize {
        self.roots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }

    /// 在 Child 仍持有准确对象时记录它，不延长 Job 或后代的生命周期。
    fn record(&self, owner: OwnerKind, child: &Child) -> io::Result<()> {
        let handle = child.as_handle().try_clone_to_owned()?;
        self.roots
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(ObservedRoot { owner, handle });
        Ok(())
    }
}

/// 当前线程的选择和观察集合一起保存，嵌套作用域可按栈恢复。
#[derive(Clone)]
struct ScopeState {
    options: ValidationOptions,
    observations: SpawnObservations,
}

thread_local! {
    /// 不向新线程自动传播；同步 Python Session 与 LspClient 在调用线程读取它。
    static CURRENT_SCOPE: RefCell<Option<ScopeState>> = const { RefCell::new(None) };
}

/// 普通返回和 panic 都恢复上层作用域，不影响其他并行测试线程。
struct ScopeGuard(Option<ScopeState>);

impl Drop for ScopeGuard {
    /// 作用域结束只恢复选择器；已经创建的 owner 保持自己的确定类型。
    fn drop(&mut self) {
        CURRENT_SCOPE.with(|scope| {
            *scope.borrow_mut() = self.0.take();
        });
    }
}

/// 在当前线程执行完整会话；测试应在闭包内显式结束或丢弃创建的 client。
pub(crate) fn with_owner<R>(
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

/// 测试只替换 owner，Transport 的正常、错误与 Drop 清理代码保持原样。
pub(super) enum TestWindowsOwner {
    Baseline(WindowsJob),
    NativeCandidate(ValidationWindowsJob),
}

impl TestWindowsOwner {
    /// 直接调用原实现；候选故障只 arm 已有测试钩子，不提前执行清理。
    pub(super) fn spawn(command: &mut Command) -> io::Result<(Child, Self)> {
        let scope = CURRENT_SCOPE.with(|scope| scope.borrow().clone());
        let options = scope
            .as_ref()
            .map(|state| state.options.clone())
            .unwrap_or_default();
        if options.owner == OwnerKind::Baseline && options.fault != CleanupFault::None {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "清理前故障只允许用于 native 候选测试",
            ));
        }
        let (child, owner) = match options.owner {
            OwnerKind::Baseline => {
                let (child, job) = WindowsJob::spawn(command)?;
                (child, Self::Baseline(job))
            }
            OwnerKind::NativeCandidate => {
                let (child, job) = ValidationWindowsJob::spawn(command)?;
                (child, Self::NativeCandidate(job))
            }
        };
        if let Some(scope) = scope {
            scope.observations.record(options.owner, &child)?;
        }
        if options.fault == CleanupFault::BeforeTerminationOnce {
            if let Self::NativeCandidate(job) = &owner {
                job.fail_next_cleanup_before_termination();
            }
        }
        Ok((child, owner))
    }

    /// 原样返回底层结果，不追加 Child 兜底，也不把累计缺口错误改成成功。
    pub(super) fn terminate_and_wait(&self) -> io::Result<()> {
        match self {
            Self::Baseline(job) => job.terminate_and_wait(),
            Self::NativeCandidate(job) => job.terminate_and_wait(),
        }
    }

    /// 活跃计数仅供既有测试观察，绝不充当候选对象完成判定。
    pub(super) fn active_processes(&self) -> io::Result<u32> {
        match self {
            Self::Baseline(job) => job.active_processes(),
            Self::NativeCandidate(job) => job.active_processes(),
        }
    }

    /// 仅由测试主动采样，不在清理返回后添加查询而掩盖立即 Wait(0) 的竞态。
    pub(super) fn snapshot(&self) -> io::Result<OwnerSnapshot> {
        match self {
            Self::Baseline(job) => Ok(OwnerSnapshot {
                owner: OwnerKind::Baseline,
                retained: None,
                total_processes: None,
                active_processes: job.active_processes()?,
            }),
            Self::NativeCandidate(job) => {
                let (retained, total_processes, active_processes) = job.validation_snapshot()?;
                Ok(OwnerSnapshot {
                    owner: OwnerKind::NativeCandidate,
                    retained: Some(retained),
                    total_processes: Some(total_processes),
                    active_processes,
                })
            }
        }
    }
}
