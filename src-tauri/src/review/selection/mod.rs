mod branch;
mod pull_request;

#[cfg(test)]
pub(crate) use branch::BranchRefs;
pub(crate) use branch::{
    BranchRefCatalog, BranchSelection, BranchSelectionResolutionError, BranchSelectionResolver,
};
pub(crate) use pull_request::{
    PullRequestResolutionError, PullRequestSelection, PullRequestSelectionResolver,
};
