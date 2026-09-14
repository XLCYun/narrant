use super::StructureReviewError;
use crate::{core_git::GitProcessAdapter, operation::OperationContext, review::SnapshotWorkspace};

/// 只拥有分析附加的 before；析构路径始终不删除已有 head。
pub(super) struct BeforeCheckout<'a> {
    workspace: &'a SnapshotWorkspace,
    armed: bool,
}

impl<'a> BeforeCheckout<'a> {
    /// 清理已确认归属的上次残留后，签出 Snapshot 固定的 comparison-base。
    pub fn prepare(
        workspace: &'a SnapshotWorkspace,
        operation: &OperationContext,
    ) -> Result<Self, StructureReviewError> {
        let adapter = GitProcessAdapter::new(&workspace.repository_path);
        let convergence = operation.without_cancellation();
        adapter
            .remove_owned_worktree_if_present(
                &workspace.before_path,
                &workspace.git_common_directory,
                convergence.cancellation(),
            )
            .map_err(|error| StructureReviewError::new("cleanup-failed", error))?;
        if let Err(error) = adapter.create_detached_worktree(
            &workspace.before_path,
            &workspace.comparison.comparison_base,
            operation.cancellation(),
            convergence.cancellation(),
        ) {
            // Core Git 已尝试补偿；再次确认附加目录已回收，残留不能作为普通取消返回。
            if let Err(cleanup) = adapter.remove_owned_worktree_if_present(
                &workspace.before_path,
                &workspace.git_common_directory,
                convergence.cancellation(),
            ) {
                return Err(StructureReviewError::new(
                    "cleanup-failed",
                    format!("{cleanup}；原始错误：{error}"),
                ));
            }
            return Err(StructureReviewError::new(
                if error.is_cancelled() {
                    "cancelled"
                } else {
                    "checkout-failed"
                },
                error,
            ));
        }
        Ok(Self {
            workspace,
            armed: true,
        })
    }

    /// 成功和失败都先释放 before，失败保留精确路径供既有恢复流程重试。
    pub fn finish(mut self) -> Result<(), StructureReviewError> {
        self.armed = false;
        self.cleanup()
    }

    /// 收敛清理使用不可取消上下文，避免用户取消连补偿也中止。
    fn cleanup(&self) -> Result<(), StructureReviewError> {
        GitProcessAdapter::new(&self.workspace.repository_path)
            .remove_owned_worktree_if_present(
                &self.workspace.before_path,
                &self.workspace.git_common_directory,
                OperationContext::background().cancellation(),
            )
            .map_err(|error| StructureReviewError::new("cleanup-failed", error))
    }
}

impl Drop for BeforeCheckout<'_> {
    /// 非分析器的意外展开仍尝试回收；分析器 panic 已在外层捕获并走显式 finish。
    fn drop(&mut self) {
        if self.armed {
            if let Err(error) = self.cleanup() {
                tracing::warn!(detail = %error.detail, "结构分析 before checkout 清理失败");
            }
        }
    }
}
