use super::*;

use std::sync::Weak;
use tokio_util::sync::CancellationToken;

/// 同一短锁下协调使用登记与禁止新使用，避免取消后插入新的分析。
#[derive(Default)]
pub(super) struct WorkspaceAccess {
    pub changing: HashSet<String>,
    pub users: HashMap<String, Weak<CancellationToken>>,
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

/// 同步工作范围内固定的 Snapshot 和目录；子工作必须在回调返回前结束。
pub(crate) struct SnapshotWorkspace {
    pub comparison: ChangeComparison,
    pub repository_path: PathBuf,
    pub git_common_directory: PathBuf,
    pub head_path: PathBuf,
    pub before_path: PathBuf,
    pub operation: OperationContext,
}

/// 私有租约由实际 blocking worker 持有，外层 future 无法提前释放目录保护。
struct SnapshotWorkspaceLease {
    workspace: SnapshotWorkspace,
    _revocation: Arc<CancellationToken>,
    _review_guard: tokio::sync::OwnedMutexGuard<()>,
    _operation_guard: tokio::sync::OwnedRwLockReadGuard<()>,
}

impl SnapshotWorkspaceLease {
    /// 整体移入租约，避免闭包仅捕获 workspace 字段而把锁留在异步调用方。
    fn execute<T, E: From<ReviewLifecycleError>>(
        self,
        work: impl FnOnce(&SnapshotWorkspace) -> Result<T, E>,
    ) -> Result<T, E> {
        self.workspace.validate()?;
        let result = work(&self.workspace)?;
        self.workspace.validate()?;
        Ok(result)
    }
}

impl SnapshotWorkspace {
    /// 执行前及成功发布前验证 head，拒绝取消或外部进程改写的源码版本。
    fn validate(&self) -> Result<(), ReviewLifecycleError> {
        GitProcessAdapter::new(&self.repository_path)
            .validate_worktree_revision(
                &self.head_path,
                &self.git_common_directory,
                &self.comparison.head_revision,
                self.operation.cancellation(),
            )
            .map_err(ReviewLifecycleError::git)
    }
}

impl ReviewLifecycle {
    /// 在固定 Review Snapshot 内执行同步工作，并封装互斥、撤销和 worker 寿命。
    /// 回调只能在此执行范围内使用目录；它启动的全部子工作必须在返回前结束。
    pub(crate) async fn run_in_workspace<T, E>(
        &self,
        operation: &OperationContext,
        review_id: &str,
        snapshot_id: &str,
        work: impl FnOnce(&SnapshotWorkspace) -> Result<T, E> + Send + 'static,
    ) -> Result<T, E>
    where
        T: Send + 'static,
        E: From<ReviewLifecycleError> + Send + 'static,
    {
        let operation = operation.child();
        let cancellation = Arc::new(operation.cancellation().clone());
        // 放弃等待只请求取消；实际 worker 持有租约直到回调与清理结束。
        let _await_guard = operation.cancellation().clone().drop_guard();
        let lease = self
            .lease_workspace(operation, cancellation, review_id, snapshot_id)
            .await?;
        tokio::task::spawn_blocking(move || lease.execute(work))
            .await
            .map_err(|error| {
                E::from(ReviewLifecycleError::new(format!(
                    "工作目录执行失败：{error}"
                )))
            })?
    }

    /// 复用现有 head，不物化另一份 checkout；等待锁期间仍响应用户取消。
    async fn lease_workspace(
        &self,
        operation: OperationContext,
        cancellation: Arc<CancellationToken>,
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
        let before_path = head_path
            .parent()
            .ok_or_else(|| ReviewLifecycleError::new("Review Workspace 路径无效"))?
            .join("before");
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
                .insert(review_id.to_owned(), Arc::downgrade(&cancellation));
        }
        Ok(SnapshotWorkspaceLease {
            workspace: SnapshotWorkspace {
                comparison: current.to_change_comparison(),
                repository_path,
                git_common_directory: entry.git_common_directory,
                head_path,
                before_path,
                operation,
            },
            _revocation: cancellation,
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
            revoked.cancel();
        }
        Ok(())
    }
}
