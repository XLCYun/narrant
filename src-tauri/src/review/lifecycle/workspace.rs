use super::*;

impl ReviewLifecycle {
    /// 先完成逻辑关闭，再尝试清理 workspace；清理失败不会重新打开 Review。
    pub fn close(&self, review_id: &str) -> Result<CloseOutcome, ReviewLifecycleError> {
        if self.is_shutting_down() {
            return Err(ReviewLifecycleError::ShuttingDown);
        }
        if self.is_refreshing(review_id)? {
            return Err(ReviewLifecycleError::Busy);
        }
        let review_lock = self.review_lock(review_id)?;
        let _review_guard = review_lock
            .try_lock()
            .map_err(|_| ReviewLifecycleError::Busy)?;
        self.remove_runtime_review(review_id)?;
        let saved = self
            .store
            .open_review(review_id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        let target = self.cleanup_target(&saved)?;
        if !target.workspace_path.exists() {
            return Ok(CloseOutcome {
                workspace_cleanup: WorkspaceCleanup::NotNeeded,
            });
        }
        let cleanup = remove_workspace(&target)
            .map_or_else(WorkspaceCleanup::Deferred, |()| WorkspaceCleanup::Complete);
        Ok(CloseOutcome {
            workspace_cleanup: cleanup,
        })
    }

    /// 启动时先用持久化所有权证据清理 workspace，再放弃未激活候选。
    pub fn recover(&self) -> Result<RecoveryReport, ReviewLifecycleError> {
        let cleanup_warnings = self.cleanup_workspaces()?;
        let abandoned_snapshot_ids = self
            .store
            .abandon_unactivated_candidates()
            .map_err(ReviewLifecycleError::persistence)?;
        Ok(RecoveryReport {
            abandoned_snapshot_ids,
            cleanup_warnings,
        })
    }

    /// 应用退出时拒绝新操作，等待既有 materialize/refresh 收敛，再清理全部 runtime。
    pub async fn shutdown(&self) -> Result<Vec<String>, ReviewLifecycleError> {
        self.shutting_down.store(true, Ordering::Release);
        let _operation_guard = self.operation_gate.write().await;
        self.clear_runtime()?;
        self.cleanup_workspaces()
    }

    /// 构造某个 Review 的精确清理目标。
    fn cleanup_target(
        &self,
        saved: &PersistedReviewState,
    ) -> Result<ReviewWorkspaceCleanupTarget, ReviewLifecycleError> {
        let entry = self
            .repository_application
            .get_repository(saved.repository_id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        Ok(ReviewWorkspaceCleanupTarget {
            review_id: saved.review_id.clone(),
            repository_path: entry.canonical_path,
            git_common_directory: entry.git_common_directory,
            workspace_path: self
                .store
                .review_workspace_path(&saved.review_id)
                .map_err(|error| ReviewLifecycleError::new(error.to_string()))?,
        })
    }

    /// 启动恢复或进程退出时尽力清理全部受管 workspace，并保留可恢复诊断。
    pub(super) fn cleanup_workspaces(&self) -> Result<Vec<String>, ReviewLifecycleError> {
        let targets = self
            .store
            .review_workspace_cleanup_targets()
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        Ok(targets
            .into_iter()
            .filter_map(|target| remove_workspace(&target).err())
            .collect())
    }
}

/// 精确移除已验证归属的受管 workspace。
fn remove_workspace(target: &ReviewWorkspaceCleanupTarget) -> Result<(), String> {
    GitProcessAdapter::new(&target.repository_path)
        .remove_owned_worktree_if_present(
            &target.workspace_path,
            &target.git_common_directory,
            OperationContext::background().cancellation(),
        )
        .map_err(|error| format!("{}：{error}", target.review_id))
}
