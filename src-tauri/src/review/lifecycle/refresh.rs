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
        let repository_path = self.require_available_repository(current.repository_id)?;
        let available = if let Some(metadata) = current.pull_request.as_ref() {
            self.pull_requests
                .resolve(
                    &repository_path,
                    &pull_request_selection(metadata),
                    operation,
                )
                .await
                .map_err(ReviewLifecycleError::pull_request)?
                .comparison
        } else {
            self.branch_selections
                .resolve(
                    &repository_path,
                    &BranchSelection {
                        base_ref: current.base_ref.clone(),
                        head_ref: current.head_ref.clone(),
                    },
                    operation,
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
        self.begin_refresh(review_id)?;
        let result = self
            .refresh_exclusive(operation, review_id, confirmation)
            .await;
        self.finish_refresh(review_id);
        result
    }

    /// 物化 durable Review 后刷新；刷新失败时关闭刚物化的运行时与 Workspace。
    pub async fn reopen_and_refresh(
        &self,
        operation: &OperationContext,
        review_id: &str,
        confirmation: ConfirmedReviewUpdate,
    ) -> Result<ReviewPresentation, ReviewLifecycleError> {
        self.present(
            operation,
            PresentReview::Saved {
                review_id: review_id.to_owned(),
            },
        )
        .await?;
        match self.refresh(operation, review_id, confirmation).await {
            Ok(presentation) => Ok(presentation),
            Err(error) => {
                let cleanup = self.close(review_id);
                match cleanup {
                    Ok(CloseOutcome {
                        workspace_cleanup: WorkspaceCleanup::Deferred(cleanup_error),
                    }) => Err(ReviewLifecycleError::new(format!(
                        "{error}；Review 已关闭，但 Workspace 清理已延后：{cleanup_error}"
                    ))),
                    Err(cleanup_error) => Err(ReviewLifecycleError::new(format!(
                        "{error}；关闭失败：{cleanup_error}"
                    ))),
                    Ok(_) => Err(error),
                }
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
                    &repository_path,
                    &pull_request_selection(metadata),
                    operation,
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
                    &repository_path,
                    &BranchSelection {
                        base_ref: current.base_ref.clone(),
                        head_ref: current.head_ref.clone(),
                    },
                    operation,
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

        let workspace = self
            .store
            .review_workspace_path(review_id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        let mut reviews = self
            .reviews
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?;
        let convergence = operation.without_cancellation();
        let adapter = GitProcessAdapter::new(&repository_path);
        if let Err(error) = adapter.transition_detached_worktree(
            &workspace,
            &entry.git_common_directory,
            &current.head_revision,
            &candidate.head_revision,
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
            );
            let _ = self.store.abandon_refresh_candidate(&candidate);
            return Err(ReviewLifecycleError::ActivationRollback {
                activation: activation_error.to_string(),
                rollback: rollback.err().map(|error| error.to_string()),
            });
        }
        let snapshot = candidate.to_change_comparison();
        reviews.insert(
            review_id.to_owned(),
            Arc::new(SnapshotContentReader::restore(&repository_path, snapshot)),
        );
        Ok(presentation_from_saved(candidate, Vec::new()))
    }

    /// 获取同一 Review 的独占刷新权。
    pub(super) fn begin_refresh(&self, review_id: &str) -> Result<(), ReviewLifecycleError> {
        let mut active = self
            .active_refreshes
            .lock()
            .map_err(|_| ReviewLifecycleError::new("Review 刷新状态不可用。"))?;
        if !active.insert(review_id.to_owned()) {
            return Err(ReviewLifecycleError::Busy);
        }
        Ok(())
    }

    /// 释放同一 Review 的独占刷新权。
    pub(super) fn finish_refresh(&self, review_id: &str) {
        if let Ok(mut active) = self.active_refreshes.lock() {
            active.remove(review_id);
        }
    }

    /// 查询 Review 当前是否正在刷新。
    pub(super) fn is_refreshing(&self, review_id: &str) -> Result<bool, ReviewLifecycleError> {
        self.active_refreshes
            .lock()
            .map(|active| active.contains(review_id))
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
