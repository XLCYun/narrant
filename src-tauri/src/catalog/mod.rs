use crate::core_git::{CancellationToken, GitProcessAdapter};
use crate::review::{ChangeStatus, LocalEntry, LocalFileChange, LocalSnapshot};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const CURRENT_SCHEMA_VERSION: i64 = 4;
const ENTRY_COLUMNS: &str = "id, canonical_path, common_directory, created_at, last_opened_at";

/// 已注册的具体本地 clone，以及当前是否仍可访问。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryEntry {
    pub id: i64,
    pub canonical_path: PathBuf,
    pub common_directory: PathBuf,
    pub created_at: i64,
    pub last_opened_at: i64,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

/// 首页与 Repository Entry 页面共享的持久化 Review 列表投影。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSummary {
    pub review_id: String,
    pub repository_entry_id: i64,
    pub repository_path: PathBuf,
    pub base_ref: String,
    pub head_ref: String,
    pub last_opened_at: i64,
    pub pull_request: Option<PullRequestMetadata>,
}

/// 清理 Review Workspace 前必须同时匹配的四项持久化所有权证据。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceCleanupTarget {
    pub review_id: String,
    pub repository_path: PathBuf,
    pub common_directory: PathBuf,
    pub workspace_path: PathBuf,
}

/// 持久化 Review 及其 current immutable Snapshot 的完整本地读取模型。
#[derive(Clone)]
pub struct SavedBranchReview {
    pub review_id: String,
    pub repository_entry_id: i64,
    pub base_ref: String,
    pub head_ref: String,
    pub base_revision: String,
    pub comparison_base: String,
    pub head_revision: String,
    pub file_changes: Vec<LocalFileChange>,
    pub created: bool,
    pub pull_request: Option<PullRequestMetadata>,
    snapshot_id: i64,
    expected_current_snapshot_id: Option<i64>,
}

impl SavedBranchReview {
    /// 返回可供 IPC 校验的 immutable Snapshot identity。
    pub fn snapshot_identity(&self) -> String {
        format!("snapshot-{}", self.snapshot_id)
    }

    /// 将数据库模型还原成 Repository Operations 可读取的不可变 Snapshot。
    pub fn snapshot(&self) -> LocalSnapshot {
        LocalSnapshot {
            base_ref: self.base_ref.clone(),
            head_ref: self.head_ref.clone(),
            base_revision: self.base_revision.clone(),
            comparison_base: self.comparison_base.clone(),
            head_revision: self.head_revision.clone(),
            file_changes: self.file_changes.clone(),
        }
    }
}

/// Pull Request Selection 随 Review 保存的协作 metadata，不包含代码内容。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestMetadata {
    pub owner: String,
    pub repository: String,
    pub number: u64,
    pub title: String,
    pub description: String,
    pub author: String,
    pub state: String,
    pub draft: bool,
    pub url: String,
    pub base_ref: String,
    pub head_ref: String,
    #[serde(default)]
    pub updated_at: Option<String>,
    pub synchronized_at: i64,
}

/// 经 Core Git 验证后的 working-tree repository 身份。
#[derive(Clone, Debug, PartialEq, Eq)]
struct InspectedRepository {
    canonical_path: PathBuf,
    common_directory: PathBuf,
}

/// Repository Catalog 的持久化或仓库验证错误。
#[derive(Debug)]
pub struct RepositoryCatalogError {
    message: String,
}

impl RepositoryCatalogError {
    /// 创建可直接显示给用户的 Catalog 错误。
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for RepositoryCatalogError {
    /// 输出稳定且可恢复的中文诊断。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RepositoryCatalogError {}

/// 应用级 SQLite Repository Catalog；所有写入通过事务原子完成。
pub struct RepositoryCatalog {
    connection: Mutex<Connection>,
    workspace_root: PathBuf,
}

impl std::fmt::Debug for RepositoryCatalog {
    /// 避免调试输出泄露数据库内部状态。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RepositoryCatalog")
            .finish_non_exhaustive()
    }
}

mod mapping;
mod migration;
mod queries;
mod repository_entries;
mod reviews;
mod snapshots;

use mapping::*;
use migration::migrate;

#[cfg(test)]
#[path = "tests/catalog/mod.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/integration.rs"]
mod integration_tests;
