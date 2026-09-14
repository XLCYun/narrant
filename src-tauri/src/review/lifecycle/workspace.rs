use super::*;

impl ReviewLifecycle {
    /// 先完成逻辑关闭，再尝试清理 workspace；清理失败不会重新打开 Review。
    pub async fn close(&self, review_id: &str) -> Result<CloseOutcome, ReviewLifecycleError> {
        let _operation_guard = self.enter_operation().await?;
        let _workspace_change = self.begin_workspace_change(review_id)?;
        let review_lock = self.review_lock(review_id)?;
        let _review_guard = review_lock.lock().await;
        self.remove_runtime_review(review_id)?;
        let saved = self
            .store
            .open_review(review_id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        let target = self.cleanup_target(&saved)?;
        let existed = target.workspace_path.exists()
            || target
                .workspace_path
                .parent()
                .is_some_and(|parent| parent.join("before").exists());
        let cleanup = remove_workspace(&target).map_or_else(WorkspaceCleanup::Deferred, |()| {
            if existed {
                WorkspaceCleanup::Complete
            } else {
                WorkspaceCleanup::NotNeeded
            }
        });
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
        self.revoke_workspace_uses()?;
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
    let parent = target
        .workspace_path
        .parent()
        .ok_or_else(|| "Review Workspace 路径无效".to_owned())?;
    let adapter = GitProcessAdapter::new(&target.repository_path);
    let mut errors = Vec::new();
    for path in [parent.join("before"), target.workspace_path.clone()] {
        if let Err(error) = adapter.remove_owned_worktree_if_present(
            &path,
            &target.git_common_directory,
            OperationContext::background().cancellation(),
        ) {
            errors.push(format!("{} {}：{error}", target.review_id, path.display()));
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("；"))
    }
}
