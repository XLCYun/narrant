use super::*;

impl ReviewStore {
    /// 暂存新的 immutable Snapshot；在 workspace 对齐前不改变 current 指针。
    pub fn stage_pull_request_snapshot(
        &self,
        current: &PersistedReviewState,
        metadata: &PullRequestMetadata,
        comparison: &ChangeComparison,
    ) -> Result<PersistedReviewState, PersistenceError> {
        let review_row_id = parse_review_id(&current.review_id)?;
        let now = current_unix_time_millis()?;
        let metadata_json = serde_json::to_string(metadata).map_err(|error| {
            PersistenceError::new(format!("无法保存 Pull Request metadata：{error}"))
        })?;
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction().map_err(map_sqlite_error)?;
        transaction
            .execute(
                "INSERT INTO review_snapshots (
                    review_id, base_revision, comparison_base, head_revision, created_at_millis,
                    pull_request_metadata
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    review_row_id,
                    comparison.base_revision,
                    comparison.comparison_base,
                    comparison.head_revision,
                    now,
                    metadata_json
                ],
            )
            .map_err(map_sqlite_error)?;
        let snapshot_id = transaction.last_insert_rowid();
        insert_file_changes(&transaction, snapshot_id, &comparison.file_changes)?;
        transaction.commit().map_err(map_sqlite_error)?;
        Ok(PersistedReviewState {
            review_id: current.review_id.clone(),
            repository_id: current.repository_id,
            base_ref: comparison.base_ref.clone(),
            head_ref: comparison.head_ref.clone(),
            base_revision: comparison.base_revision.clone(),
            comparison_base: comparison.comparison_base.clone(),
            head_revision: comparison.head_revision.clone(),
            file_changes: comparison.file_changes.clone(),
            created: true,
            pull_request: Some(metadata.clone()),
            snapshot_id,
            expected_current_snapshot_id: Some(current.snapshot_id),
        })
    }

    /// 查找相同 Branch Selection 已存在且可打开的 current Snapshot。
    pub fn find_branch_review(
        &self,
        repository_id: i64,
        base_ref: &str,
        head_ref: &str,
    ) -> Result<Option<PersistedReviewState>, PersistenceError> {
        let connection = self.lock_connection()?;
        let row_id = connection
            .query_row(
                "SELECT id FROM reviews
                 WHERE repository_id = ?1 AND selection_kind = 'branch'
                   AND branch_base_ref = ?2 AND branch_head_ref = ?3
                   AND current_snapshot_id IS NOT NULL",
                params![repository_id, base_ref, head_ref],
                |row| row.get::<_, i64>(0),
            )
            .optional()
            .map_err(map_sqlite_error)?;
        drop(connection);
        row_id
            .map(|id| self.open_review(&format!("review-{id}")))
            .transpose()
    }

    /// 在 workspace HEAD 已校验后原子推进 current Snapshot 指针。
    pub fn activate_snapshot(&self, review: &PersistedReviewState) -> Result<(), PersistenceError> {
        let row_id = parse_review_id(&review.review_id)?;
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction().map_err(map_sqlite_error)?;
        let changed = transaction
            .execute(
                "UPDATE reviews SET
                    current_snapshot_id = ?1,
                    pull_request_metadata = COALESCE(
                        (SELECT pull_request_metadata FROM review_snapshots WHERE id = ?1),
                        pull_request_metadata
                    )
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
            .map_err(map_sqlite_error)?;
        if changed != 1 {
            return Err(PersistenceError::new(
                "Review current Snapshot 已改变；请重新打开后重试。",
            ));
        }
        transaction
            .execute(
                "UPDATE review_snapshots SET activated_at_millis = COALESCE(activated_at_millis, ?1)
                 WHERE id = ?2 AND review_id = ?3",
                params![current_unix_time_millis()?, review.snapshot_id, row_id],
            )
            .map_err(map_sqlite_error)?;
        transaction.commit().map_err(map_sqlite_error)?;
        Ok(())
    }

    /// 保存明确刷新产生的新 immutable Snapshot，但暂不改变 current 指针。
    pub fn save_refresh_snapshot(
        &self,
        review_id: &str,
        comparison: &ChangeComparison,
    ) -> Result<PersistedReviewState, PersistenceError> {
        let row_id = parse_review_id(review_id)?;
        let current = self.open_review(review_id)?;
        if current.base_ref != comparison.base_ref || current.head_ref != comparison.head_ref {
            return Err(PersistenceError::new(
                "刷新结果与 Review Selection 不一致。",
            ));
        }
        let now = current_unix_time_millis()?;
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction().map_err(map_sqlite_error)?;
        transaction
            .execute(
                "INSERT INTO review_snapshots (
                    review_id, base_revision, comparison_base, head_revision, created_at_millis
                 ) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    row_id,
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
            review_id: review_id.to_owned(),
            repository_id: current.repository_id,
            base_ref: comparison.base_ref.clone(),
            head_ref: comparison.head_ref.clone(),
            base_revision: comparison.base_revision.clone(),
            comparison_base: comparison.comparison_base.clone(),
            head_revision: comparison.head_revision.clone(),
            file_changes: comparison.file_changes.clone(),
            created: true,
            pull_request: current.pull_request,
            snapshot_id,
            expected_current_snapshot_id: Some(current.snapshot_id),
        })
    }

    /// 仅在 current 仍是用户确认的旧版本时原子推进刷新 Snapshot。
    pub fn activate_refresh_snapshot(
        &self,
        review: &PersistedReviewState,
        expected_current: &PersistedReviewState,
    ) -> Result<(), PersistenceError> {
        let row_id = parse_review_id(&review.review_id)?;
        if expected_current.review_id != review.review_id {
            return Err(PersistenceError::new("刷新前后的 Review identity 不一致。"));
        }
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction().map_err(map_sqlite_error)?;
        let now = current_unix_time_millis()?;
        let changed = transaction
            .execute(
                "UPDATE reviews SET
                    current_snapshot_id = ?1,
                    last_opened_at_millis = ?2,
                    pull_request_metadata = COALESCE(
                        (SELECT pull_request_metadata FROM review_snapshots WHERE id = ?1),
                        pull_request_metadata
                    )
                 WHERE id = ?3 AND current_snapshot_id = ?4",
                params![
                    review.snapshot_id,
                    now,
                    row_id,
                    expected_current.snapshot_id
                ],
            )
            .map_err(map_sqlite_error)?;
        if changed != 1 {
            return Err(PersistenceError::new(
                "Review current Snapshot 已改变；请重新检查更新。",
            ));
        }
        transaction
            .execute(
                "UPDATE review_snapshots SET activated_at_millis = COALESCE(activated_at_millis, ?1)
                 WHERE id = ?2 AND review_id = ?3",
                params![now, review.snapshot_id, row_id],
            )
            .map_err(map_sqlite_error)?;
        transaction.commit().map_err(map_sqlite_error)?;
        Ok(())
    }

    /// 删除尚未激活的 Review Refresh Candidate 及其 File Changes。
    pub fn abandon_refresh_candidate(
        &self,
        review: &PersistedReviewState,
    ) -> Result<(), PersistenceError> {
        let row_id = parse_review_id(&review.review_id)?;
        let connection = self.lock_connection()?;
        connection
            .execute(
                "DELETE FROM review_snapshots
                 WHERE id = ?1 AND review_id = ?2 AND activated_at_millis IS NULL
                   AND NOT EXISTS(
                     SELECT 1 FROM reviews
                     WHERE id = ?2 AND current_snapshot_id = ?1
                   )",
                params![review.snapshot_id, row_id],
            )
            .map_err(map_sqlite_error)?;
        Ok(())
    }

    /// 启动恢复时删除全部从未激活的候选，返回被放弃的 Snapshot identities。
    pub fn abandon_unactivated_candidates(&self) -> Result<Vec<String>, PersistenceError> {
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction().map_err(map_sqlite_error)?;
        let candidate_ids = {
            let mut statement = transaction
                .prepare(
                    "SELECT s.id FROM review_snapshots s
                     WHERE s.activated_at_millis IS NULL
                       AND NOT EXISTS(
                         SELECT 1 FROM reviews r WHERE r.current_snapshot_id = s.id
                       )
                     ORDER BY s.id",
                )
                .map_err(map_sqlite_error)?;
            let candidate_ids = statement
                .query_map([], |row| row.get::<_, i64>(0))
                .map_err(map_sqlite_error)?
                .collect::<Result<Vec<_>, _>>()
                .map_err(map_sqlite_error)?;
            candidate_ids
        };
        transaction
            .execute(
                "DELETE FROM review_snapshots
                 WHERE activated_at_millis IS NULL
                   AND NOT EXISTS(
                     SELECT 1 FROM reviews r WHERE r.current_snapshot_id = review_snapshots.id
                   )",
                [],
            )
            .map_err(map_sqlite_error)?;
        transaction.commit().map_err(map_sqlite_error)?;
        Ok(candidate_ids
            .into_iter()
            .map(|id| format!("snapshot-{id}"))
            .collect())
    }
}
