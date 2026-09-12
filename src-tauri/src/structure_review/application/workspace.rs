use super::StructureReviewError;
use crate::{
    core_git::GitProcessAdapter, operation::OperationContext, review::SnapshotWorkspaceLease,
};

/// 只拥有分析附加的 before；析构路径始终不删除已有 head。
pub(super) struct BeforeCheckout<'a> {
    lease: &'a SnapshotWorkspaceLease,
    armed: bool,
}

impl<'a> BeforeCheckout<'a> {
    /// 清理已确认归属的上次残留后，签出 Snapshot 固定的 comparison-base。
    pub fn prepare(
        lease: &'a SnapshotWorkspaceLease,
        operation: &OperationContext,
    ) -> Result<Self, StructureReviewError> {
        let adapter = GitProcessAdapter::new(&lease.repository_path);
        let convergence = operation.without_cancellation();
        adapter
            .remove_owned_worktree_if_present(
                &lease.before_path,
                &lease.git_common_directory,
                convergence.cancellation(),
            )
            .map_err(|error| StructureReviewError::new("cleanup-failed", error))?;
        adapter
            .create_detached_worktree(
                &lease.before_path,
                &lease.comparison.comparison_base,
                operation.cancellation(),
                convergence.cancellation(),
            )
            .map_err(|error| StructureReviewError::new("checkout-failed", error))?;
        Ok(Self { lease, armed: true })
    }

    /// 成功和失败都先释放 before，失败保留精确路径供既有恢复流程重试。
    pub fn finish(mut self) -> Result<(), StructureReviewError> {
        self.armed = false;
        self.cleanup()
    }

    /// 收敛清理使用不可取消上下文，避免用户取消连补偿也中止。
    fn cleanup(&self) -> Result<(), StructureReviewError> {
        GitProcessAdapter::new(&self.lease.repository_path)
            .remove_owned_worktree_if_present(
                &self.lease.before_path,
                &self.lease.git_common_directory,
                OperationContext::background().cancellation(),
            )
            .map_err(|error| StructureReviewError::new("cleanup-failed", error))
    }
}

impl Drop for BeforeCheckout<'_> {
    /// worker panic 时仍做精确回收；错误会在下次恢复时再次报告。
    fn drop(&mut self) {
        if self.armed {
            if let Err(error) = self.cleanup() {
                tracing::warn!(detail = %error.detail, "结构分析 before checkout 清理失败");
            }
        }
    }
}

/// 发布结果前重新检查现有 head，防止外部进程改写固定源码。
pub(super) fn verify_head(
    lease: &SnapshotWorkspaceLease,
    operation: &OperationContext,
) -> Result<(), StructureReviewError> {
    GitProcessAdapter::new(&lease.repository_path)
        .validate_worktree_revision(
            &lease.head_path,
            &lease.git_common_directory,
            &lease.comparison.head_revision,
            operation.cancellation(),
        )
        .map_err(|error| StructureReviewError::new("review-unavailable", error))
}
