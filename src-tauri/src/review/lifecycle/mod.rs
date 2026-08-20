use crate::core_git::{GitCommandError, GitProcessAdapter};
use crate::operation::OperationContext;
use crate::persistence::{PersistedReviewState, ReviewStore, ReviewWorkspaceCleanupTarget};
use crate::repository::{RepositoryApplication, RepositoryApplicationError};
use crate::review::FileChangeContent;
use crate::review::PullRequestMetadata;
use crate::review::PullRequestResolutionError;
use crate::review::{BranchSelection, BranchSelectionResolutionError, BranchSelectionResolver};
use crate::review::{ChangeComparison, SnapshotContentReader};
use crate::review::{PullRequestSelection, PullRequestSelectionResolver};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// 一个进程内 Review 的运行时表示；Reader 与 Snapshot identity 必须一起替换。
pub(super) struct OpenReviewRuntime {
    pub(super) snapshot_id: String,
    pub(super) reader: Arc<SnapshotContentReader>,
}

/// 在全局运行时锁外安全使用的 Open Review 只读快照。
pub(super) struct OpenReviewRuntimeSnapshot {
    pub(super) snapshot_id: String,
    pub(super) reader: Arc<SnapshotContentReader>,
}

/// 进程内所有已打开 Review 及唯一 Presented Review 的状态。
pub(super) struct RuntimeState {
    pub(super) open_reviews: HashMap<String, OpenReviewRuntime>,
    pub(super) presented_review_id: Option<String>,
    presentation_generation: u64,
}

/// 一次 Presented Review 变更的 CAS 令牌，用于失败复合操作安全恢复。
pub(super) struct PresentationTransition {
    target_review_id: String,
    previous_review_id: Option<String>,
    installed_generation: u64,
    pub(super) changed: bool,
}

/// 安装 Reader 时对 Presented Review 的处理方式。
pub(super) enum OpenReviewRuntimeUpdate {
    OpenAndPresent,
    ReplaceSnapshot,
}

/// Review 生命周期对外返回的稳定 Snapshot 投影。
pub struct ReviewPresentation {
    pub review_id: String,
    pub snapshot_id: String,
    pub snapshot: ChangeComparison,
    pub pull_request: Option<PullRequestMetadata>,
    #[allow(dead_code, reason = "保留非致命清理诊断，当前 IPC 尚未展示")]
    pub cleanup_warnings: Vec<String>,
}

/// 打开或创建 Review 时允许的业务输入。
pub enum PresentReview {
    Saved {
        review_id: String,
    },
    Branch {
        repository_id: i64,
        base_ref: String,
        head_ref: String,
    },
    PullRequest {
        repository_id: i64,
        selection: PullRequestSelection,
    },
}

/// 用户确认刷新时携带的精确 current 与 available revisions。
pub struct ConfirmedReviewUpdate {
    pub current_base_revision: String,
    pub current_head_revision: String,
    pub available_base_revision: String,
    pub available_head_revision: String,
}

/// 更新检查结果，只描述 current 与 available，不改变任何 current 状态。
#[derive(Debug)]
pub struct ReviewUpdate {
    pub update_available: bool,
    pub current_base_revision: String,
    pub current_head_revision: String,
    pub available_base_revision: String,
    pub available_head_revision: String,
}

/// Review 逻辑关闭后的 workspace 清理状态。
pub enum WorkspaceCleanup {
    NotNeeded,
    Complete,
    #[allow(
        dead_code,
        reason = "清理诊断由 Tauri adapter 映射，当前 IPC 仅暴露错误文本"
    )]
    Deferred(String),
}

/// 关闭结果明确区分逻辑关闭和可延后重试的物理清理。
pub struct CloseOutcome {
    #[allow(dead_code, reason = "关闭结果由未来显式关闭入口消费")]
    pub workspace_cleanup: WorkspaceCleanup,
}

