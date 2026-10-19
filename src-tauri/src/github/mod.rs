mod auth;
mod discovery;

pub(crate) use auth::*;
pub(crate) use discovery::{
    discover_pull_requests, observe_pull_request, remote_names_for_repository_with_cancellation,
    GitHubRepository, PullRequestDiscoveryQuery, PullRequestGroup,
};

#[cfg(test)]
#[path = "tests/discovery.rs"]
mod discovery_tests;
