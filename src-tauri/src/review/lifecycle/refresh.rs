use super::presentation::presentation_from_saved;
use super::*;

impl ReviewLifecycle {
    /// 解析最新可用版本，但不创建候选、不切换 workspace、不推进 current。
    pub async fn check_update(
        &self,
        operation: &OperationContext,
        review_id: &str,
    ) -> Result<ReviewUpdate, ReviewLifecycleError> {
        let current = self
            .store
            .open_review(review_id)
            .map_err(|error| ReviewLifecycleError::ReviewUnavailable(error.to_string()))?;
        let open_review = self.open_review_runtime(review_id)?.ok_or_else(|| {
            ReviewLifecycleError::ReviewUnavailable("Review 已关闭或失效，请重新打开。".to_owned())
        })?;
        if open_review.snapshot_id != current.snapshot_identity() {
            return Err(ReviewLifecycleError::StaleSnapshot);
        }
        let repository_path = self.require_available_repository(current.repository_id)?;
        let available = if let Some(metadata) = current.pull_request.as_ref() {
            self.pull_requests
                .resolve(
                    operation,
                    &repository_path,
                    &pull_request_selection(metadata),
                )
                .await
                .map_err(ReviewLifecycleError::pull_request)?
                .comparison
        } else {
            self.branch_selections
                .resolve(
                    operation,
                    &repository_path,
                    &BranchSelection {
                        base_ref: current.base_ref.clone(),
                        head_ref: current.head_ref.clone(),
                    },
                )
                .await
                .map_err(ReviewLifecycleError::branch)?
                .comparison
        };
        Ok(ReviewUpdate {
            update_available: current.base_revision != available.base_revision
                || current.head_revision != available.head_revision,
            current_base_revision: current.base_revision,
            current_head_revision: current.head_revision,
            available_base_revision: available.base_revision,
            available_head_revision: available.head_revision,
        })
    }

    /// 刷新 Review；准备阶段可取消，开始改变 workspace 后必须激活或回滚到旧版本。
    pub async fn refresh(
        &self,
        operation: &OperationContext,
        review_id: &str,
        confirmation: ConfirmedReviewUpdate,
    ) -> Result<ReviewPresentation, ReviewLifecycleError> {
        let _operation_guard = self.enter_operation().await?;
        self.refresh_guarded(operation, review_id, confirmation)
            .await
    }

    /// 在 lifecycle operation gate 内执行 refresh，供复合操作避免与 shutdown 交错。
    async fn refresh_guarded(
        &self,
        operation: &OperationContext,
        review_id: &str,
        confirmation: ConfirmedReviewUpdate,
    ) -> Result<ReviewPresentation, ReviewLifecycleError> {
        let _workspace_change = self.begin_workspace_change(review_id)?;
        let review_lock = self.review_lock(review_id)?;
        let _review_guard = review_lock.lock().await;
        self.refresh_exclusive(operation, review_id, confirmation)
            .await
    }

    /// 物化 durable Review 后刷新；刷新失败时保留运行时并恢复此前 Presented Review。
    pub async fn reopen_and_refresh(
        &self,
        operation: &OperationContext,
        review_id: &str,
        confirmation: ConfirmedReviewUpdate,
    ) -> Result<ReviewPresentation, ReviewLifecycleError> {
        let _operation_guard = self.enter_operation().await?;
        let present = self
            .present_guarded(
                operation,
                PresentReview::Saved {
                    review_id: review_id.to_owned(),
                },
            )
            .await?;
        match self
            .refresh_guarded(operation, review_id, confirmation)
            .await
        {
            Ok(presentation) => Ok(presentation),
            Err(error) => {
                self.restore_presentation(present.transition)?;
                Err(error)
            }
        }
    }

