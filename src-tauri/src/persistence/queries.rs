use super::*;

impl ReviewStore {
    /// 只读取已激活 current Snapshot 的身份，供后台工作频繁校验且不加载 File Changes。
    pub fn current_snapshot_identity(&self, review_id: &str) -> Result<String, PersistenceError> {
        let row_id = parse_review_id(review_id)?;
        let connection = self.lock_connection()?;
        connection
            .query_row(
                "SELECT s.id FROM reviews r
                 JOIN review_snapshots s ON s.id = r.current_snapshot_id
                 WHERE r.id = ?1 AND s.activated_at_millis IS NOT NULL",
                [row_id],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(map_sqlite_error)?
            .map(|id| format!("snapshot-{id}"))
            .ok_or_else(|| PersistenceError::new("Review 不存在或尚无 current Snapshot。"))
    }

    /// 返回一个 Review 曾成功激活的 Snapshot identities，不暴露失败候选。
    #[cfg(test)]
    pub fn list_review_snapshot_identities(
        &self,
        review_id: &str,
    ) -> Result<Vec<String>, PersistenceError> {
        let row_id = parse_review_id(review_id)?;
        let connection = self.lock_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id FROM review_snapshots
                 WHERE review_id = ?1 AND activated_at_millis IS NOT NULL
                 ORDER BY id",
            )
            .map_err(map_sqlite_error)?;
        let snapshot_ids = statement
            .query_map([row_id], |row| row.get::<_, i64>(0))
            .map_err(map_sqlite_error)?
            .map(|id| id.map(|id| format!("snapshot-{id}")))
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sqlite_error)?;
        Ok(snapshot_ids)
    }

    /// 按 durable reviewId 读取 current Snapshot 及稳定 File Change metadata。
    pub fn open_review(&self, review_id: &str) -> Result<PersistedReviewState, PersistenceError> {
        let row_id = parse_review_id(review_id)?;
        let connection = self.lock_connection()?;
        let review = connection
            .query_row(
                "SELECT r.repository_id, r.selection_kind, r.branch_base_ref, r.branch_head_ref,
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
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, String>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, i64>(7)?,
                        row.get::<_, Option<String>>(8)?,
                    ))
                },
            )
            .optional()
            .map_err(map_sqlite_error)?
            .ok_or_else(|| PersistenceError::new("Review 不存在或尚无 current Snapshot。"))?;
        let mut statement = connection
            .prepare(
                "SELECT path, old_path, status, before_mode, after_mode,
                        before_object_id, after_object_id
                 FROM file_changes WHERE snapshot_id = ?1 ORDER BY id",
            )
            .map_err(map_sqlite_error)?;
        let file_changes = statement
            .query_map([review.7], |row| {
                let before_mode = row.get::<_, Option<u32>>(3)?;
                let after_mode = row.get::<_, Option<u32>>(4)?;
                let before_object_id = row.get::<_, Option<String>>(5)?;
                let after_object_id = row.get::<_, Option<String>>(6)?;
                Ok(FileChange {
                    path: row.get(0)?,
                    old_path: row.get(1)?,
                    status: parse_status(&row.get::<_, String>(2)?)?,
                    before: content_ref_from_metadata(&before_object_id, before_mode),
                    after: content_ref_from_metadata(&after_object_id, after_mode),
                })
            })
            .map_err(map_sqlite_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sqlite_error)?;
        let pull_request: Option<PullRequestMetadata> = review
            .8
            .map(|metadata| serde_json::from_str(&metadata))
            .transpose()
            .map_err(|_| PersistenceError::new("Pull Request metadata 已损坏。"))?;
        Ok(PersistedReviewState {
            review_id: format!("review-{row_id}"),
            repository_id: review.0,
            base_ref: pull_request.as_ref().map_or_else(
                || review.2.clone().unwrap_or_default(),
                |metadata| metadata.base_ref.clone(),
            ),
            head_ref: pull_request.as_ref().map_or_else(
                || review.3.clone().unwrap_or_default(),
                |metadata| metadata.head_ref.clone(),
            ),
            base_revision: review.4,
            comparison_base: review.5,
            head_revision: review.6,
            file_changes,
            created: false,
            pull_request,
            snapshot_id: review.7,
        })
    }

    /// 按最后打开时间列出一个 Repository 的 Reviews 或全局最近 Reviews。
    pub fn list_reviews(
        &self,
        repository_id: Option<i64>,
    ) -> Result<Vec<ReviewSummary>, PersistenceError> {
        let connection = self.lock_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT r.id, r.repository_id, e.canonical_path,
                        r.branch_base_ref, r.branch_head_ref, r.last_opened_at_millis,
                        r.pull_request_metadata, s.base_revision,
                        s.comparison_base, s.head_revision
                 FROM reviews r
                 JOIN repositories e ON e.id = r.repository_id
                 JOIN review_snapshots s ON s.id = r.current_snapshot_id
                 WHERE r.current_snapshot_id IS NOT NULL
                   AND (?1 IS NULL OR r.repository_id = ?1)
                 ORDER BY r.last_opened_at_millis DESC, r.id DESC",
            )
            .map_err(map_sqlite_error)?;
        let reviews = statement
            .query_map([repository_id], |row| {
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
                    repository_id: row.get(1)?,
                    repository_path: PathBuf::from(row.get::<_, String>(2)?),
                    base_ref: pull_request
                        .as_ref()
                        .map(|metadata| metadata.base_ref.clone())
                        .or_else(|| row.get::<_, Option<String>>(3).ok().flatten())
                        .unwrap_or_default(),
                    head_ref: pull_request
                        .as_ref()
                        .map(|metadata| metadata.head_ref.clone())
                        .or_else(|| row.get::<_, Option<String>>(4).ok().flatten())
                        .unwrap_or_default(),
                    last_opened_at_millis: row.get(5)?,
                    pull_request,
                    base_revision: row.get(7)?,
                    comparison_base: row.get(8)?,
                    head_revision: row.get(9)?,
                })
            })
            .map_err(map_sqlite_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sqlite_error)?;
        Ok(reviews)
    }

    /// 更新 Review 的最近打开时间，供全局 recent projection 排序。
    pub fn mark_review_opened(&self, review_id: &str) -> Result<(), PersistenceError> {
        let row_id = parse_review_id(review_id)?;
        let now = current_unix_time_millis()?;
        let connection = self.lock_connection()?;
        let changed = connection
            .execute(
                "UPDATE reviews SET last_opened_at_millis = ?1 WHERE id = ?2",
                params![now, row_id],
            )
            .map_err(map_sqlite_error)?;
        if changed != 1 {
            return Err(PersistenceError::new("Review 不存在。"));
        }
        Ok(())
    }

    /// 返回所有持久化 Review 的精确 workspace 清理证据，包括尚未激活的 Review。
    pub fn review_workspace_cleanup_targets(
        &self,
    ) -> Result<Vec<ReviewWorkspaceCleanupTarget>, PersistenceError> {
        let connection = self.lock_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT r.id, e.canonical_path, e.git_common_directory
                 FROM reviews r
                 JOIN repositories e ON e.id = r.repository_id
                 ORDER BY r.id",
            )
            .map_err(map_sqlite_error)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    PathBuf::from(row.get::<_, String>(1)?),
                    PathBuf::from(row.get::<_, String>(2)?),
                ))
            })
            .map_err(map_sqlite_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sqlite_error)?;
        Ok(rows
            .into_iter()
            .map(|(id, repository_path, git_common_directory)| {
                let review_id = format!("review-{id}");
                ReviewWorkspaceCleanupTarget {
                    workspace_path: self.workspace_root.join(&review_id).join("repo"),
                    review_id,
                    repository_path,
                    git_common_directory,
                }
            })
            .collect())
    }

    /// 返回 durable Review 对应的精确 Narrant-managed checkout 路径。
    pub fn review_workspace_path(&self, review_id: &str) -> Result<PathBuf, PersistenceError> {
        parse_review_id(review_id)?;
        Ok(self.workspace_root.join(review_id).join("repo"))
    }

    /// 统一处理 Persistence mutex 状态，避免调用点重复泄露同步细节。
    pub(super) fn lock_connection(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, Connection>, PersistenceError> {
        self.connection
            .lock()
            .map_err(|_| PersistenceError::new("Persistence 状态不可用。"))
    }
}
