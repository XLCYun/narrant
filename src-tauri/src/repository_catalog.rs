use crate::change_content::{
    local::LocalEntry, local::LocalFileChange, local::LocalSnapshot, ChangeStatus,
};
use crate::core_git::{CancellationToken, GitProcessAdapter};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const CURRENT_SCHEMA_VERSION: i64 = 3;
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

impl RepositoryCatalog {
    /// 打开应用数据库并执行显式 schema migration。
    pub fn open(database_path: &Path) -> Result<Self, RepositoryCatalogError> {
        if let Some(parent) = database_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                RepositoryCatalogError::new(format!("无法创建 Repository Catalog 目录：{error}"))
            })?;
        }
        let mut connection = Connection::open(database_path)
            .and_then(|connection| {
                connection.execute_batch(
                    "PRAGMA foreign_keys = ON;
                     PRAGMA journal_mode = WAL;
                     PRAGMA synchronous = FULL;",
                )?;
                Ok(connection)
            })
            .map_err(database_recovery_error)?;
        migrate(&mut connection).map_err(database_recovery_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
            workspace_root: database_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("reviews"),
        })
    }

    /// 验证并注册一个具体 working-tree clone；重复添加同一路径返回原入口。
    pub fn register(
        &self,
        selected_path: &Path,
    ) -> Result<RepositoryEntry, RepositoryCatalogError> {
        let inspected = inspect_repository(selected_path)?;
        let now = unix_timestamp_millis()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(database_recovery_error)?;
        transaction
            .execute(
                "INSERT INTO repository_entries (
                    canonical_path, common_directory, created_at, last_opened_at
                 ) VALUES (?1, ?2, ?3, ?3)
                 ON CONFLICT(canonical_path) DO UPDATE SET
                    common_directory = excluded.common_directory,
                    last_opened_at = excluded.last_opened_at",
                params![
                    inspected.canonical_path.to_string_lossy(),
                    inspected.common_directory.to_string_lossy(),
                    now
                ],
            )
            .map_err(database_recovery_error)?;
        let entry_query =
            format!("SELECT {ENTRY_COLUMNS} FROM repository_entries WHERE canonical_path = ?1");
        let entry = transaction
            .query_row(
                &entry_query,
                [inspected.canonical_path.to_string_lossy()],
                saved_entry_from_row,
            )
            .map_err(database_recovery_error)?;
        transaction.commit().map_err(database_recovery_error)?;
        Ok(entry)
    }

    /// 按添加顺序倒序列出全部入口，打开活动不改变仓库位置。
    pub fn list(&self) -> Result<Vec<RepositoryEntry>, RepositoryCatalogError> {
        let connection = self.connection()?;
        let list_query = format!(
            "SELECT {ENTRY_COLUMNS} FROM repository_entries
             ORDER BY id DESC"
        );
        let mut statement = connection
            .prepare(&list_query)
            .map_err(database_recovery_error)?;
        let saved = statement
            .query_map([], saved_entry_from_row)
            .map_err(database_recovery_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_recovery_error)?;
        Ok(saved.into_iter().map(resolve_availability).collect())
    }

    /// 打开一个入口；可用时原子更新最后打开时间，不可用时返回历史入口。
    pub fn open_entry(&self, id: i64) -> Result<RepositoryEntry, RepositoryCatalogError> {
        let mut connection = self.connection()?;
        let entry_query = format!("SELECT {ENTRY_COLUMNS} FROM repository_entries WHERE id = ?1");
        let saved = connection
            .query_row(&entry_query, [id], saved_entry_from_row)
            .optional()
            .map_err(database_recovery_error)?
            .ok_or_else(|| RepositoryCatalogError::new("Repository Entry 不存在。"))?;
        let mut entry = resolve_availability(saved);
        if entry.available {
            let now = unix_timestamp_millis()?;
            let transaction = connection.transaction().map_err(database_recovery_error)?;
            transaction
                .execute(
                    "UPDATE repository_entries SET last_opened_at = ?1 WHERE id = ?2",
                    params![now, id],
                )
                .map_err(database_recovery_error)?;
            transaction.commit().map_err(database_recovery_error)?;
            entry.last_opened_at = now;
        }
        Ok(entry)
    }

    /// 返回 Repository Entry 是否已经获得持久化 Workspace Trust。
    pub fn is_workspace_trusted(&self, id: i64) -> Result<bool, RepositoryCatalogError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM workspace_trust WHERE repository_entry_id = ?1
                 )",
                [id],
                |row| row.get(0),
            )
            .map_err(database_recovery_error)
    }

    /// 持久化用户对一个 Repository Entry 授予的 Workspace Trust。
    pub fn grant_workspace_trust(&self, id: i64) -> Result<(), RepositoryCatalogError> {
        let now = unix_timestamp_millis()?;
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "INSERT INTO workspace_trust (repository_entry_id, granted_at)
                 SELECT id, ?2 FROM repository_entries WHERE id = ?1
                 ON CONFLICT(repository_entry_id) DO NOTHING",
                params![id, now],
            )
            .map_err(database_recovery_error)?;
        if changed == 0 {
            let exists: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM repository_entries WHERE id = ?1)",
                    [id],
                    |row| row.get(0),
                )
                .map_err(database_recovery_error)?;
            if !exists {
                return Err(RepositoryCatalogError::new("Repository Entry 不存在。"));
            }
        }
        Ok(())
    }

    /// 原子保存 Branch Selection、首个 Snapshot、File Changes 与 current 指针。
    ///
    /// 同一 Repository Entry 的相同 canonical branch pair 会返回既有 Review。
    pub fn save_branch_review(
        &self,
        repository_entry_id: i64,
        snapshot: &LocalSnapshot,
    ) -> Result<SavedBranchReview, RepositoryCatalogError> {
        if !self.is_workspace_trusted(repository_entry_id)? {
            return Err(RepositoryCatalogError::new(
                "尚未授予 Workspace Trust；请先阅读风险说明并确认授权。",
            ));
        }
        let now = unix_timestamp_millis()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(database_recovery_error)?;
        transaction
            .execute(
                "INSERT INTO reviews (
                    repository_entry_id, selection_kind, base_ref, head_ref,
                    current_snapshot_id, created_at, last_opened_at
                 ) VALUES (?1, 'branch', ?2, ?3, NULL, ?4, ?4)
                 ON CONFLICT(repository_entry_id, selection_kind, base_ref, head_ref)
                 DO UPDATE SET last_opened_at = excluded.last_opened_at",
                params![
                    repository_entry_id,
                    snapshot.base_ref,
                    snapshot.head_ref,
                    now
                ],
            )
            .map_err(database_recovery_error)?;
        let review_row_id: i64 = transaction
            .query_row(
                "SELECT id FROM reviews
                 WHERE repository_entry_id = ?1 AND selection_kind = 'branch'
                   AND base_ref = ?2 AND head_ref = ?3",
                params![repository_entry_id, snapshot.base_ref, snapshot.head_ref],
                |row| row.get(0),
            )
            .map_err(database_recovery_error)?;
        let current_snapshot_id: Option<i64> = transaction
            .query_row(
                "SELECT current_snapshot_id FROM reviews WHERE id = ?1",
                [review_row_id],
                |row| row.get(0),
            )
            .map_err(database_recovery_error)?;
        if current_snapshot_id.is_some() {
            transaction.commit().map_err(database_recovery_error)?;
            drop(connection);
            return self.open_review(&format!("review-{review_row_id}"));
        }
        transaction
            .execute(
                "INSERT INTO review_snapshots (
                    review_id, base_revision, comparison_base, head_revision, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    review_row_id,
                    snapshot.base_revision,
                    snapshot.comparison_base,
                    snapshot.head_revision,
                    now
                ],
            )
            .map_err(database_recovery_error)?;
        let snapshot_id = transaction.last_insert_rowid();
        insert_file_changes(&transaction, snapshot_id, &snapshot.file_changes)?;
        transaction.commit().map_err(database_recovery_error)?;
        Ok(SavedBranchReview {
            review_id: format!("review-{review_row_id}"),
            repository_entry_id,
            base_ref: snapshot.base_ref.clone(),
            head_ref: snapshot.head_ref.clone(),
            base_revision: snapshot.base_revision.clone(),
            comparison_base: snapshot.comparison_base.clone(),
            head_revision: snapshot.head_revision.clone(),
            file_changes: snapshot.file_changes.clone(),
            created: true,
            pull_request: None,
            snapshot_id,
            expected_current_snapshot_id: None,
        })
    }

    /// 保存由 base GitHub repository identity 与 PR number 唯一标识的 Review。
    pub fn save_pull_request_review(
        &self,
        repository_entry_id: i64,
        metadata: &PullRequestMetadata,
        snapshot: &LocalSnapshot,
    ) -> Result<SavedBranchReview, RepositoryCatalogError> {
        if !self.is_workspace_trusted(repository_entry_id)? {
            return Err(RepositoryCatalogError::new(
                "尚未授予 Workspace Trust；请先阅读风险说明并确认授权。",
            ));
        }
        let now = unix_timestamp_millis()?;
        let selection_repository = format!(
            "{}/{}",
            metadata.owner.to_ascii_lowercase(),
            metadata.repository.to_ascii_lowercase()
        );
        let selection_number = metadata.number.to_string();
        let metadata_json = serde_json::to_string(metadata).map_err(|error| {
            RepositoryCatalogError::new(format!("无法保存 Pull Request metadata：{error}"))
        })?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(database_recovery_error)?;
        transaction
            .execute(
                "INSERT INTO reviews (
                    repository_entry_id, selection_kind, base_ref, head_ref,
                    current_snapshot_id, created_at, last_opened_at, pull_request_metadata
                 ) VALUES (?1, 'pull_request', ?2, ?3, NULL, ?4, ?4, ?5)
                 ON CONFLICT(repository_entry_id, selection_kind, base_ref, head_ref)
                 DO UPDATE SET
                    last_opened_at = excluded.last_opened_at,
                    pull_request_metadata = excluded.pull_request_metadata",
                params![
                    repository_entry_id,
                    selection_repository,
                    selection_number,
                    now,
                    metadata_json
                ],
            )
            .map_err(database_recovery_error)?;
        let review_row_id: i64 = transaction
            .query_row(
                "SELECT id FROM reviews
                 WHERE repository_entry_id = ?1 AND selection_kind = 'pull_request'
                   AND base_ref = ?2 AND head_ref = ?3",
                params![repository_entry_id, selection_repository, selection_number],
                |row| row.get(0),
            )
            .map_err(database_recovery_error)?;
        let current_snapshot_id: Option<i64> = transaction
            .query_row(
                "SELECT current_snapshot_id FROM reviews WHERE id = ?1",
                [review_row_id],
                |row| row.get(0),
            )
            .map_err(database_recovery_error)?;
        if current_snapshot_id.is_some() {
            transaction.commit().map_err(database_recovery_error)?;
            drop(connection);
            return self.open_review(&format!("review-{review_row_id}"));
        }
        transaction
            .execute(
                "INSERT INTO review_snapshots (
                    review_id, base_revision, comparison_base, head_revision, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    review_row_id,
                    snapshot.base_revision,
                    snapshot.comparison_base,
                    snapshot.head_revision,
                    now
                ],
            )
            .map_err(database_recovery_error)?;
        let snapshot_id = transaction.last_insert_rowid();
        insert_file_changes(&transaction, snapshot_id, &snapshot.file_changes)?;
        transaction.commit().map_err(database_recovery_error)?;
        Ok(SavedBranchReview {
            review_id: format!("review-{review_row_id}"),
            repository_entry_id,
            base_ref: snapshot.base_ref.clone(),
            head_ref: snapshot.head_ref.clone(),
            base_revision: snapshot.base_revision.clone(),
            comparison_base: snapshot.comparison_base.clone(),
            head_revision: snapshot.head_revision.clone(),
            file_changes: snapshot.file_changes.clone(),
            created: true,
            pull_request: Some(metadata.clone()),
            snapshot_id,
            expected_current_snapshot_id: None,
        })
    }

    /// 查找相同 Pull Request Selection 已存在且可打开的 current Snapshot。
    pub fn find_pull_request_review(
        &self,
        repository_entry_id: i64,
        owner: &str,
        repository: &str,
        number: u64,
    ) -> Result<Option<SavedBranchReview>, RepositoryCatalogError> {
        let selection_repository = format!(
            "{}/{}",
            owner.to_ascii_lowercase(),
            repository.to_ascii_lowercase()
        );
        let connection = self.connection()?;
        let row_id = connection
            .query_row(
                "SELECT id FROM reviews
                 WHERE repository_entry_id = ?1 AND selection_kind = 'pull_request'
                   AND base_ref = ?2 AND head_ref = ?3
                   AND current_snapshot_id IS NOT NULL",
                params![
                    repository_entry_id,
                    selection_repository,
                    number.to_string()
                ],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(database_recovery_error)?;
        drop(connection);
        row_id
            .map(|id| self.open_review(&format!("review-{id}")))
            .transpose()
    }

    /// 列出一个 Repository Entry 的全部 current Reviews，供历史列表与重开入口使用。
    pub fn list_saved_review_snapshots(
        &self,
        repository_entry_id: i64,
    ) -> Result<Vec<SavedBranchReview>, RepositoryCatalogError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id FROM reviews
                 WHERE repository_entry_id = ?1 AND current_snapshot_id IS NOT NULL
                 ORDER BY last_opened_at DESC, id DESC",
            )
            .map_err(database_recovery_error)?;
        let review_ids = statement
            .query_map([repository_entry_id], |row| row.get::<_, i64>(0))
            .map_err(database_recovery_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_recovery_error)?;
        drop(statement);
        drop(connection);
        review_ids
            .into_iter()
            .map(|id| self.open_review(&format!("review-{id}")))
            .collect()
    }

    /// 暂存新的 immutable Snapshot；在 workspace 对齐前不改变 current 指针。
    pub fn stage_pull_request_snapshot(
        &self,
        current: &SavedBranchReview,
        metadata: &PullRequestMetadata,
        snapshot: &LocalSnapshot,
    ) -> Result<SavedBranchReview, RepositoryCatalogError> {
        let review_row_id = parse_review_id(&current.review_id)?;
        let now = unix_timestamp_millis()?;
        let metadata_json = serde_json::to_string(metadata).map_err(|error| {
            RepositoryCatalogError::new(format!("无法保存 Pull Request metadata：{error}"))
        })?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(database_recovery_error)?;
        transaction
            .execute(
                "INSERT INTO review_snapshots (
                    review_id, base_revision, comparison_base, head_revision, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    review_row_id,
                    snapshot.base_revision,
                    snapshot.comparison_base,
                    snapshot.head_revision,
                    now
                ],
            )
            .map_err(database_recovery_error)?;
        let snapshot_id = transaction.last_insert_rowid();
        insert_file_changes(&transaction, snapshot_id, &snapshot.file_changes)?;
        transaction
            .execute(
                "UPDATE reviews SET pull_request_metadata = ?1, last_opened_at = ?2
                 WHERE id = ?3 AND selection_kind = 'pull_request'",
                params![metadata_json, now, review_row_id],
            )
            .map_err(database_recovery_error)?;
        transaction.commit().map_err(database_recovery_error)?;
        Ok(SavedBranchReview {
            review_id: current.review_id.clone(),
            repository_entry_id: current.repository_entry_id,
            base_ref: snapshot.base_ref.clone(),
            head_ref: snapshot.head_ref.clone(),
            base_revision: snapshot.base_revision.clone(),
            comparison_base: snapshot.comparison_base.clone(),
            head_revision: snapshot.head_revision.clone(),
            file_changes: snapshot.file_changes.clone(),
            created: true,
            pull_request: Some(metadata.clone()),
            snapshot_id,
            expected_current_snapshot_id: Some(current.snapshot_id),
        })
    }

    /// 查找相同 Branch Selection 已存在且可打开的 current Snapshot。
    pub fn find_branch_review(
        &self,
        repository_entry_id: i64,
        base_ref: &str,
        head_ref: &str,
    ) -> Result<Option<SavedBranchReview>, RepositoryCatalogError> {
        let connection = self.connection()?;
        let row_id = connection
            .query_row(
                "SELECT id FROM reviews
                 WHERE repository_entry_id = ?1 AND selection_kind = 'branch'
                   AND base_ref = ?2 AND head_ref = ?3
                   AND current_snapshot_id IS NOT NULL",
                params![repository_entry_id, base_ref, head_ref],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(database_recovery_error)?;
        drop(connection);
        row_id
            .map(|id| self.open_review(&format!("review-{id}")))
            .transpose()
    }

    /// 在 workspace HEAD 已校验后原子推进 current Snapshot 指针。
    pub fn activate_snapshot(
        &self,
        review: &SavedBranchReview,
    ) -> Result<(), RepositoryCatalogError> {
        let row_id = parse_review_id(&review.review_id)?;
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "UPDATE reviews SET current_snapshot_id = ?1
                 WHERE id = ?2
                   AND (
                     current_snapshot_id = ?3
                     OR (current_snapshot_id IS NULL AND ?3 IS NULL)
                   )",
                params![
                    review.snapshot_id,
                    row_id,
                    review.expected_current_snapshot_id
                ],
            )
            .map_err(database_recovery_error)?;
        if changed != 1 {
            return Err(RepositoryCatalogError::new(
                "Review current Snapshot 已改变；请重新打开后重试。",
            ));
        }
        Ok(())
    }

    /// 保存明确刷新产生的新 immutable Snapshot，但暂不改变 current 指针。
    pub fn save_refresh_snapshot(
        &self,
        review_id: &str,
        snapshot: &LocalSnapshot,
    ) -> Result<SavedBranchReview, RepositoryCatalogError> {
        let row_id = parse_review_id(review_id)?;
        let current = self.open_review(review_id)?;
        if current.base_ref != snapshot.base_ref || current.head_ref != snapshot.head_ref {
            return Err(RepositoryCatalogError::new(
                "刷新结果与 Review Selection 不一致。",
            ));
        }
        let now = unix_timestamp_millis()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(database_recovery_error)?;
        transaction
            .execute(
                "INSERT INTO review_snapshots (
                    review_id, base_revision, comparison_base, head_revision, created_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    row_id,
                    snapshot.base_revision,
                    snapshot.comparison_base,
                    snapshot.head_revision,
                    now
                ],
            )
            .map_err(database_recovery_error)?;
        let snapshot_id = transaction.last_insert_rowid();
        insert_file_changes(&transaction, snapshot_id, &snapshot.file_changes)?;
        transaction.commit().map_err(database_recovery_error)?;
        Ok(SavedBranchReview {
            review_id: review_id.to_owned(),
            repository_entry_id: current.repository_entry_id,
            base_ref: snapshot.base_ref.clone(),
            head_ref: snapshot.head_ref.clone(),
            base_revision: snapshot.base_revision.clone(),
            comparison_base: snapshot.comparison_base.clone(),
            head_revision: snapshot.head_revision.clone(),
            file_changes: snapshot.file_changes.clone(),
            created: true,
            pull_request: current.pull_request,
            snapshot_id,
            expected_current_snapshot_id: Some(current.snapshot_id),
        })
    }

    /// 仅在 current 仍是用户确认的旧版本时原子推进刷新 Snapshot。
    pub fn activate_refresh_snapshot(
        &self,
        review: &SavedBranchReview,
        expected_current: &SavedBranchReview,
    ) -> Result<(), RepositoryCatalogError> {
        let row_id = parse_review_id(&review.review_id)?;
        if expected_current.review_id != review.review_id {
            return Err(RepositoryCatalogError::new(
                "刷新前后的 Review identity 不一致。",
            ));
        }
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "UPDATE reviews SET current_snapshot_id = ?1, last_opened_at = ?2
                 WHERE id = ?3 AND current_snapshot_id = ?4",
                params![
                    review.snapshot_id,
                    unix_timestamp_millis()?,
                    row_id,
                    expected_current.snapshot_id
                ],
            )
            .map_err(database_recovery_error)?;
        if changed != 1 {
            return Err(RepositoryCatalogError::new(
                "Review current Snapshot 已改变；请重新检查更新。",
            ));
        }
        Ok(())
    }

    /// 按 durable reviewId 读取 current Snapshot 及稳定 File Change metadata。
    pub fn open_review(
        &self,
        review_id: &str,
    ) -> Result<SavedBranchReview, RepositoryCatalogError> {
        let row_id = parse_review_id(review_id)?;
        let connection = self.connection()?;
        let review = connection
            .query_row(
                "SELECT r.repository_entry_id, r.selection_kind, r.base_ref, r.head_ref,
                        s.base_revision, s.comparison_base, s.head_revision, s.id,
                        r.pull_request_metadata
                 FROM reviews r
                 JOIN review_snapshots s ON s.id = r.current_snapshot_id
                 WHERE r.id = ?1",
                [row_id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, Option<String>>(8)?,
                    ))
                },
            )
            .optional()
            .map_err(database_recovery_error)?
            .ok_or_else(|| RepositoryCatalogError::new("Review 不存在或尚无 current Snapshot。"))?;
        let mut statement = connection
            .prepare(
                "SELECT path, old_path, status, before_mode, after_mode,
                        before_object_id, after_object_id
                 FROM file_changes WHERE snapshot_id = ?1 ORDER BY id",
            )
            .map_err(database_recovery_error)?;
        let file_changes = statement
            .query_map([review.7], |row| {
                let before_mode = row.get::<_, Option<u32>>(3)?;
                let after_mode = row.get::<_, Option<u32>>(4)?;
                let before_object_id = row.get::<_, Option<String>>(5)?;
                let after_object_id = row.get::<_, Option<String>>(6)?;
                Ok(LocalFileChange {
                    path: row.get(0)?,
                    old_path: row.get(1)?,
                    status: parse_status(&row.get::<_, String>(2)?)?,
                    before_mode,
                    after_mode,
                    before_entry: entry_from_metadata(&before_object_id, before_mode),
                    after_entry: entry_from_metadata(&after_object_id, after_mode),
                    before_object_id,
                    after_object_id,
                })
            })
            .map_err(database_recovery_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_recovery_error)?;
        let pull_request: Option<PullRequestMetadata> = review
            .8
            .map(|metadata| serde_json::from_str(&metadata))
            .transpose()
            .map_err(|_| RepositoryCatalogError::new("Pull Request metadata 已损坏。"))?;
        Ok(SavedBranchReview {
            review_id: format!("review-{row_id}"),
            repository_entry_id: review.0,
            base_ref: pull_request
                .as_ref()
                .map_or_else(|| review.2.clone(), |metadata| metadata.base_ref.clone()),
            head_ref: pull_request
                .as_ref()
                .map_or_else(|| review.3.clone(), |metadata| metadata.head_ref.clone()),
            base_revision: review.4,
            comparison_base: review.5,
            head_revision: review.6,
            file_changes,
            created: false,
            pull_request,
            snapshot_id: review.7,
            expected_current_snapshot_id: Some(review.7),
        })
    }

    /// 按最后打开时间列出一个 Repository Entry 的 Reviews 或全局最近 Reviews。
    pub fn list_reviews(
        &self,
        repository_entry_id: Option<i64>,
    ) -> Result<Vec<ReviewSummary>, RepositoryCatalogError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT r.id, r.repository_entry_id, e.canonical_path,
                        r.base_ref, r.head_ref, r.last_opened_at,
                        r.pull_request_metadata
                 FROM reviews r
                 JOIN repository_entries e ON e.id = r.repository_entry_id
                 WHERE r.current_snapshot_id IS NOT NULL
                   AND (?1 IS NULL OR r.repository_entry_id = ?1)
                 ORDER BY r.last_opened_at DESC, r.id DESC",
            )
            .map_err(database_recovery_error)?;
        let reviews = statement
            .query_map([repository_entry_id], |row| {
                let metadata = row.get::<_, Option<String>>(6)?;
                let pull_request = metadata
                    .map(|json| serde_json::from_str::<PullRequestMetadata>(&json))
                    .transpose()
                    .map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            6,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                Ok(ReviewSummary {
                    review_id: format!("review-{}", row.get::<_, i64>(0)?),
                    repository_entry_id: row.get(1)?,
                    repository_path: PathBuf::from(row.get::<_, String>(2)?),
                    base_ref: row.get(3)?,
                    head_ref: row.get(4)?,
                    last_opened_at: row.get(5)?,
                    pull_request,
                })
            })
            .map_err(database_recovery_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_recovery_error)?;
        Ok(reviews)
    }

    /// 更新 Review 的最近打开时间，供全局 recent projection 排序。
    pub fn mark_review_opened(&self, review_id: &str) -> Result<(), RepositoryCatalogError> {
        let row_id = parse_review_id(review_id)?;
        let now = unix_timestamp_millis()?;
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "UPDATE reviews SET last_opened_at = ?1 WHERE id = ?2",
                params![now, row_id],
            )
            .map_err(database_recovery_error)?;
        if changed != 1 {
            return Err(RepositoryCatalogError::new("Review 不存在。"));
        }
        Ok(())
    }

    /// 返回所有持久化 Review 的精确 workspace 清理证据。
    pub fn workspace_cleanup_targets(
        &self,
    ) -> Result<Vec<WorkspaceCleanupTarget>, RepositoryCatalogError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT r.id, e.canonical_path, e.common_directory
                 FROM reviews r
                 JOIN repository_entries e ON e.id = r.repository_entry_id
                 WHERE r.current_snapshot_id IS NOT NULL
                 ORDER BY r.id",
            )
            .map_err(database_recovery_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    PathBuf::from(row.get::<_, String>(1)?),
                    PathBuf::from(row.get::<_, String>(2)?),
                ))
            })
            .map_err(database_recovery_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_recovery_error)?;
        Ok(rows
            .into_iter()
            .map(|(id, repository_path, common_directory)| {
                let review_id = format!("review-{id}");
                WorkspaceCleanupTarget {
                    workspace_path: self.workspace_root.join(&review_id).join("repo"),
                    review_id,
                    repository_path,
                    common_directory,
                }
            })
            .collect())
    }

    /// 返回 durable Review 对应的精确 Narrant-managed checkout 路径。
    pub fn review_workspace_path(
        &self,
        review_id: &str,
    ) -> Result<PathBuf, RepositoryCatalogError> {
        parse_review_id(review_id)?;
        Ok(self.workspace_root.join(review_id).join("repo"))
    }

    /// 统一处理 Catalog mutex 状态，避免调用点重复泄露同步细节。
    fn connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>, RepositoryCatalogError> {
        self.connection
            .lock()
            .map_err(|_| RepositoryCatalogError::new("Repository Catalog 状态不可用。"))
    }
}

