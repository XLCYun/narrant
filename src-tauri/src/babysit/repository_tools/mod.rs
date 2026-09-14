use std::fs;
use std::path::{Path, PathBuf};
use tokio_util::sync::CancellationToken;

mod contract;
mod glob;
mod grep;
mod read;
#[path = "rig.rs"]
mod rig_adapter;

pub use contract::*;
// 保留拆分前的公开类型路径；具体 Rig 工具类型只由集成方与契约测试按需引用。
#[allow(unused_imports)]
pub use rig_adapter::{RepositoryRigTools, RigGlobTool, RigGrepTool, RigReadTool};

/// 绑定一个 canonical Review Workspace 的 Narrant-owned 工具集合。
#[derive(Clone, Debug)]
pub struct RepositoryTools {
    pub(super) workspace_root: PathBuf,
    #[cfg(test)]
    cancel_before_execute: Option<CancellationToken>,
}

impl RepositoryTools {
    /// canonicalize 一次 Review Workspace，并拒绝非目录根。
    pub fn new(workspace_root: impl AsRef<Path>) -> Result<Self, RepositoryToolError> {
        let canonical_root = fs::canonicalize(workspace_root.as_ref())
            .map_err(|_| RepositoryToolError::WorkspaceUnavailable)?;
        if !canonical_root.is_dir() {
            return Err(RepositoryToolError::WorkspaceUnavailable);
        }
        Ok(Self {
            workspace_root: canonical_root,
            #[cfg(test)]
            cancel_before_execute: None,
        })
    }

    /// 测试专用：在进入真实 repository tool 实现时触发指定取消信号。
    #[cfg(test)]
    pub(crate) fn with_execute_cancellation(mut self, cancellation: CancellationToken) -> Self {
        self.cancel_before_execute = Some(cancellation);
        self
    }

    /// 测试专用：在 typed 或 Rig 工具进入真实实现前触发边界取消。
    #[cfg(test)]
    fn trigger_execute_cancellation(&self) {
        if let Some(cancel) = &self.cancel_before_execute {
            cancel.cancel();
        }
    }

    /// 返回仅含 Read、Glob、Grep 且每项带输入 schema 的注册视图。
    #[cfg(test)]
    pub fn registration(&self) -> RepositoryToolRegistrationView {
        RepositoryToolRegistrationView {
            tools: self.registrations(),
        }
    }

    /// 返回固定顺序的三项工具注册，供 provider adapter 消费。
    #[cfg(test)]
    pub fn registrations(&self) -> Vec<RepositoryToolRegistration> {
        rig_adapter::registrations()
    }

    /// 为一次可取消分析构造可直接交给 Rig agent builder 的工具。
    pub fn rig_tools(&self, cancellation: &CancellationToken) -> RepositoryRigTools {
        rig_adapter::build(self, cancellation)
    }

    /// 执行 typed Read 请求。
    pub fn read(
        &self,
        input: ReadInput,
        cancellation: &CancellationToken,
    ) -> Result<ReadResponse, RepositoryToolError> {
        read::execute(&self.workspace_root, &input, cancellation)
    }

    /// 通过统一 typed seam 执行 Read、Glob 或 Grep 工具。
    #[cfg(test)]
    pub fn execute(
        &self,
        request: RepositoryToolRequest,
        cancellation: &CancellationToken,
    ) -> Result<RepositoryToolResponse, RepositoryToolError> {
        #[cfg(test)]
        self.trigger_execute_cancellation();
        match request {
            RepositoryToolRequest::Read(input) => self
                .read(input, cancellation)
                .map(RepositoryToolResponse::Read),
            RepositoryToolRequest::Glob(input) => {
                glob::execute(&self.workspace_root, &input, cancellation)
                    .map(RepositoryToolResponse::Glob)
            }
            RepositoryToolRequest::Grep(input) => {
                grep::execute(&self.workspace_root, &input, cancellation)
                    .map(RepositoryToolResponse::Grep)
            }
        }
    }
}

/// 检查共享取消 signal，并将取消统一映射为稳定工具错误。
pub(super) fn ensure_not_cancelled(
    cancellation: &CancellationToken,
) -> Result<(), RepositoryToolError> {
    if cancellation.is_cancelled() {
        Err(RepositoryToolError::Cancelled)
    } else {
        Ok(())
    }
}

/// 以组件边界证明 canonical 候选路径位于 Review Workspace 内。
pub(super) fn is_contained(workspace_root: &Path, candidate: &Path) -> bool {
    candidate.strip_prefix(workspace_root).is_ok()
}

/// 把 workspace-relative 路径投影为工具协议的 POSIX 路径。
pub(super) fn path_string(path: &Path) -> String {
    path.to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/")
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
