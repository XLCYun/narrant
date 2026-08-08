use crate::repository_catalog::{
    PullRequestMetadata, RepositoryCatalog, RepositoryCatalogError, RepositoryEntry, ReviewSummary,
    SavedBranchReview,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// 无需打开 Review 即可展示的持久化 Snapshot 摘要。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedReviewSummary {
    pub review_id: String,
    pub base_revision: String,
    pub head_revision: String,
    pub pull_request: Option<PullRequestMetadata>,
}

/// Repository Entry 页面需要的一次完整 Review projection。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryReviewsProjection {
    pub saved_reviews: Vec<SavedReviewSummary>,
    pub reviews: Vec<ReviewSummary>,
}

/// 首页需要的 Repository Catalog projection。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryCatalogProjection {
    pub repositories: Vec<RepositoryEntry>,
    pub recent_reviews: Vec<ReviewSummary>,
}

/// 一个 Repository Entry 的完整可进入上下文。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryContext {
    pub repository: RepositoryEntry,
    pub saved_reviews: Vec<SavedReviewSummary>,
    pub reviews: Vec<ReviewSummary>,
}

/// Repository 打开流程的可观察结果；Trust 与可用性判断留在模块内部。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum RepositoryOpenResult {
    Unavailable { repository: RepositoryEntry },
    TrustRequired { repository: RepositoryEntry },
    Ready { context: RepositoryContext },
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
            recent_reviews: self.catalog.list_reviews(None)?,
        })
    }

    /// 打开指定 Repository，并在进入可用 context 前完成 Trust 与 Review projection。
    pub fn open_context(
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
            context: self.context(repository)?,
        })
    }

    /// 持久化 Trust 后返回可直接呈现的完整 Repository context。
    pub fn grant_trust_and_open(
        &self,
        repository_entry_id: i64,
    ) -> Result<RepositoryContext, RepositoryApplicationError> {
        self.catalog.grant_workspace_trust(repository_entry_id)?;
        let result = self.open_context(repository_entry_id)?;
        match result {
            RepositoryOpenResult::Ready { context } => Ok(context),
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

    /// 读取当前 Repository 的两个 Review projection，并拒绝不可用入口。
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

    /// 从已有 Repository projection 组合 application context。
    fn context(
        &self,
        repository: RepositoryEntry,
    ) -> Result<RepositoryContext, RepositoryApplicationError> {
        let projection = self.review_projection(repository.id)?;
        Ok(RepositoryContext {
            repository,
            saved_reviews: projection.saved_reviews,
            reviews: projection.reviews,
        })
    }

    /// 读取页面同时需要的 saved Snapshot 与 Review summary。
    fn review_projection(
        &self,
        repository_entry_id: i64,
    ) -> Result<RepositoryReviewsProjection, RepositoryApplicationError> {
        let saved_reviews = self
            .catalog
            .list_saved_review_snapshots(repository_entry_id)?
            .into_iter()
            .map(saved_review_summary)
            .collect();
        Ok(RepositoryReviewsProjection {
            saved_reviews,
            reviews: self.catalog.list_reviews(Some(repository_entry_id))?,
        })
    }
}

/// 将 Catalog 的完整 Review 模型压缩成 Repository 页面所需的摘要。
fn saved_review_summary(review: SavedBranchReview) -> SavedReviewSummary {
    SavedReviewSummary {
        review_id: review.review_id,
        base_revision: review.base_revision,
        head_revision: review.head_revision,
        pull_request: review.pull_request,
    }
}

#[cfg(test)]
#[path = "repository_application_tests.rs"]
mod tests;
