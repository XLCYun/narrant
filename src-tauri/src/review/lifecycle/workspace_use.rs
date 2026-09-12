use super::*;
use std::sync::Weak;

/// 同一短锁下协调使用登记与禁止新使用，避免取消后插入新的分析。
#[derive(Default)]
pub(super) struct WorkspaceAccess {
    pub changing: HashSet<String>,
    pub users: HashMap<String, Weak<AtomicBool>>,
}

/// 目录收敛期间关闭使用入口；错误和 future 被丢弃时也恢复入口。
pub(super) struct WorkspaceChangeGuard<'a> {
    lifecycle: &'a ReviewLifecycle,
    review_id: String,
}

impl Drop for WorkspaceChangeGuard<'_> {
    /// 释放当前收敛操作独占的入口标记。
    fn drop(&mut self) {
        self.lifecycle.finish_refresh(&self.review_id);
    }
}

/// 固定 Snapshot 对现有 head 的独占使用；必须由实际分析 worker 持有到退出。
pub(crate) struct SnapshotWorkspaceLease {
    pub comparison: ChangeComparison,
    pub repository_path: PathBuf,
    pub git_common_directory: PathBuf,
    pub head_path: PathBuf,
    pub before_path: PathBuf,
    revoked: Arc<AtomicBool>,
    _review_guard: tokio::sync::OwnedMutexGuard<()>,
    _operation_guard: tokio::sync::OwnedRwLockReadGuard<()>,
}

impl SnapshotWorkspaceLease {
    /// 只读合并用户取消和目录使用撤销，不向消费者提供取消触发权。
    pub fn operation_context(&self, operation: &OperationContext) -> OperationContext {
        operation.with_revocation(self.revoked.clone())
    }
}

impl ReviewLifecycle {
    /// 复用现有 head，不物化另一份 checkout；等待锁期间仍响应用户取消。
    pub(crate) async fn lease_workspace(
        &self,
        operation: &OperationContext,
        review_id: &str,
        snapshot_id: &str,
    ) -> Result<SnapshotWorkspaceLease, ReviewLifecycleError> {
        if self.is_shutting_down() {
            return Err(ReviewLifecycleError::ShuttingDown);
        }
        let operation_guard = tokio::select! {
            _ = operation.cancellation().cancelled() => return Err(ReviewLifecycleError::Cancelled("结构分析已取消。".into())),
            guard = self.operation_gate.clone().read_owned() => guard,
        };
        if self.is_refreshing(review_id)? {
            return Err(ReviewLifecycleError::Busy);
        }
        let review_lock = self.review_lock(review_id)?;
        let review_guard = tokio::select! {
            _ = operation.cancellation().cancelled() => return Err(ReviewLifecycleError::Cancelled("结构分析已取消。".into())),
            guard = review_lock.lock_owned() => guard,
        };
        let current = self
            .store
            .open_review(review_id)
            .map_err(ReviewLifecycleError::persistence)?;
        let runtime = self.open_review_runtime(review_id)?.ok_or_else(|| {
            ReviewLifecycleError::ReviewUnavailable("Review 已关闭，请重新打开。".into())
        })?;
        if current.snapshot_identity() != snapshot_id || runtime.snapshot_id != snapshot_id {
            return Err(ReviewLifecycleError::StaleSnapshot);
        }
        let repository_path = self.require_trusted_repository(current.repository_id)?;
        let entry = self
            .repository_application
            .get_repository(current.repository_id)
            .map_err(Self::repository_application_error)?;
        let head_path = self
            .store
            .review_workspace_path(review_id)
            .map_err(ReviewLifecycleError::persistence)?;
        GitProcessAdapter::new(&repository_path)
            .validate_worktree_revision(
                &head_path,
                &entry.git_common_directory,
                &current.head_revision,
                operation.cancellation(),
            )
            .map_err(ReviewLifecycleError::git)?;
        let before_path = head_path
            .parent()
            .ok_or_else(|| ReviewLifecycleError::new("Review Workspace 路径无效"))?
            .join("before");
        let revoked = Arc::new(AtomicBool::new(false));
        {
            let mut access = self
                .workspace_access
                .lock()
                .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?;
            if self.is_shutting_down() {
                return Err(ReviewLifecycleError::ShuttingDown);
            }
            if access.changing.contains(review_id) {
                return Err(ReviewLifecycleError::Busy);
            }
            if operation.cancellation().is_cancelled() {
                return Err(ReviewLifecycleError::Cancelled("结构分析已取消。".into()));
            }
            access
                .users
                .insert(review_id.to_owned(), Arc::downgrade(&revoked));
        }
        Ok(SnapshotWorkspaceLease {
            comparison: current.to_change_comparison(),
            repository_path,
            git_common_directory: entry.git_common_directory,
            head_path,
            before_path,
            revoked,
            _review_guard: review_guard,
            _operation_guard: operation_guard,
        })
    }

    /// 先禁止新使用并撤销已有使用，再由调用者等待 Review 锁。
    pub(super) fn begin_workspace_change(
        &self,
        review_id: &str,
    ) -> Result<WorkspaceChangeGuard<'_>, ReviewLifecycleError> {
        self.begin_refresh(review_id)?;
        Ok(WorkspaceChangeGuard {
            lifecycle: self,
            review_id: review_id.to_owned(),
        })
    }

    /// shutdown 的独立入口也撤销全部使用，之后才能等待 owned gate 排空。
    pub(super) fn revoke_workspace_uses(&self) -> Result<(), ReviewLifecycleError> {
        let access = self
            .workspace_access
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?;
        for revoked in access.users.values().filter_map(Weak::upgrade) {
            revoked.store(true, Ordering::Release);
        }
        Ok(())
    }
}
