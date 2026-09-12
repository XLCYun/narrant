pub(super) mod application_settings;
pub(super) mod application_update;
pub(super) mod babysit;
pub(super) mod github;
pub(super) mod repository;
pub(super) mod review;
pub(super) mod structure_review;

#[cfg(test)]
#[path = "tests/review.rs"]
mod review_tests;
