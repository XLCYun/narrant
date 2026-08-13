use crate::catalog::{
    PullRequestMetadata, RepositoryCatalog, RepositoryCatalogError, RepositoryEntry, ReviewSummary,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Repository Entry Overview 中显式呈现的 Review Selection。
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
        pull_request: PullRequestMetadata,
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

/// Repository Entry 范围内可继续打开的一个 Review 摘要。
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
    pub repository_entry_id: i64,
    pub repository_path: PathBuf,
    pub review: RepositoryReviewSummary,
}

/// Repository Entry 页面需要的一次完整 Review projection。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryReviewsProjection {
    pub reviews: Vec<RepositoryReviewSummary>,
}

/// 首页需要的 Repository Catalog projection。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryCatalogProjection {
    pub repositories: Vec<RepositoryEntry>,
    pub recent_reviews: Vec<RecentReviewSummary>,
}

/// 一个 Repository Entry 的完整可进入上下文。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryEntryOverview {
    pub repository: RepositoryEntry,
    pub reviews: Vec<RepositoryReviewSummary>,
}

/// Repository 打开流程的可观察结果；Trust 与可用性判断留在模块内部。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RepositoryOpenResult {
    Unavailable { repository: RepositoryEntry },
    TrustRequired { repository: RepositoryEntry },
    Ready { overview: RepositoryEntryOverview },
}

/// Repository application 对外暴露的可恢复失败类别。
#[derive(Debug, PartialEq, Eq)]
pub enum RepositoryApplicationError {
    /// Repository Entry 当前无法解析为可用的 working tree。
    Unavailable(String),
    /// Repository 尚未获得执行 Git 操作所需的 Workspace Trust。
    WorkspaceTrustRequired,
    /// Catalog 持久化边界返回的错误。
    Catalog(String),
    /// 应用流程自身检测到的状态错误。
    Operation(String),
}

impl std::fmt::Display for RepositoryApplicationError {
    /// 输出稳定的 Repository application 诊断。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(message) | Self::Catalog(message) | Self::Operation(message) => {
                formatter.write_str(message)
            }
            Self::WorkspaceTrustRequired => {
                formatter.write_str("尚未授予 Workspace Trust；请先阅读风险说明并确认授权。")
            }
        }
    }
}

impl std::error::Error for RepositoryApplicationError {}

impl From<RepositoryCatalogError> for RepositoryApplicationError {
    /// 将 Catalog storage/validation 错误收敛到 application seam。
    fn from(error: RepositoryCatalogError) -> Self {
        Self::Catalog(error.to_string())
    }
}

impl RepositoryApplicationError {
    /// 创建 application 流程自身的可恢复错误。
    fn operation(message: impl Into<String>) -> Self {
        Self::Operation(message.into())
    }
}

/// 集中 Repository Entry availability、Workspace Trust 与 Review projection。
pub struct RepositoryEntryApplication {
    catalog: Arc<RepositoryCatalog>,
}

impl RepositoryEntryApplication {
    /// 使用与 Review lifecycle 共享的 Catalog 创建应用模块。
    pub fn new(catalog: Arc<RepositoryCatalog>) -> Self {
        Self { catalog }
    }

    /// 返回首页所需的 Repository 与全局最近 Review projection。
    pub fn load_catalog(&self) -> Result<RepositoryCatalogProjection, RepositoryApplicationError> {
        Ok(RepositoryCatalogProjection {
            repositories: self.catalog.list()?,
            recent_reviews: self
                .catalog
                .list_reviews(None)?
                .into_iter()
                .map(recent_review_summary)
                .collect(),
        })
    }

    /// 打开指定 Repository，并在呈现 Overview 前完成 Trust 与 Review projection。
    pub fn open_overview(
        &self,
        repository_entry_id: i64,
    ) -> Result<RepositoryOpenResult, RepositoryApplicationError> {
        let repository = self.catalog.open_entry(repository_entry_id)?;
        if !repository.available {
            return Ok(RepositoryOpenResult::Unavailable { repository });
        }
        if !self.catalog.is_workspace_trusted(repository_entry_id)? {
            return Ok(RepositoryOpenResult::TrustRequired { repository });
        }
        Ok(RepositoryOpenResult::Ready {
            overview: self.overview(repository)?,
        })
    }

