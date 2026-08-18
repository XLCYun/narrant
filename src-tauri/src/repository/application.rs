use super::verify_repository_location;
use crate::persistence::{PersistenceError, Repository, ReviewStore, ReviewSummary};
use crate::review::PullRequestMetadata;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Repository Overview 中显式呈现的 Review Selection。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ReviewSelectionSummary {
    Branch {
        base_ref: String,
        head_ref: String,
    },
    PullRequest {
        base_ref: String,
        head_ref: String,
        pull_request: Box<PullRequestMetadata>,
    },
}

/// Review 当前 Snapshot 的完整 revision identity。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSnapshotSummary {
    pub base_revision: String,
    pub comparison_base: String,
    pub head_revision: String,
}

/// Repository 范围内可继续打开的一个 Review 摘要。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryReviewSummary {
    pub review_id: String,
    pub selection: ReviewSelectionSummary,
    pub current_snapshot: ReviewSnapshotSummary,
    pub last_opened_at: i64,
}

/// 首页跨 Repository 聚合的最近 Review 摘要。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecentReviewSummary {
    pub repository_id: i64,
    pub repository_path: PathBuf,
    pub review: RepositoryReviewSummary,
}

/// Repository 页面需要的一次完整 Review projection。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryReviewList {
    pub reviews: Vec<RepositoryReviewSummary>,
}

/// 首页需要的 Persistence projection。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryNavigation {
    pub repositories: Vec<RepositorySummary>,
    pub recent_reviews: Vec<RecentReviewSummary>,
}

/// 一个 Repository 的完整可进入上下文。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryOverview {
    pub repository: RepositorySummary,
    pub reviews: Vec<RepositoryReviewSummary>,
}

/// Repository 打开流程的可观察结果；Trust 与可用性判断留在模块内部。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RepositoryOpenResult {
    Unavailable { repository: RepositorySummary },
    TrustRequired { repository: RepositorySummary },
    Ready { overview: RepositoryOverview },
}

/// Repository 当前是否仍解析到最初登记的本地 Git 仓库。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RepositoryAvailability {
    Available,
    Unavailable { reason: String },
}

/// Repository 及其由 Application 层解析的当前可用性。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositorySummary {
    #[serde(flatten)]
    pub repository: Repository,
    pub availability: RepositoryAvailability,
}

/// Repository application 对外暴露的可恢复失败类别。
#[derive(Debug, PartialEq, Eq)]
pub enum RepositoryApplicationError {
    /// Repository 当前无法解析为可用的 working tree。
    Unavailable(String),
    /// Repository 尚未获得执行 Git 操作所需的 Workspace Trust。
    WorkspaceTrustRequired,
    /// Persistence 持久化边界返回的错误。
    Persistence(String),
    /// 应用流程自身检测到的状态错误。
    Operation(String),
}

impl std::fmt::Display for RepositoryApplicationError {
    /// 输出稳定的 Repository application 诊断。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) | Self::Persistence(message) | Self::Operation(message) => {
                formatter.write_str(message)
            }
            Self::WorkspaceTrustRequired => {
                formatter.write_str("尚未授予 Workspace Trust；请先阅读风险说明并确认授权。")
            }
        }
    }
}

impl std::error::Error for RepositoryApplicationError {}

impl From<PersistenceError> for RepositoryApplicationError {
    /// 将 Persistence storage/validation 错误收敛到 application seam。
    fn from(error: PersistenceError) -> Self {
        Self::Persistence(error.to_string())
    }
}

impl RepositoryApplicationError {
    /// 创建 application 流程自身的可恢复错误。
    fn operation(message: impl Into<String>) -> Self {
        Self::Operation(message.into())
    }
}

/// 集中 Repository availability、Workspace Trust 与 Review projection。
pub struct RepositoryApplication {
    store: Arc<ReviewStore>,
}

impl RepositoryApplication {
    /// 使用与 Review lifecycle 共享的 Persistence 创建应用模块。
    pub fn new(store: Arc<ReviewStore>) -> Self {
        Self { store }
    }

    /// 返回首页所需的 Repository 与全局最近 Review projection。
    pub fn load_navigation(&self) -> Result<RepositoryNavigation, RepositoryApplicationError> {
        Ok(RepositoryNavigation {
            repositories: self
                .store
                .list_repositories()?
                .into_iter()
                .map(repository_summary)
                .collect(),
            recent_reviews: self
                .store
                .list_reviews(None)?
                .into_iter()
                .map(recent_review_summary)
                .collect(),
        })
    }

    /// 打开指定 Repository，并在呈现 Overview 前完成 Trust 与 Review projection。
    pub fn open_overview(
        &self,
        repository_id: i64,
    ) -> Result<RepositoryOpenResult, RepositoryApplicationError> {
        let repository = repository_summary(self.store.get_repository(repository_id)?);
        if matches!(
            repository.availability,
            RepositoryAvailability::Unavailable { .. }
        ) {
            return Ok(RepositoryOpenResult::Unavailable { repository });
        }
        if !self.store.has_workspace_trust(repository_id)? {
            return Ok(RepositoryOpenResult::TrustRequired { repository });
        }
        self.store.record_repository_opened(repository_id)?;
        Ok(RepositoryOpenResult::Ready {
            overview: self.overview(repository)?,
        })
    }