/// 执行第一版 schema migration，不在 SQLite 中创建任何 secret 字段。
fn migrate(connection: &mut Connection) -> rusqlite::Result<()> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if version == 0 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "CREATE TABLE repository_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                canonical_path TEXT NOT NULL UNIQUE,
                common_directory TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_opened_at INTEGER NOT NULL
             );
             PRAGMA user_version = 1;",
        )?;
        transaction.commit()?;
    }
    if version < 2 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "CREATE TABLE workspace_trust (
                repository_entry_id INTEGER PRIMARY KEY
                    REFERENCES repository_entries(id) ON DELETE CASCADE,
                granted_at INTEGER NOT NULL
             );
             CREATE TABLE reviews (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                repository_entry_id INTEGER NOT NULL
                    REFERENCES repository_entries(id) ON DELETE CASCADE,
                selection_kind TEXT NOT NULL CHECK(selection_kind IN ('branch', 'pull_request')),
                base_ref TEXT NOT NULL,
                head_ref TEXT NOT NULL,
                current_snapshot_id INTEGER,
                created_at INTEGER NOT NULL,
                last_opened_at INTEGER NOT NULL,
                UNIQUE(repository_entry_id, selection_kind, base_ref, head_ref)
             );
             CREATE TABLE review_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                review_id INTEGER NOT NULL REFERENCES reviews(id) ON DELETE CASCADE,
                base_revision TEXT NOT NULL,
                comparison_base TEXT NOT NULL,
                head_revision TEXT NOT NULL,
                created_at INTEGER NOT NULL
             );
             CREATE TABLE file_changes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                snapshot_id INTEGER NOT NULL
                    REFERENCES review_snapshots(id) ON DELETE CASCADE,
                path TEXT NOT NULL,
                old_path TEXT,
                status TEXT NOT NULL,
                before_mode INTEGER,
                after_mode INTEGER,
                before_object_id TEXT,
                after_object_id TEXT
             );
             PRAGMA user_version = 2;",
        )?;
        transaction.commit()?;
    }
    if version < 3 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "ALTER TABLE reviews ADD COLUMN pull_request_metadata TEXT;
             PRAGMA user_version = 3;",
        )?;
        transaction.commit()?;
    }
    Ok(())
}