    /// 持久化 Trust 后返回可直接呈现的完整 Repository Entry Overview。
    pub fn grant_trust_and_open(
        &self,
        repository_entry_id: i64,
    ) -> Result<RepositoryEntryOverview, RepositoryApplicationError> {
        self.catalog.grant_workspace_trust(repository_entry_id)?;
        let result = self.open_overview(repository_entry_id)?;
        match result {
            RepositoryOpenResult::Ready { overview } => Ok(overview),
            RepositoryOpenResult::Unavailable { repository } => {
                Err(RepositoryApplicationError::Unavailable(
                    repository
                        .unavailable_reason
                        .unwrap_or_else(|| "Repository Entry 当前不可用。".to_owned()),
                ))
            }
            RepositoryOpenResult::TrustRequired { .. } => Err(
                RepositoryApplicationError::operation("Workspace Trust 未能生效，请重试。"),
            ),
        }
    }

    /// 读取当前 Repository 的统一 Review projection，并拒绝不可用入口。
    pub fn load_reviews(
        &self,
        repository_entry_id: i64,
    ) -> Result<RepositoryReviewsProjection, RepositoryApplicationError> {
        self.require_available_path(repository_entry_id)?;
        self.review_projection(repository_entry_id)
    }

    /// 注册并验证一个具体 working-tree clone。
    pub fn register(&self, path: &Path) -> Result<RepositoryEntry, RepositoryApplicationError> {
        Ok(self.catalog.register(path)?)
    }

    /// 为需要 Core Git 或 GitHub discovery 的 command 解析可用 canonical path。
    pub fn require_available_path(
        &self,
        repository_entry_id: i64,
    ) -> Result<PathBuf, RepositoryApplicationError> {
        let repository = self.catalog.inspect_entry(repository_entry_id)?;
        if repository.available {
            Ok(repository.canonical_path)
        } else {
            Err(RepositoryApplicationError::Unavailable(
                repository
                    .unavailable_reason
                    .unwrap_or_else(|| "Repository Entry 当前不可用。".to_owned()),
            ))
        }
    }

    /// 返回已经通过 availability 与 Workspace Trust gate 的 canonical path。
    pub fn require_trusted_path(
        &self,
        repository_entry_id: i64,
    ) -> Result<PathBuf, RepositoryApplicationError> {
        let repository = self.require_available_path(repository_entry_id)?;
        if !self.catalog.is_workspace_trusted(repository_entry_id)? {
            return Err(RepositoryApplicationError::WorkspaceTrustRequired);
        }
        Ok(repository)
    }

    /// 读取 Repository Entry 的最新 projection，不改变最近打开时间。
    pub fn inspect_entry(
        &self,
        repository_entry_id: i64,
    ) -> Result<RepositoryEntry, RepositoryApplicationError> {
        Ok(self.catalog.inspect_entry(repository_entry_id)?)
    }

    /// 从已有 Repository projection 组合 Repository Entry Overview。
    fn overview(
        &self,
        repository: RepositoryEntry,
    ) -> Result<RepositoryEntryOverview, RepositoryApplicationError> {
        let projection = self.review_projection(repository.id)?;
        Ok(RepositoryEntryOverview {
            repository,
            reviews: projection.reviews,
        })
    }

    /// 一次读取页面所需的统一 Review summaries。
    fn review_projection(
        &self,
        repository_entry_id: i64,
    ) -> Result<RepositoryReviewsProjection, RepositoryApplicationError> {
        let reviews = self
            .catalog
            .list_reviews(Some(repository_entry_id))?
            .into_iter()
            .map(repository_review_summary)
            .collect();
        Ok(RepositoryReviewsProjection { reviews })
    }
}

/// 将 Catalog 的统一持久化投影转换成 Repository Entry Overview 摘要。
fn repository_review_summary(review: ReviewSummary) -> RepositoryReviewSummary {
    let selection = match review.pull_request {
        Some(pull_request) => ReviewSelectionSummary::PullRequest {
            base_ref: review.base_ref,
            head_ref: review.head_ref,
            pull_request,
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
        last_opened_at: review.last_opened_at,
    }
}

/// 为全局最近列表补充 Review 的 Repository 所有权。
fn recent_review_summary(summary: ReviewSummary) -> RecentReviewSummary {
    RecentReviewSummary {
        repository_entry_id: summary.repository_entry_id,
        repository_path: summary.repository_path.clone(),
        review: repository_review_summary(summary),
    }
}

#[cfg(test)]
#[path = "application_tests.rs"]
mod tests;