    /// 持久化 Trust 后返回可直接呈现的完整 Repository Overview。
    pub fn grant_trust_and_open(
        &self,
        repository_id: i64,
    ) -> Result<RepositoryOverview, RepositoryApplicationError> {
        self.store.insert_workspace_trust(repository_id)?;
        let result = self.open_overview(repository_id)?;
        match result {
            RepositoryOpenResult::Ready { overview } => Ok(overview),
            RepositoryOpenResult::Unavailable { repository } => Err(
                RepositoryApplicationError::Unavailable(unavailable_reason(&repository)),
            ),
            RepositoryOpenResult::TrustRequired { .. } => Err(
                RepositoryApplicationError::operation("Workspace Trust 未能生效，请重试。"),
            ),
        }
    }

    /// 读取当前 Repository 的统一 Review projection，并拒绝不可用入口。
    pub fn load_reviews(
        &self,
        repository_id: i64,
    ) -> Result<RepositoryReviewList, RepositoryApplicationError> {
        self.require_available_path(repository_id)?;
        self.review_projection(repository_id)
    }

    /// 注册并验证一个具体 working-tree clone。
    pub fn register_repository(
        &self,
        path: &Path,
    ) -> Result<RepositorySummary, RepositoryApplicationError> {
        let location = verify_repository_location(path)?;
        Ok(repository_summary(self.store.upsert_repository(&location)?))
    }

    /// 为需要 Core Git 或 GitHub discovery 的 command 解析可用 canonical path。
    pub fn require_available_path(
        &self,
        repository_id: i64,
    ) -> Result<PathBuf, RepositoryApplicationError> {
        let summary = repository_summary(self.store.get_repository(repository_id)?);
        match summary.availability {
            RepositoryAvailability::Available => Ok(summary.repository.canonical_path),
            RepositoryAvailability::Unavailable { reason } => {
                Err(RepositoryApplicationError::Unavailable(reason))
            }
        }
    }

    /// 返回已经通过 availability 与 Workspace Trust gate 的 canonical path。
    pub fn require_trusted_path(
        &self,
        repository_id: i64,
    ) -> Result<PathBuf, RepositoryApplicationError> {
        let repository = self.require_available_path(repository_id)?;
        if !self.store.has_workspace_trust(repository_id)? {
            return Err(RepositoryApplicationError::WorkspaceTrustRequired);
        }
        Ok(repository)
    }

    /// 读取 Repository 的最新 projection，不改变最近打开时间。
    pub fn get_repository(
        &self,
        repository_id: i64,
    ) -> Result<Repository, RepositoryApplicationError> {
        Ok(self.store.get_repository(repository_id)?)
    }

    /// 从已有 Repository projection 组合 Repository Overview。
    fn overview(
        &self,
        repository: RepositorySummary,
    ) -> Result<RepositoryOverview, RepositoryApplicationError> {
        let projection = self.review_projection(repository.repository.id)?;
        Ok(RepositoryOverview {
            repository,
            reviews: projection.reviews,
        })
    }

    /// 一次读取页面所需的统一 Review summaries。
    fn review_projection(
        &self,
        repository_id: i64,
    ) -> Result<RepositoryReviewList, RepositoryApplicationError> {
        let reviews = self
            .store
            .list_reviews(Some(repository_id))?
            .into_iter()
            .map(repository_review_summary)
            .collect();
        Ok(RepositoryReviewList { reviews })
    }
}

/// 将 Persistence 的统一持久化投影转换成 Repository Overview 摘要。
fn repository_review_summary(review: ReviewSummary) -> RepositoryReviewSummary {
    let selection = match review.pull_request {
        Some(pull_request) => ReviewSelectionSummary::PullRequest {
            base_ref: review.base_ref,
            head_ref: review.head_ref,
            pull_request: Box::new(pull_request),
        },
        None => ReviewSelectionSummary::Branch {
            base_ref: review.base_ref,
            head_ref: review.head_ref,
        },
    };
    RepositoryReviewSummary {
        review_id: review.review_id,
        selection,
        current_snapshot: ReviewSnapshotSummary {
            base_revision: review.base_revision,
            comparison_base: review.comparison_base,
            head_revision: review.head_revision,
        },
        last_opened_at: review.last_opened_at_millis,
    }
}

/// 为全局最近列表补充 Review 的 Repository 所有权。
fn recent_review_summary(summary: ReviewSummary) -> RecentReviewSummary {
    RecentReviewSummary {
        repository_id: summary.repository_id,
        repository_path: summary.repository_path.clone(),
        review: repository_review_summary(summary),
    }
}

/// 使用 Core Git 验证持久路径并生成当前 Repository 可用性。
fn repository_summary(repository: Repository) -> RepositorySummary {
    let availability = match verify_repository_location(&repository.canonical_path) {
        Ok(location)
            if location.canonical_path == repository.canonical_path
                && location.git_common_directory == repository.git_common_directory =>
        {
            RepositoryAvailability::Available
        }
        Ok(_) => RepositoryAvailability::Unavailable {
            reason: "保存的路径不再指向原 Repository。".to_owned(),
        },
        Err(error) => RepositoryAvailability::Unavailable {
            reason: error.to_string(),
        },
    };
    RepositorySummary {
        repository,
        availability,
    }
}

/// 从不可用 Repository 投影中提取稳定诊断。
fn unavailable_reason(repository: &RepositorySummary) -> String {
    match &repository.availability {
        RepositoryAvailability::Unavailable { reason } => reason.clone(),
        RepositoryAvailability::Available => "Repository 当前可用。".to_owned(),
    }
}

#[cfg(test)]
#[path = "application_tests.rs"]
mod tests;
