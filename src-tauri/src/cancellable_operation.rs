use crate::core_git::CancellationToken;
use std::collections::HashMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
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

/// 一次应用操作可以观察的运行上下文。
#[derive(Clone)]
pub struct OperationContext {
    cancellation: CancellationToken,
}

impl OperationContext {
    /// 从已有共享标记创建跨模块传递的操作上下文。
    pub(crate) fn from_cancellation(cancellation: CancellationToken) -> Self {
        Self { cancellation }
    }

    /// 返回跨越本次应用操作全部可取消阶段的共享标记。
    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    /// 创建不响应外部取消的上下文，供必须收敛的领域阶段使用。
    pub fn non_cancellable() -> Self {
        Self {
            cancellation: CancellationToken::default(),
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
        token: CancellationToken,
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
            let token = CancellationToken::default();
            state.entries.insert(
                id.clone(),
                OperationEntry::Running {
                    registration,
                    token: token.clone(),
                },
            );
            (
                OperationContext {
                    cancellation: token,
                },
                registration,
            )
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
            Some(OperationEntry::Running { token, .. }) => token.cancel(),
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
#[path = "cancellable_operation_tests.rs"]
mod tests;
