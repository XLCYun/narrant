mod branch;
mod pull_request;

#[cfg(test)]
pub(crate) use branch::BranchRefs;
pub(crate) use branch::{
    BranchRefCatalog, BranchSelection, BranchSelectionResolutionError, BranchSelectionResolver,
};
#[allow(
    unused_imports,
    reason = "供 Persistence 测试构造强类型 Pull Request 状态"
)]
pub(crate) use pull_request::{
    PullRequestMetadata, PullRequestResolutionError, PullRequestSelection,
    PullRequestSelectionResolver, PullRequestState,
};