    /// 在刷新互斥区中执行准备、workspace 收敛、激活和必要回滚。
    async fn refresh_exclusive(
        &self,
        operation: &OperationContext,
        review_id: &str,
        confirmation: ConfirmedReviewUpdate,
    ) -> Result<ReviewPresentation, ReviewLifecycleError> {
        let current = self
            .store
            .open_review(review_id)
            .map_err(|error| ReviewLifecycleError::ReviewUnavailable(error.to_string()))?;
        let open_review = self.open_review_runtime(review_id)?.ok_or_else(|| {
            ReviewLifecycleError::ReviewUnavailable("Review 已关闭或失效，请重新打开。".to_owned())
        })?;
        if open_review.snapshot_id != current.snapshot_identity() {
            return Err(ReviewLifecycleError::StaleSnapshot);
        }
        let repository_path = self.require_available_repository(current.repository_id)?;
        let entry = self
            .repository_application
            .get_repository(current.repository_id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        if current.base_revision != confirmation.current_base_revision
            || current.head_revision != confirmation.current_head_revision
        {
            return Err(ReviewLifecycleError::StaleCurrent);
        }
        let candidate = if let Some(metadata) = current.pull_request.as_ref() {
            let available = self
                .pull_requests
                .resolve(
                    operation,
                    &repository_path,
                    &pull_request_selection(metadata),
                )
                .await
                .map_err(ReviewLifecycleError::pull_request)?;
            require_confirmed_available(&confirmation, &available.comparison, "Pull Request")?;
            if current.base_revision == available.comparison.base_revision
                && current.head_revision == available.comparison.head_revision
            {
                return Ok(presentation_from_saved(current, Vec::new()));
            }
            self.store
                .stage_pull_request_refresh_candidate(
                    &current,
                    &available.metadata,
                    &available.comparison,
                )
                .map_err(|error| ReviewLifecycleError::new(error.to_string()))?
        } else {
            let available = self
                .branch_selections
                .resolve(
                    operation,
                    &repository_path,
                    &BranchSelection {
                        base_ref: current.base_ref.clone(),
                        head_ref: current.head_ref.clone(),
                    },
                )
                .await
                .map_err(ReviewLifecycleError::branch)?
                .comparison;
            require_confirmed_available(&confirmation, &available, "Branch Selection")?;
            if current.base_revision == confirmation.available_base_revision
                && current.head_revision == confirmation.available_head_revision
            {
                return Ok(presentation_from_saved(current, Vec::new()));
            }
            self.store
                .stage_branch_refresh_candidate(review_id, &available)
                .map_err(|error| ReviewLifecycleError::new(error.to_string()))?
        };
        self.cancel_staged_refresh(operation, &candidate)?;

        let workspace = self
            .store
            .review_workspace_path(review_id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        let convergence = operation.without_cancellation();
        let adapter = GitProcessAdapter::new(&repository_path);
        if let Err(error) = adapter.transition_detached_worktree(
            &workspace,
            &entry.git_common_directory,
            &current.head_revision,
            &candidate.head_revision,
            convergence.cancellation(),
            convergence.cancellation(),
        ) {
            let _ = self.store.abandon_refresh_candidate(&candidate);
            return Err(ReviewLifecycleError::WorkspaceTransition(error.to_string()));
        }
        if let Err(activation_error) = self.store.activate_refresh_candidate(&candidate, &current) {
            let rollback = adapter.transition_detached_worktree(
                &workspace,
                &entry.git_common_directory,
                &candidate.head_revision,
                &current.head_revision,
                convergence.cancellation(),
                convergence.cancellation(),
            );
            let _ = self.store.abandon_refresh_candidate(&candidate);
            return Err(ReviewLifecycleError::ActivationRollback {
                activation: activation_error.to_string(),
                rollback: rollback.err().map(|error| error.to_string()),
            });
        }
        let snapshot = candidate.to_change_comparison();
        let reader = Arc::new(SnapshotContentReader::restore(&repository_path, snapshot));
        self.update_open_review_runtime(
            review_id.to_owned(),
            candidate.snapshot_identity(),
            reader,
            OpenReviewRuntimeUpdate::ReplaceSnapshot,
        )?;
        Ok(presentation_from_saved(candidate, Vec::new()))
    }

    /// 在候选已持久化、尚未改变 Workspace 前处理最后一次可取消边界。
    pub(super) fn cancel_staged_refresh(
        &self,
        operation: &OperationContext,
        candidate: &PersistedReviewState,
    ) -> Result<(), ReviewLifecycleError> {
        if !operation.cancellation().is_cancelled() {
            return Ok(());
        }
        self.store
            .abandon_refresh_candidate(candidate)
            .map_err(ReviewLifecycleError::persistence)?;
        Err(ReviewLifecycleError::Cancelled("操作已取消。".to_owned()))
    }

    /// 获取同一 Review 的独占刷新权。
    pub(super) fn begin_refresh(&self, review_id: &str) -> Result<(), ReviewLifecycleError> {
        let mut access = self
            .workspace_access
            .lock()
            .map_err(|_| ReviewLifecycleError::new("Review 刷新状态不可用。"))?;
        if !access.changing.insert(review_id.to_owned()) {
            return Err(ReviewLifecycleError::Busy);
        }
        if let Some(revoked) = access
            .users
            .get(review_id)
            .and_then(std::sync::Weak::upgrade)
        {
            revoked.cancel();
        }
        Ok(())
    }

    /// 释放同一 Review 的独占刷新权。
    pub(super) fn finish_refresh(&self, review_id: &str) {
        if let Ok(mut access) = self.workspace_access.lock() {
            access.changing.remove(review_id);
        }
    }

    /// 查询 Review 当前是否正在刷新。
    pub(super) fn is_refreshing(&self, review_id: &str) -> Result<bool, ReviewLifecycleError> {
        self.workspace_access
            .lock()
            .map(|access| access.changing.contains(review_id))
            .map_err(|_| ReviewLifecycleError::new("Review 刷新状态不可用。"))
    }
}

/// 从 durable metadata 恢复 Pull Request Selection identity。
fn pull_request_selection(metadata: &PullRequestMetadata) -> PullRequestSelection {
    PullRequestSelection {
        repository: crate::github::GitHubRepository {
            owner: metadata.owner.clone(),
            name: metadata.repository.clone(),
        },
        number: metadata.number,
    }
}

/// 拒绝用户确认后再次移动的 Review Selection。
fn require_confirmed_available(
    confirmation: &ConfirmedReviewUpdate,
    available: &ChangeComparison,
    selection_name: &str,
) -> Result<(), ReviewLifecycleError> {
    if available.base_revision != confirmation.available_base_revision
        || available.head_revision != confirmation.available_head_revision
    {
        return Err(ReviewLifecycleError::SelectionMismatch(format!(
            "{selection_name} 在确认后再次变化；请重新检查并确认新版本。"
        )));
    }
    Ok(())
}
