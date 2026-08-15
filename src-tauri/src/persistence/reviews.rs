use super::*;

impl ReviewStore {
    /// 原子保存 Branch Selection、首个 Snapshot、File Changes 与 current 指针。
    ///
    /// 同一 Repository 的相同 canonical branch pair 会返回既有 Review。
    pub fn save_branch_review(
        &self,
        repository_id: i64,
        comparison: &ChangeComparison,
    ) -> Result<PersistedReviewState, PersistenceError> {
        let now = current_unix_time_millis()?;
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction().map_err(map_sqlite_error)?;
        transaction
            .execute(
                "INSERT INTO reviews (
                    repository_id, selection_kind, branch_base_ref, branch_head_ref,
                    current_snapshot_id, created_at_millis, last_opened_at_millis
                 ) VALUES (?1, 'branch', ?2, ?3, NULL, ?4, ?4)
                 ON CONFLICT(repository_id, branch_base_ref, branch_head_ref)
                 WHERE selection_kind = 'branch'
                 DO UPDATE SET last_opened_at_millis = excluded.last_opened_at_millis",
                params![repository_id, comparison.base_ref, comparison.head_ref, now],
            )
            .map_err(map_sqlite_error)?;
        let review_row_id: i64 = transaction
            .query_row(
                "SELECT id FROM reviews
                 WHERE repository_id = ?1 AND selection_kind = 'branch'
                   AND branch_base_ref = ?2 AND branch_head_ref = ?3",
                params![repository_id, comparison.base_ref, comparison.head_ref],
                |row| row.get(0),
            )
            .map_err(map_sqlite_error)?;
        let current_snapshot_id: Option<i64> = transaction
            .query_row(
                "SELECT current_snapshot_id FROM reviews WHERE id = ?1",
                [review_row_id],
                |row| row.get(0),
            )
            .map_err(map_sqlite_error)?;
        if current_snapshot_id.is_some() {
            transaction.commit().map_err(map_sqlite_error)?;
            drop(connection);
            return self.open_review(&format!("review-{review_row_id}"));
        }
        transaction
            .execute(
                "INSERT INTO review_snapshots (
                    review_id, base_revision, comparison_base, head_revision, created_at_millis
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    review_row_id,
                    comparison.base_revision,
                    comparison.comparison_base,
                    comparison.head_revision,
                    now
                ],
            )
            .map_err(map_sqlite_error)?;
        let snapshot_id = transaction.last_insert_rowid();
        insert_file_changes(&transaction, snapshot_id, &comparison.file_changes)?;
        transaction.commit().map_err(map_sqlite_error)?;
        Ok(PersistedReviewState {
            review_id: format!("review-{review_row_id}"),
            repository_id,
            base_ref: comparison.base_ref.clone(),
            head_ref: comparison.head_ref.clone(),
            base_revision: comparison.base_revision.clone(),
            comparison_base: comparison.comparison_base.clone(),
            head_revision: comparison.head_revision.clone(),
            file_changes: comparison.file_changes.clone(),
            created: true,
            pull_request: None,
            snapshot_id,
            expected_current_snapshot_id: None,
        })
    }

    /// 保存由 base GitHub repository identity 与 PR number 唯一标识的 Review。
    pub fn save_pull_request_review(
        &self,
        repository_id: i64,
        metadata: &PullRequestMetadata,
        comparison: &ChangeComparison,
    ) -> Result<PersistedReviewState, PersistenceError> {
        let now = current_unix_time_millis()?;
        let selection_owner = metadata.owner.to_ascii_lowercase();
        let selection_repository = metadata.repository.to_ascii_lowercase();
        let metadata_json = serde_json::to_string(metadata).map_err(|error| {
            PersistenceError::new(format!("无法保存 Pull Request metadata：{error}"))
        })?;
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction().map_err(map_sqlite_error)?;
        transaction
            .execute(
                "INSERT INTO reviews (
                    repository_id, selection_kind, github_repository_owner,
                    github_repository_name, pull_request_number,
                    current_snapshot_id, created_at_millis, last_opened_at_millis, pull_request_metadata
                 ) VALUES (?1, 'pull_request', ?2, ?3, ?4, NULL, ?5, ?5, ?6)
                 ON CONFLICT(repository_id, github_repository_owner,
                             github_repository_name, pull_request_number)
                 WHERE selection_kind = 'pull_request'
                 DO UPDATE SET
                    last_opened_at_millis = excluded.last_opened_at_millis,
                    pull_request_metadata = excluded.pull_request_metadata",
                params![
                    repository_id,
                    selection_owner,
                    selection_repository,
                    metadata.number,
                    now,
                    metadata_json
                ],
            )
            .map_err(map_sqlite_error)?;
        let review_row_id: i64 = transaction
            .query_row(
                "SELECT id FROM reviews
                 WHERE repository_id = ?1 AND selection_kind = 'pull_request'
                   AND github_repository_owner = ?2
                   AND github_repository_name = ?3
                   AND pull_request_number = ?4",
                params![
                    repository_id,
                    selection_owner,
                    selection_repository,
                    metadata.number
                ],
                |row| row.get(0),
            )
            .map_err(map_sqlite_error)?;
        let current_snapshot_id: Option<i64> = transaction
            .query_row(
                "SELECT current_snapshot_id FROM reviews WHERE id = ?1",
                [review_row_id],
                |row| row.get(0),
            )
            .map_err(map_sqlite_error)?;
        if current_snapshot_id.is_some() {
            transaction.commit().map_err(map_sqlite_error)?;
            drop(connection);
            return self.open_review(&format!("review-{review_row_id}"));
        }
        transaction
            .execute(
                "INSERT INTO review_snapshots (
                    review_id, base_revision, comparison_base, head_revision, created_at_millis
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    review_row_id,
                    comparison.base_revision,
                    comparison.comparison_base,
                    comparison.head_revision,
                    now
                ],
            )
            .map_err(map_sqlite_error)?;
        let snapshot_id = transaction.last_insert_rowid();
        insert_file_changes(&transaction, snapshot_id, &comparison.file_changes)?;
        transaction.commit().map_err(map_sqlite_error)?;
        Ok(PersistedReviewState {
            review_id: format!("review-{review_row_id}"),
            repository_id,
            base_ref: comparison.base_ref.clone(),
            head_ref: comparison.head_ref.clone(),
            base_revision: comparison.base_revision.clone(),
            comparison_base: comparison.comparison_base.clone(),
            head_revision: comparison.head_revision.clone(),
            file_changes: comparison.file_changes.clone(),
            created: true,
            pull_request: Some(metadata.clone()),
            snapshot_id,
            expected_current_snapshot_id: None,
        })
    }

    /// 查找相同 Pull Request Selection 已存在且可打开的 current Snapshot。
    pub fn find_pull_request_review(
        &self,
        repository_id: i64,
        owner: &str,
        repository: &str,
        number: u64,
    ) -> Result<Option<PersistedReviewState>, PersistenceError> {
        let connection = self.lock_connection()?;
        let row_id = connection
            .query_row(
                "SELECT id FROM reviews
                 WHERE repository_id = ?1 AND selection_kind = 'pull_request'
                   AND github_repository_owner = ?2
                   AND github_repository_name = ?3
                   AND pull_request_number = ?4
                   AND current_snapshot_id IS NOT NULL",
                params![
                    repository_id,
                    owner.to_ascii_lowercase(),
                    repository.to_ascii_lowercase(),
                    number
                ],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(map_sqlite_error)?;
        drop(connection);
        row_id
            .map(|id| self.open_review(&format!("review-{id}")))
            .transpose()
    }

    /// 列出一个 Repository 的全部 current Reviews，供历史列表与重开入口使用。
    #[cfg(test)]
    pub fn list_saved_review_snapshots(
        &self,
        repository_id: i64,
    ) -> Result<Vec<PersistedReviewState>, PersistenceError> {
        let connection = self.lock_connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id FROM reviews
                 WHERE repository_id = ?1 AND current_snapshot_id IS NOT NULL
                 ORDER BY last_opened_at_millis DESC, id DESC",
            )
            .map_err(map_sqlite_error)?;
        let review_ids = statement
            .query_map([repository_id], |row| row.get::<_, i64>(0))
            .map_err(map_sqlite_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sqlite_error)?;
        drop(statement);
        drop(connection);
        review_ids
            .into_iter()
            .map(|id| self.open_review(&format!("review-{id}")))
            .collect()
    }
}
