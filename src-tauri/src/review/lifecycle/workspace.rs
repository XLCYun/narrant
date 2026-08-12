use super::*;

impl ReviewLifecycle {
    /// 先完成逻辑关闭，再尝试清理 workspace；清理失败不会重新打开 Review。
    pub fn close(&self, review_id: &str) -> Result<CloseOutcome, ReviewLifecycleError> {
        if self.is_refreshing(review_id)? {
            return Err(ReviewLifecycleError::Busy);
        }
        self.reviews
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?
            .remove(review_id);
        let saved = self
            .catalog
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

    /// 启动时放弃未激活候选，并逐项清理遗留受管 workspace。
    pub fn recover(&self) -> Result<RecoveryReport, ReviewLifecycleError> {
        let abandoned_snapshot_ids = self
            .catalog
            .abandon_unactivated_candidates()
            .map_err(ReviewLifecycleError::catalog)?;
        let cleanup_warnings = self.cleanup_workspaces(None)?;
        Ok(RecoveryReport {
            abandoned_snapshot_ids,
            cleanup_warnings,
        })
    }

    /// 应用退出时清空运行时句柄，并尽力清理全部 workspace。
    pub fn shutdown(&self) -> Result<Vec<String>, ReviewLifecycleError> {
        self.reviews
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?
            .clear();
        self.cleanup_workspaces(None)
    }

    /// 构造某个 Review 的精确清理目标。
    fn cleanup_target(
        &self,
        saved: &SavedBranchReview,
    ) -> Result<WorkspaceCleanupTarget, ReviewLifecycleError> {
        let entry = self
            .repository_application
            .inspect_entry(saved.repository_entry_id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        Ok(WorkspaceCleanupTarget {
            review_id: saved.review_id.clone(),
            repository_path: entry.canonical_path,
            common_directory: entry.common_directory,
            workspace_path: self
                .catalog
                .review_workspace_path(&saved.review_id)
                .map_err(|error| ReviewLifecycleError::new(error.to_string()))?,
        })
    }

    /// 尽力清理除指定 Review 外的受管 workspace，并保留全部可恢复诊断。
    pub(super) fn cleanup_workspaces(
        &self,
        except_review_id: Option<&str>,
    ) -> Result<Vec<String>, ReviewLifecycleError> {
        let targets = self
            .catalog
            .workspace_cleanup_targets()
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        Ok(targets
            .into_iter()
            .filter(|target| {
                !except_review_id.is_some_and(|review_id| review_id == target.review_id)
                    && target.workspace_path.exists()
            })
            .filter_map(|target| remove_workspace(&target).err())
            .collect())
    }
}

/// 精确移除已验证归属的受管 workspace。
fn remove_workspace(target: &WorkspaceCleanupTarget) -> Result<(), String> {
    GitProcessAdapter::new(&target.repository_path)
        .remove_owned_worktree(
            &target.workspace_path,
            &target.common_directory,
            &CancellationToken::default(),
        )
        .map_err(|error| format!("{}：{error}", target.review_id))
}
