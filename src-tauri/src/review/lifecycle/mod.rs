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
use std::sync::{Arc, Mutex};

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
    Deferred(String),
}

/// 关闭结果明确区分逻辑关闭和可延后重试的物理清理。
pub struct CloseOutcome {
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
    reviews: Mutex<HashMap<String, Arc<SnapshotContentReader>>>,
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
            reviews: Mutex::new(HashMap::new()),
            active_refreshes: Mutex::new(HashSet::new()),
        }
    }
}

mod presentation;
mod refresh;
mod workspace;

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