/// 将一个 Snapshot 的稳定 File Change metadata 写入当前事务。
fn insert_file_changes(
    transaction: &rusqlite::Transaction<'_>,
    snapshot_id: i64,
    file_changes: &[LocalFileChange],
) -> Result<(), RepositoryCatalogError> {
    for change in file_changes {
        transaction
            .execute(
                "INSERT INTO file_changes (
                    snapshot_id, path, old_path, status, before_mode, after_mode,
                    before_object_id, after_object_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    snapshot_id,
                    change.path,
                    change.old_path,
                    status_text(change.status),
                    change.before_mode,
                    change.after_mode,
                    change.before_object_id,
                    change.after_object_id
                ],
            )
            .map_err(database_recovery_error)?;
    }
    Ok(())
}

/// 将 durable string identity 解析为数据库主键。
fn parse_review_id(review_id: &str) -> Result<i64, RepositoryCatalogError> {
    review_id
        .strip_prefix("review-")
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| RepositoryCatalogError::new("reviewId 格式无效。"))
}

/// 将 domain status 保存成稳定的数据库标签。
fn status_text(status: ChangeStatus) -> &'static str {
    match status {
        ChangeStatus::Added => "added",
        ChangeStatus::Modified => "modified",
        ChangeStatus::Deleted => "deleted",
        ChangeStatus::Renamed => "renamed",
    }
}