/// 启动恢复报告；候选放弃和 workspace 清理互不阻塞。
pub struct RecoveryReport {
    #[allow(dead_code, reason = "启动恢复当前只执行副作用，报告保留供诊断")]
    pub abandoned_snapshot_ids: Vec<String>,
    #[allow(dead_code, reason = "启动恢复当前只执行副作用，报告保留供诊断")]
    pub cleanup_warnings: Vec<String>,
}

/// Review 生命周期失败；类别用于调用方决定恢复动作，文本保持既有用户契约。
#[derive(Debug)]
pub enum ReviewLifecycleError {
    RepositoryUnavailable(String),
    WorkspaceTrustRequired,
    ReviewUnavailable(String),
    Busy,
    StaleSnapshot,
    StaleCurrent,
    SelectionMismatch(String),
    Cancelled(String),
    Persistence(String),
    Git(String),
    WorkspaceTransition(String),
    ActivationRollback {
        activation: String,
        rollback: Option<String>,
    },
    ShuttingDown,
    RuntimeUnavailable,
    Operation(String),
}

impl ReviewLifecycleError {
    /// 将下层诊断包装为生命周期错误。
    fn new(message: impl Into<String>) -> Self {
        Self::Operation(message.into())
    }

    /// 将 Persistence 失败标记为持久化边界故障。
    fn persistence(error: impl std::fmt::Display) -> Self {
        Self::Persistence(error.to_string())
    }

    /// 将 Core Git 失败分类为取消或普通 Git 故障。
    fn git(error: GitCommandError) -> Self {
        if error.is_cancelled() {
            Self::Cancelled(error.to_string())
        } else {
            Self::Git(error.to_string())
        }
    }

    /// 保留 Pull Request resolver 已识别的协作取消语义。
    fn pull_request(error: PullRequestResolutionError) -> Self {
        match error {
            PullRequestResolutionError::Cancelled(message) => Self::Cancelled(message),
            error => Self::Operation(error.to_string()),
        }
    }

    /// 保留 Branch Selection resolver 已识别的取消语义。
    fn branch(error: BranchSelectionResolutionError) -> Self {
        if error.is_cancelled() {
            Self::Cancelled(error.to_string())
        } else {
            Self::Git(error.to_string())
        }
    }
}

impl std::fmt::Display for ReviewLifecycleError {
    /// 输出面向用户且可恢复的中文诊断。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RepositoryUnavailable(message)
            | Self::ReviewUnavailable(message)
            | Self::SelectionMismatch(message)
            | Self::Cancelled(message)
            | Self::Persistence(message)
            | Self::Git(message)
            | Self::WorkspaceTransition(message)
            | Self::Operation(message) => formatter.write_str(message),
            Self::WorkspaceTrustRequired => {
                formatter.write_str("尚未授予 Workspace Trust；请先阅读风险说明并确认授权。")
            }
            Self::Busy => formatter.write_str("此 Review 正在刷新；请等待当前操作完成。"),
            Self::StaleSnapshot => {
                formatter.write_str("Snapshot identity 与 Review 的 current Snapshot 不一致。")
            }
            Self::StaleCurrent => formatter.write_str("current Snapshot 已改变；请重新检查更新。"),
            Self::ActivationRollback {
                activation,
                rollback: None,
            } => formatter.write_str(activation),
            Self::ActivationRollback {
                activation,
                rollback: Some(rollback),
            } => write!(
                formatter,
                "{activation}；恢复旧 Review Workspace 失败：{rollback}"
            ),
            Self::ShuttingDown => formatter.write_str("应用正在退出，无法开始新的 Review 操作。"),
            Self::RuntimeUnavailable => formatter.write_str("Review 状态不可用。"),
        }
    }
}

impl std::error::Error for ReviewLifecycleError {}

