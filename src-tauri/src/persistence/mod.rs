use crate::review::{
    ChangeComparison, FileChange, FileChangeStatus, FileContentRef, PullRequestMetadata,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const CURRENT_SCHEMA_VERSION: i64 = 5;
const REPOSITORY_COLUMNS: &str =
    "id, canonical_path, git_common_directory, created_at_millis, last_opened_at_millis";

/// 注册到 Narrant 并拥有 Review 历史的具体本地 Git 仓库。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Repository {
    pub id: i64,
    pub canonical_path: PathBuf,
    pub git_common_directory: PathBuf,
    pub created_at_millis: i64,
    pub last_opened_at_millis: i64,
}

/// application 层一次查询所需的持久化 Review 与 current Snapshot 投影。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSummary {
    pub review_id: String,
    pub repository_id: i64,
    pub repository_path: PathBuf,
    pub base_ref: String,
    pub head_ref: String,
    pub last_opened_at_millis: i64,
    pub pull_request: Option<PullRequestMetadata>,
    pub base_revision: String,
    pub comparison_base: String,
    pub head_revision: String,
}

/// 清理 Review Workspace 前必须同时匹配的四项持久化所有权证据。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewWorkspaceCleanupTarget {
    pub review_id: String,
    pub repository_path: PathBuf,
    pub git_common_directory: PathBuf,
    pub workspace_path: PathBuf,
}

/// 持久化 Review 及其 current immutable Snapshot 的完整本地读取模型。
#[derive(Clone)]
pub struct PersistedReviewState {
    pub review_id: String,
    pub repository_id: i64,
    pub base_ref: String,
    pub head_ref: String,
    pub base_revision: String,
    pub comparison_base: String,
    pub head_revision: String,
    pub file_changes: Vec<FileChange>,
    pub created: bool,
    pub pull_request: Option<PullRequestMetadata>,
    snapshot_id: i64,
}

impl PersistedReviewState {
    /// 返回可供 IPC 校验的 immutable Snapshot identity。
    pub fn snapshot_identity(&self) -> String {
        format!("snapshot-{}", self.snapshot_id)
    }

    /// 将数据库模型还原成 Repository Operations 可读取的不可变 Snapshot。
    pub fn to_change_comparison(&self) -> ChangeComparison {
        ChangeComparison {
            base_ref: self.base_ref.clone(),
            head_ref: self.head_ref.clone(),
            base_revision: self.base_revision.clone(),
            comparison_base: self.comparison_base.clone(),
            head_revision: self.head_revision.clone(),
            file_changes: self.file_changes.clone(),
        }
    }
}

/// Repository Application 写入持久化边界的稳定仓库位置。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RepositoryLocation {
    pub(crate) canonical_path: PathBuf,
    pub(crate) git_common_directory: PathBuf,
}

/// Persistence 的持久化或仓库验证错误。
#[derive(Debug)]
pub struct PersistenceError {
    message: String,
}

impl PersistenceError {
    /// 创建可直接显示给用户的 Persistence 错误。
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for PersistenceError {
    /// 输出稳定且可恢复的中文诊断。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for PersistenceError {}

/// 应用级 SQLite Persistence；所有写入通过事务原子完成。
pub struct ReviewStore {
    connection: Mutex<Connection>,
    workspace_root: PathBuf,
}

impl std::fmt::Debug for ReviewStore {
    /// 避免调试输出泄露数据库内部状态。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ReviewStore")
            .finish_non_exhaustive()
    }
}

mod mapping;
mod migration;
mod queries;
mod repositories;
mod review_snapshots;
mod reviews;

use mapping::*;
use migration::migrate;

#[cfg(test)]
#[path = "tests/persistence/mod.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/integration.rs"]
mod integration_tests;