/// 从数据库状态标签恢复 domain status。
fn parse_status(status: &str) -> rusqlite::Result<ChangeStatus> {
    match status {
        "added" => Ok(ChangeStatus::Added),
        "modified" => Ok(ChangeStatus::Modified),
        "deleted" => Ok(ChangeStatus::Deleted),
        "renamed" => Ok(ChangeStatus::Renamed),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

/// 用持久化 object ID 与 mode 恢复按需读取条目。
fn entry_from_metadata(object_id: &Option<String>, mode: Option<u32>) -> Option<LocalEntry> {
    Some(LocalEntry {
        object_id: object_id.clone()?,
        mode: mode?,
    })
}

/// 从稳定的列顺序重建尚未探测可用性的入口。
fn saved_entry_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepositoryEntry> {
    Ok(RepositoryEntry {
        id: row.get(0)?,
        canonical_path: PathBuf::from(row.get::<_, String>(1)?),
        common_directory: PathBuf::from(row.get::<_, String>(2)?),
        created_at: row.get(3)?,
        last_opened_at: row.get(4)?,
        available: true,
        unavailable_reason: None,
    })
}

/// 用 Core Git 检查保存的路径，且不改变任何 repository 状态。
fn resolve_availability(mut entry: RepositoryEntry) -> RepositoryEntry {
    match inspect_repository(&entry.canonical_path) {
        Ok(inspected)
            if inspected.canonical_path == entry.canonical_path
                && inspected.common_directory == entry.common_directory => {}
        Ok(_) => {
            entry.available = false;
            entry.unavailable_reason = Some("保存的路径不再指向原 Repository Entry。".to_owned());
        }
        Err(error) => {
            entry.available = false;
            entry.unavailable_reason = Some(error.to_string());
        }
    }
    entry
}

/// 通过机器可读 Core Git 输出验证 working tree、canonical path、common directory 和能力。
fn inspect_repository(selected_path: &Path) -> Result<InspectedRepository, RepositoryCatalogError> {
    let selected_path = selected_path.canonicalize().map_err(|error| {
        RepositoryCatalogError::new(format!("Repository Entry 路径不存在或不可访问：{error}"))
    })?;
    let adapter = GitProcessAdapter::new(&selected_path);
    let cancellation = CancellationToken::default();
    let output = adapter
        .run(
            [
                "rev-parse",
                "--is-inside-work-tree",
                "--show-toplevel",
                "--git-common-dir",
                "--show-object-format",
            ],
            &cancellation,
        )
        .map_err(|error| {
            RepositoryCatalogError::new(format!(
                "所选路径不是兼容的 working-tree Git 仓库；请确认已安装支持所需能力的 Core Git：{error}"
            ))
        })?;
    let lines = String::from_utf8(output.stdout)
        .map_err(|_| RepositoryCatalogError::new("Core Git 返回了无效的仓库路径编码。"))?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if lines.len() != 4 || lines[0] != "true" || lines[3].is_empty() {
        return Err(RepositoryCatalogError::new(
            "所选路径不是兼容的 working-tree Git 仓库。",
        ));
    }
    let canonical_path = PathBuf::from(&lines[1]).canonicalize().map_err(|error| {
        RepositoryCatalogError::new(format!("无法解析 canonical repository path：{error}"))
    })?;
    let common_directory = {
        let path = PathBuf::from(&lines[2]);
        let path = if path.is_absolute() {
            path
        } else {
            canonical_path.join(path)
        };
        path.canonicalize().map_err(|error| {
            RepositoryCatalogError::new(format!("无法解析 Git common directory：{error}"))
        })?
    };
    probe_required_capabilities(&adapter, &cancellation)?;
    Ok(InspectedRepository {
        canonical_path,
        common_directory,
    })
}

/// 只读探测 Repository Entry 页面立即依赖的 refs、remotes 与 worktree 能力。
fn probe_required_capabilities(
    adapter: &GitProcessAdapter,
    cancellation: &CancellationToken,
) -> Result<(), RepositoryCatalogError> {
    for arguments in [
        vec![
            "for-each-ref",
            "--format=%(refname)%00",
            "refs/heads",
            "refs/remotes",
        ],
        vec!["remote"],
        vec!["worktree", "list", "--porcelain", "-z"],
    ] {
        adapter.run(arguments, cancellation).map_err(|error| {
            RepositoryCatalogError::new(format!(
                "Core Git 缺少 Repository Entry 所需的 refs、remotes 或 worktree 能力：{error}"
            ))
        })?;
    }
    Ok(())
}

/// 返回可排序且无需额外时间依赖的 Unix 毫秒时间。
fn unix_timestamp_millis() -> Result<i64, RepositoryCatalogError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| RepositoryCatalogError::new(format!("系统时间无效：{error}")))?;
    i64::try_from(duration.as_millis())
        .map_err(|_| RepositoryCatalogError::new("系统时间超出支持范围。"))
}

/// 将 SQLite 故障转换为明确的恢复指引。
fn database_recovery_error(error: rusqlite::Error) -> RepositoryCatalogError {
    RepositoryCatalogError::new(format!(
        "Repository Catalog 无法打开或迁移：{error}。请完整退出 Narrant，备份后移走或删除数据库以重建 Catalog。"
    ))
}

#[cfg(test)]
#[path = "repository_catalog_tests.rs"]
mod tests;
