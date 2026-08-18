use std::collections::HashMap;
use std::future::Future;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::time::{Duration, Instant};

const EARLY_CANCELLATION_TTL: Duration = Duration::from_secs(60);
const MAX_EARLY_CANCELLATIONS: usize = 1_024;

/// 一次可取消应用操作的稳定身份。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OperationId(String);

impl OperationId {
    /// 校验并创建非空且有界的操作身份。
    pub fn new(value: impl Into<String>) -> Result<Self, OperationError> {
        let value = value.into();
        if value.is_empty() || value.len() > 200 {
            return Err(OperationError::InvalidOperationId);
        }
        Ok(Self(value))
    }
}

/// 应用操作可只读观察的取消信号，不能触发上层操作取消。
#[derive(Clone, Default)]
pub(crate) struct CancellationSignal(Arc<AtomicBool>);

impl CancellationSignal {
    /// 仅由同模块的取消 source 标记共享状态。
    fn mark_cancelled(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// 返回调用方是否已经请求取消。
    pub(crate) fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

/// 仅供操作 registry 持有的取消触发权。
#[derive(Clone, Default)]
struct CancellationSource(CancellationSignal);

impl CancellationSource {
    /// 创建一对共享状态的触发源和只读观察信号。
    fn new() -> (Self, CancellationSignal) {
        let signal = CancellationSignal::default();
        (Self(signal.clone()), signal)
    }

    /// 请求取消所有共享此 source 的操作阶段。
    fn cancel(&self) {
        self.0.mark_cancelled();
    }
}

/// 仅测试使用的取消触发器，避免向生产调用链暴露取消权限。
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct TestCancellationController(CancellationSource);

#[cfg(test)]
impl TestCancellationController {
    /// 请求取消测试中与其配对的只读信号。
    pub(crate) fn cancel(&self) {
        self.0.cancel();
    }
}

/// 为机制层测试构造取消 source 与只读 signal 配对。
#[cfg(test)]
pub(crate) fn test_cancellation_signal() -> (TestCancellationController, CancellationSignal) {
    let (source, signal) = CancellationSource::new();
    (TestCancellationController(source), signal)
}

/// 机制层测试可用的取消配对，同时保持生产 API 只读。
#[cfg(test)]
#[derive(Clone)]
pub(crate) struct TestCancellation {
    controller: TestCancellationController,
    signal: CancellationSignal,
}

#[cfg(test)]
impl Default for TestCancellation {
    /// 创建尚未取消的测试信号。
    fn default() -> Self {
        let (controller, signal) = test_cancellation_signal();
        Self { controller, signal }
    }
}

#[cfg(test)]
impl TestCancellation {
    /// 请求取消配对信号。
    pub(crate) fn cancel(&self) {
        self.controller.cancel();
    }
}

#[cfg(test)]
impl std::ops::Deref for TestCancellation {
    type Target = CancellationSignal;

    /// 将测试配对透明借用为机制层需要的只读信号。
    fn deref(&self) -> &Self::Target {
        &self.signal
    }
}

/// 一次应用操作可以观察的运行上下文。
#[derive(Clone)]
pub struct OperationContext {
    cancellation: CancellationSignal,
}

impl OperationContext {
    /// 创建没有外部取消来源的后台根操作上下文。
    pub fn background() -> Self {
        Self {
            cancellation: CancellationSignal::default(),
        }
    }

    /// 从当前上下文派生一个保留未来字段但不响应取消的收敛上下文。
    pub fn without_cancellation(&self) -> Self {
        Self::background()
    }

    /// 返回跨越本次应用操作全部可取消阶段的只读共享信号。
    pub(crate) fn cancellation(&self) -> &CancellationSignal {
        &self.cancellation
    }

    /// 从测试信号构造上下文，以验证取消传播而不开放生产构造入口。
    #[cfg(test)]
    pub(crate) fn from_test_cancellation(cancellation: &TestCancellation) -> Self {
        Self {
            cancellation: cancellation.signal.clone(),
        }
    }
}

/// 可取消应用操作注册失败的稳定类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationError {
    InvalidOperationId,
    DuplicateOperation,
    Cancelled,
    RegistryUnavailable,
}

impl std::fmt::Display for OperationError {
    /// 将稳定类别转换为面向调用方的诊断信息。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidOperationId => "操作 ID 无效。",
            Self::DuplicateOperation => "相同 ID 的操作正在运行。",
            Self::Cancelled => "操作已取消。",
            Self::RegistryUnavailable => "操作状态不可用。",
        })
    }
}

