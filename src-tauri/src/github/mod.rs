mod auth;
mod discovery;

pub(crate) use auth::*;
pub(crate) use discovery::{
    discover_pull_requests, observe_pull_request, remote_names_for_repository_with_cancellation,
    GitHubRepository, PullRequestGroup,
};

#[cfg(test)]
#[path = "discovery_tests.rs"]
mod discovery_tests;