/// 统一拥有 durable Review、current Snapshot、Workspace 与运行时读取句柄的一致性边界。
pub struct ReviewLifecycle {
    store: Arc<ReviewStore>,
    repository_application: Arc<RepositoryApplication>,
    branch_selections: BranchSelectionResolver,
    pull_requests: Arc<PullRequestSelectionResolver>,
    /// 全局运行时锁只保护内存状态，绝不跨越 Core Git 操作持有。
    runtime: Mutex<RuntimeState>,
    /// 每个 Review 的操作锁允许同一 Review 串行收敛，同时不阻塞其他 Review。
    review_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// 生命周期 gate 允许 Review 操作并发，但让 shutdown 等待全部操作安全收敛。
    operation_gate: tokio::sync::RwLock<()>,
    /// shutdown 开始后立即拒绝尚未进入 gate 的新操作。
    shutting_down: AtomicBool,
    active_refreshes: Mutex<HashSet<String>>,
}

impl ReviewLifecycle {
    /// 使用共享 Persistence 创建生命周期服务。
    #[allow(
        dead_code,
        reason = "保留匿名 Pull Request resolver 的便捷构造 interface"
    )]
    pub fn new(store: Arc<ReviewStore>) -> Self {
        Self::with_pull_request_resolver(
            store,
            Arc::new(PullRequestSelectionResolver::anonymous(
                "https://api.github.com",
            )),
        )
    }

    /// 使用显式 Pull Request Selection resolver 创建生命周期服务。
    #[allow(dead_code, reason = "保留显式 resolver 注入 interface")]
    pub fn with_pull_request_resolver(
        store: Arc<ReviewStore>,
        pull_requests: Arc<PullRequestSelectionResolver>,
    ) -> Self {
        let repository_application = Arc::new(RepositoryApplication::new(store.clone()));
        Self::with_repository_application(store, repository_application, pull_requests)
    }

    /// 使用生产环境共享的 Repository application 创建生命周期服务。
    pub fn with_repository_application(
        store: Arc<ReviewStore>,
        repository_application: Arc<RepositoryApplication>,
        pull_requests: Arc<PullRequestSelectionResolver>,
    ) -> Self {
        Self {
            store,
            repository_application,
            branch_selections: BranchSelectionResolver,
            pull_requests,
            runtime: Mutex::new(RuntimeState {
                open_reviews: HashMap::new(),
                presented_review_id: None,
                presentation_generation: 0,
            }),
            review_locks: Mutex::new(HashMap::new()),
            operation_gate: tokio::sync::RwLock::new(()),
            shutting_down: AtomicBool::new(false),
            active_refreshes: Mutex::new(HashSet::new()),
        }
    }

    /// 进入可与其他 Review 并发、但必须先于 shutdown 完成的生命周期操作。
    pub(super) async fn enter_operation(
        &self,
    ) -> Result<tokio::sync::RwLockReadGuard<'_, ()>, ReviewLifecycleError> {
        if self.shutting_down.load(Ordering::Acquire) {
            return Err(ReviewLifecycleError::ShuttingDown);
        }
        let guard = self.operation_gate.read().await;
        if self.shutting_down.load(Ordering::Acquire) {
            drop(guard);
            return Err(ReviewLifecycleError::ShuttingDown);
        }
        Ok(guard)
    }

    /// 查询 shutdown 是否已开始，供同步 close 与并发测试拒绝新工作。
    pub(super) fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Acquire)
    }

    /// 返回一个按 durable reviewId 复用的 Review 操作锁。
    pub(super) fn review_lock(
        &self,
        review_id: &str,
    ) -> Result<Arc<tokio::sync::Mutex<()>>, ReviewLifecycleError> {
        let mut locks = self
            .review_locks
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?;
        Ok(locks
            .entry(review_id.to_owned())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone())
    }

    /// 读取某个 Open Review 的具名快照；锁在返回后立即释放，避免跨越 Git 读操作。
    pub(super) fn open_review_runtime(
        &self,
        review_id: &str,
    ) -> Result<Option<OpenReviewRuntimeSnapshot>, ReviewLifecycleError> {
        let runtime = self
            .runtime
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?;
        Ok(runtime
            .open_reviews
            .get(review_id)
            .map(|review| OpenReviewRuntimeSnapshot {
                snapshot_id: review.snapshot_id.clone(),
                reader: review.reader.clone(),
            }))
    }

    /// 在一个短临界区中安装或替换 Open Review runtime，并按策略更新展示状态。
    pub(super) fn update_open_review_runtime(
        &self,
        review_id: String,
        snapshot_id: String,
        reader: Arc<SnapshotContentReader>,
        update: OpenReviewRuntimeUpdate,
    ) -> Result<Option<PresentationTransition>, ReviewLifecycleError> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?;
        if matches!(update, OpenReviewRuntimeUpdate::ReplaceSnapshot)
            && !runtime.open_reviews.contains_key(&review_id)
        {
            return Err(ReviewLifecycleError::ReviewUnavailable(
                "Review 已关闭或失效，请重新打开。".to_owned(),
            ));
        }
        runtime.open_reviews.insert(
            review_id.clone(),
            OpenReviewRuntime {
                snapshot_id,
                reader,
            },
        );
        match update {
            OpenReviewRuntimeUpdate::OpenAndPresent => {
                Ok(Some(Self::transition_presentation(&mut runtime, review_id)))
            }
            OpenReviewRuntimeUpdate::ReplaceSnapshot => Ok(None),
        }
    }

    /// 仅切换 Presented Review，不触碰任何 Workspace 或 Reader，并返回可恢复令牌。
    pub(super) fn present_open_review(
        &self,
        review_id: &str,
    ) -> Result<PresentationTransition, ReviewLifecycleError> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?;
        if !runtime.open_reviews.contains_key(review_id) {
            return Err(ReviewLifecycleError::RuntimeUnavailable);
        }
        Ok(Self::transition_presentation(
            &mut runtime,
            review_id.to_owned(),
        ))
    }

    /// 仅当展示 generation 仍匹配时回滚复合操作，避免覆盖任何较新的成功 present。
    pub(super) fn restore_presentation(
        &self,
        transition: PresentationTransition,
    ) -> Result<(), ReviewLifecycleError> {
        if !transition.changed {
            return Ok(());
        }
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?;
        if runtime.presentation_generation == transition.installed_generation
            && runtime.presented_review_id.as_deref() == Some(&transition.target_review_id)
        {
            runtime.presented_review_id = transition.previous_review_id;
            runtime.presentation_generation = runtime.presentation_generation.wrapping_add(1);
        }
        Ok(())
    }

    /// 原子记录一次真实 presentation 变更；重复呈现当前 Review 不推进 generation。
    fn transition_presentation(
        runtime: &mut RuntimeState,
        target_review_id: String,
    ) -> PresentationTransition {
        let previous_review_id = runtime.presented_review_id.clone();
        let changed = previous_review_id.as_deref() != Some(&target_review_id);
        if changed {
            runtime.presented_review_id = Some(target_review_id.clone());
            runtime.presentation_generation = runtime.presentation_generation.wrapping_add(1);
        }
        PresentationTransition {
            target_review_id,
            previous_review_id,
            installed_generation: runtime.presentation_generation,
            changed,
        }
    }

    /// 从运行时集合移除 Review，并在它是 Presented Review 时清空展示指针。
    pub(super) fn remove_runtime_review(
        &self,
        review_id: &str,
    ) -> Result<(), ReviewLifecycleError> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?;
        runtime.open_reviews.remove(review_id);
        if runtime.presented_review_id.as_deref() == Some(review_id) {
            runtime.presented_review_id = None;
            runtime.presentation_generation = runtime.presentation_generation.wrapping_add(1);
        }
        Ok(())
    }

    /// 释放所有进程内 Reader 与展示指针；Workspace 清理由调用方在锁外执行。
    pub(super) fn clear_runtime(&self) -> Result<(), ReviewLifecycleError> {
        let mut runtime = self
            .runtime
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?;
        runtime.open_reviews.clear();
        runtime.presented_review_id = None;
        runtime.presentation_generation = runtime.presentation_generation.wrapping_add(1);
        Ok(())
    }
}

mod presentation;
mod refresh;
mod workspace;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
