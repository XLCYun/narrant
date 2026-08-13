use super::*;

impl RepositoryCatalog {
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
    #[cfg(test)]
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
}