impl std::error::Error for OperationError {}

/// Registry 中运行项与先到取消请求的内部表示。
enum OperationEntry {
    EarlyCancellation {
        requested_at: Instant,
    },
    Running {
        registration: u64,
        source: CancellationSource,
    },
}

#[derive(Default)]
/// 可取消操作 registry 的全部可变状态。
struct OperationState {
    next_registration: u64,
    entries: HashMap<OperationId, OperationEntry>,
}

/// 集中管理应用操作的注册、取消投递、重复拒绝和自动释放。
#[derive(Clone, Default)]
pub struct CancellableOperations {
    state: Arc<Mutex<OperationState>>,
}

impl CancellableOperations {
    /// 注册并运行一次操作；无论成功、失败或 future 被丢弃都会自动释放注册。
    pub async fn run<T, F, Fut>(&self, id: OperationId, operation: F) -> Result<T, OperationError>
    where
        F: FnOnce(OperationContext) -> Fut,
        Fut: Future<Output = T>,
    {
        let (context, registration) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| OperationError::RegistryUnavailable)?;
            prune_early_cancellations(&mut state);
            match state.entries.get(&id) {
                Some(OperationEntry::Running { .. }) => {
                    return Err(OperationError::DuplicateOperation);
                }
                Some(OperationEntry::EarlyCancellation { .. }) => {
                    state.entries.remove(&id);
                    return Err(OperationError::Cancelled);
                }
                None => {}
            }
            state.next_registration = state.next_registration.wrapping_add(1);
            let registration = state.next_registration;
            let (source, cancellation) = CancellationSource::new();
            state.entries.insert(
                id.clone(),
                OperationEntry::Running {
                    registration,
                    source,
                },
            );
            (OperationContext { cancellation }, registration)
        };
        let _release = OperationRegistration {
            state: self.state.clone(),
            id,
            registration,
        };
        Ok(operation(context).await)
    }

    /// 取消运行中的操作，或暂存一个尚未开始操作的取消请求。
    pub fn cancel(&self, id: &OperationId) -> Result<(), OperationError> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| OperationError::RegistryUnavailable)?;
        prune_early_cancellations(&mut state);
        match state.entries.get(id) {
            Some(OperationEntry::Running { source, .. }) => source.cancel(),
            Some(OperationEntry::EarlyCancellation { .. }) => {}
            None => {
                if early_cancellation_count(&state) >= MAX_EARLY_CANCELLATIONS {
                    remove_oldest_early_cancellation(&mut state);
                }
                state.entries.insert(
                    id.clone(),
                    OperationEntry::EarlyCancellation {
                        requested_at: Instant::now(),
                    },
                );
            }
        }
        Ok(())
    }
}

/// 在 future 生命周期结束时精确释放本次注册的 RAII guard。
struct OperationRegistration {
    state: Arc<Mutex<OperationState>>,
    id: OperationId,
    registration: u64,
}

impl Drop for OperationRegistration {
    /// 只释放本次注册，避免旧 future 删除同 ID 的后继操作。
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.lock() {
            let owns_entry = matches!(
                state.entries.get(&self.id),
                Some(OperationEntry::Running { registration, .. })
                    if *registration == self.registration
            );
            if owns_entry {
                state.entries.remove(&self.id);
            }
        }
    }
}

fn prune_early_cancellations(state: &mut OperationState) {
    state.entries.retain(|_, entry| match entry {
        OperationEntry::EarlyCancellation { requested_at } => {
            requested_at.elapsed() < EARLY_CANCELLATION_TTL
        }
        OperationEntry::Running { .. } => true,
    });
}

fn early_cancellation_count(state: &OperationState) -> usize {
    state
        .entries
        .values()
        .filter(|entry| matches!(entry, OperationEntry::EarlyCancellation { .. }))
        .count()
}

fn remove_oldest_early_cancellation(state: &mut OperationState) {
    let oldest = state
        .entries
        .iter()
        .filter_map(|(id, entry)| match entry {
            OperationEntry::EarlyCancellation { requested_at } => Some((id.clone(), *requested_at)),
            OperationEntry::Running { .. } => None,
        })
        .min_by_key(|(_, requested_at)| *requested_at)
        .map(|(id, _)| id);
    if let Some(id) = oldest {
        state.entries.remove(&id);
    }
}

#[cfg(test)]
#[path = "cancellable_tests.rs"]
mod tests;
