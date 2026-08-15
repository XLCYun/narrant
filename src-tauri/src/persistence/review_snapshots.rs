use super::*;

/// 刷新候选需要随激活一起发布的 Selection metadata。
enum RefreshCandidateMetadata<'a> {
    Branch,
    PullRequest(&'a PullRequestMetadata),
}

/// Snapshot 激活时允许的 current 指针前置状态与活动时间策略。
enum SnapshotActivation {
    Initial,
    Refresh {
        expected_current_snapshot_id: i64,
        last_opened_at_millis: i64,
    },
}

impl ReviewStore {
    /// 暂存 Pull Request 刷新候选及其 metadata，但不改变 current 指针。
    pub fn stage_pull_request_refresh_candidate(
        &self,
        current: &PersistedReviewState,
        metadata: &PullRequestMetadata,
        comparison: &ChangeComparison,
    ) -> Result<PersistedReviewState, PersistenceError> {
        self.stage_refresh_candidate(
            current,
            comparison,
            RefreshCandidateMetadata::PullRequest(metadata),
        )
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

    /// 在首个 workspace HEAD 已校验后激活 Review 的初始 Snapshot。
    pub fn activate_initial_snapshot(
        &self,
        candidate: &PersistedReviewState,
    ) -> Result<(), PersistenceError> {
        self.activate_candidate(candidate, SnapshotActivation::Initial)
    }

    /// 暂存 Branch Selection 刷新候选，但不改变 current 指针。
    pub fn stage_branch_refresh_candidate(
        &self,
        review_id: &str,
        comparison: &ChangeComparison,
    ) -> Result<PersistedReviewState, PersistenceError> {
        let current = self.open_review(review_id)?;
        if current.base_ref != comparison.base_ref || current.head_ref != comparison.head_ref {
            return Err(PersistenceError::new(
                "刷新结果与 Review Selection 不一致。",
            ));
        }
        self.stage_refresh_candidate(&current, comparison, RefreshCandidateMetadata::Branch)
    }

    /// 仅在 current 仍是用户确认的旧版本时原子推进刷新 Snapshot。
    pub fn activate_refresh_candidate(
        &self,
        candidate: &PersistedReviewState,
        expected_current: &PersistedReviewState,
    ) -> Result<(), PersistenceError> {
        if expected_current.review_id != candidate.review_id {
            return Err(PersistenceError::new("刷新前后的 Review identity 不一致。"));
        }
        self.activate_candidate(
            candidate,
            SnapshotActivation::Refresh {
                expected_current_snapshot_id: expected_current.snapshot_id,
                last_opened_at_millis: current_unix_time_millis()?,
            },
        )
    }

    /// 在一个事务中保存刷新候选及其 File Changes。
    fn stage_refresh_candidate(
        &self,
        current: &PersistedReviewState,
        comparison: &ChangeComparison,
        metadata: RefreshCandidateMetadata<'_>,
    ) -> Result<PersistedReviewState, PersistenceError> {
        let review_row_id = parse_review_id(&current.review_id)?;
        let now = current_unix_time_millis()?;
        let (metadata_json, pull_request) = match metadata {
            RefreshCandidateMetadata::Branch => (None, current.pull_request.clone()),
            RefreshCandidateMetadata::PullRequest(metadata) => {
                let metadata_json = serde_json::to_string(metadata).map_err(|error| {
                    PersistenceError::new(format!("无法保存 Pull Request metadata：{error}"))
                })?;
                (Some(metadata_json), Some(metadata.clone()))
            }
        };
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
            pull_request,
            snapshot_id,
        })
    }

    /// 以 compare-and-swap 原子激活候选，并保留首次激活时间。
    fn activate_candidate(
        &self,
        candidate: &PersistedReviewState,
        activation: SnapshotActivation,
    ) -> Result<(), PersistenceError> {
        let (expected_current_snapshot_id, last_opened_at_millis, conflict_message) =
            match activation {
                SnapshotActivation::Initial => (
                    None,
                    None,
                    "Review current Snapshot 已改变；请重新打开后重试。",
                ),
                SnapshotActivation::Refresh {
                    expected_current_snapshot_id,
                    last_opened_at_millis,
                } => (
                    Some(expected_current_snapshot_id),
                    Some(last_opened_at_millis),
                    "Review current Snapshot 已改变；请重新检查更新。",
                ),
            };
        let row_id = parse_review_id(&candidate.review_id)?;
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction().map_err(map_sqlite_error)?;
        let changed = transaction
            .execute(
                "UPDATE reviews SET
                    current_snapshot_id = ?1,
                    last_opened_at_millis = COALESCE(?4, last_opened_at_millis),
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
                    candidate.snapshot_id,
                    row_id,
                    expected_current_snapshot_id,
                    last_opened_at_millis
                ],
            )
            .map_err(map_sqlite_error)?;
        if changed != 1 {
            return Err(PersistenceError::new(conflict_message));
        }
        let activated_at_millis = last_opened_at_millis.unwrap_or(current_unix_time_millis()?);
        transaction
            .execute(
                "UPDATE review_snapshots SET activated_at_millis = COALESCE(activated_at_millis, ?1)
                 WHERE id = ?2 AND review_id = ?3",
                params![activated_at_millis, candidate.snapshot_id, row_id],
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
