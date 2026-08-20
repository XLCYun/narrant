use super::*;
use crate::review::FileContent;
use httpmock::prelude::*;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;
use tokio::sync::Barrier;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// 测试专用的双阶段同步点，用于确定性暂停已持有 lifecycle read gate 的操作。
pub(super) struct LifecycleOperationGateBarrier {
    entered: Barrier,
    released: Barrier,
}

impl LifecycleOperationGateBarrier {
    /// 创建一个由生命周期操作与测试线程共同参与的同步点。
    pub(super) fn new() -> Self {
        Self {
            entered: Barrier::new(2),
            released: Barrier::new(2),
        }
    }

    /// 由生命周期操作报告已持有 read gate，并等待测试线程释放。
    pub(super) async fn pause_operation(&self) {
        self.entered.wait().await;
        self.released.wait().await;
    }

    /// 等待生命周期操作进入 read gate 后的暂停位置。
    pub(super) async fn wait_until_operation_paused(&self) {
        self.entered.wait().await;
    }

    /// 释放已暂停的生命周期操作，使其继续收敛。
    pub(super) async fn release_operation(&self) {
        self.released.wait().await;
    }
}

/// 安装会在真实 `git worktree add` 中阻塞的 post-checkout hook。
#[cfg(unix)]
fn install_blocking_post_checkout_hook(repository: &Path, entered: &Path, released: &Path) {
    let hook = repository.join(".git/hooks/post-checkout");
    std::fs::create_dir_all(hook.parent().unwrap()).unwrap();
    std::fs::write(
        &hook,
        format!(
            "#!/bin/sh\nset -eu\n: > \"{}\"\nwhile [ ! -e \"{}\" ]; do sleep 0.01; done\n",
            entered.display(),
            released.display()
        ),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&hook).unwrap().permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(hook, permissions).unwrap();
}

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
