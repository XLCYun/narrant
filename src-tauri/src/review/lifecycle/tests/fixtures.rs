use super::*;
use crate::review::FileContent;
use httpmock::prelude::*;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

#[test]
fn same_review_cannot_hold_two_refresh_leases() {
    let fixture = LifecycleFixture::new();
    let lifecycle = ReviewLifecycle::new(fixture.store.clone());

    lifecycle.begin_refresh("review-1").unwrap();
    let error = lifecycle.begin_refresh("review-1").unwrap_err();
    lifecycle.begin_refresh("review-2").unwrap();

    assert!(matches!(error, ReviewLifecycleError::Busy));
    lifecycle.finish_refresh("review-1");
    lifecycle.begin_refresh("review-1").unwrap();
}
use crate::repository::verify_repository_location;
