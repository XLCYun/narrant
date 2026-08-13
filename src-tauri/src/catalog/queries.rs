use super::*;

impl RepositoryCatalog {
    /// 返回一个 Review 曾成功激活的 Snapshot identities，不暴露失败候选。
    #[cfg(test)]
    pub fn list_review_snapshot_identities(
        &self,
        review_id: &str,
    ) -> Result<Vec<String>, RepositoryCatalogError> {
        let row_id = parse_review_id(review_id)?;
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id FROM review_snapshots
                 WHERE review_id = ?1 AND activated_at IS NOT NULL
                 ORDER BY id",
            )
            .map_err(database_recovery_error)?;
        let snapshot_ids = statement
            .query_map([row_id], |row| row.get::<_, i64>(0))
            .map_err(database_recovery_error)?
            .map(|id| id.map(|id| format!("snapshot-{id}")))
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_recovery_error)?;
        Ok(snapshot_ids)
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
                        r.pull_request_metadata, s.base_revision,
                        s.comparison_base, s.head_revision
                 FROM reviews r
                 JOIN repository_entries e ON e.id = r.repository_entry_id
                 JOIN review_snapshots s ON s.id = r.current_snapshot_id
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
                    base_revision: row.get(7)?,
                    comparison_base: row.get(8)?,
                    head_revision: row.get(9)?,
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
    pub(super) fn connection(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Connection>, RepositoryCatalogError> {
        self.connection
            .lock()
            .map_err(|_| RepositoryCatalogError::new("Repository Catalog 状态不可用。"))
    }
}
