use super::*;

use std::sync::mpsc;
use tokio_util::sync::CancellationToken;

/// 放弃等待本身触发取消；关闭尚未开始也能观察到，实际工作退出前仍保护 head。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_workspace_execution_cancels_without_releasing_the_running_callback() {
    let fixture = Fixture::new().await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let stopped = Arc::new(tokio::sync::Notify::new());
    let (release, released) = mpsc::channel();
    let (lifecycle, review, snapshot) = (
        fixture.lifecycle.clone(),
        fixture.presentation.review_id.clone(),
        fixture.presentation.snapshot_id.clone(),
    );
    let (worker_entered, worker_stopped) = (entered.clone(), stopped.clone());
    let running = tokio::spawn(async move {
        lifecycle
            .run_in_workspace(
                &OperationContext::background(),
                &review,
                &snapshot,
                move |workspace| {
                    worker_entered.notify_one();
                    let deadline = std::time::Instant::now() + Duration::from_secs(5);
                    while !workspace.operation.cancellation().is_cancelled() {
                        assert!(std::time::Instant::now() < deadline);
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    worker_stopped.notify_one();
                    released.recv_timeout(Duration::from_secs(5)).unwrap();
                    assert!(workspace.head_path.join("lib.rs").is_file());
                    Ok::<_, ReviewLifecycleError>(())
                },
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    running.abort();
    assert!(running.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(5), stopped.notified())
        .await
        .unwrap();
    let mut closing = Box::pin(fixture.lifecycle.close(&fixture.presentation.review_id));
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut closing)
            .await
            .is_err()
    );
    let error = fixture
        .lifecycle
        .run_in_workspace(
            &OperationContext::background(),
            &fixture.presentation.review_id,
            &fixture.presentation.snapshot_id,
            |_| -> Result<(), ReviewLifecycleError> {
                panic!("关闭过程中不应接纳新工作")
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, ReviewLifecycleError::Busy));
    assert!(fixture.head.exists());
    release.send(()).unwrap();
    tokio::time::timeout(Duration::from_secs(5), closing)
        .await
        .unwrap()
        .unwrap();
    assert!(!fixture.head.exists());
}

/// 同一 Review 的执行串行，等待者取消不启动回调、不取消正在使用目录的兄弟操作。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn serializes_workspace_callbacks_and_cancels_only_the_waiting_operation() {
    let fixture = Fixture::new().await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let (release, released) = mpsc::channel();
    let (lifecycle, review, snapshot) = (
        fixture.lifecycle.clone(),
        fixture.presentation.review_id.clone(),
        fixture.presentation.snapshot_id.clone(),
    );
    let worker_entered = entered.clone();
    let running = tokio::spawn(async move {
        lifecycle
            .run_in_workspace(
                &OperationContext::background(),
                &review,
                &snapshot,
                move |workspace| {
                    worker_entered.notify_one();
                    released.recv_timeout(Duration::from_secs(5)).unwrap();
                    assert!(!workspace.operation.cancellation().is_cancelled());
                    Ok::<_, ReviewLifecycleError>(workspace.comparison.head_revision.clone())
                },
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    let cancellation = CancellationToken::new();
    let operation = OperationContext::new(cancellation.clone());
    let mut waiting = Box::pin(fixture.lifecycle.run_in_workspace(
        &operation,
        &fixture.presentation.review_id,
        &fixture.presentation.snapshot_id,
        |_| -> Result<(), ReviewLifecycleError> { panic!("等待阶段取消不应启动回调") },
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(30), &mut waiting)
            .await
            .is_err()
    );
    cancellation.cancel();
    assert!(matches!(
        tokio::time::timeout(Duration::from_secs(1), waiting)
            .await
            .unwrap(),
        Err(ReviewLifecycleError::Cancelled(_))
    ));
    let background = OperationContext::background();
    let mut next = Box::pin(fixture.lifecycle.run_in_workspace(
        &background,
        &fixture.presentation.review_id,
        &fixture.presentation.snapshot_id,
        |workspace| Ok::<_, ReviewLifecycleError>(workspace.comparison.head_revision.clone()),
    ));
    assert!(tokio::time::timeout(Duration::from_millis(30), &mut next)
        .await
        .is_err());
    release.send(()).unwrap();
    assert_eq!(
        running.await.unwrap().unwrap(),
        fixture.presentation.snapshot.head_revision
    );
    assert_eq!(
        next.await.unwrap(),
        fixture.presentation.snapshot.head_revision
    );
}

/// 新入口直接保留 Snapshot 和关闭状态校验，成功返回前拒绝外部改写的 head。
#[tokio::test]
async fn workspace_execution_checks_snapshot_head_and_shutdown() {
    let fixture = Fixture::new().await;
    let error = fixture
        .lifecycle
        .run_in_workspace(
            &OperationContext::background(),
            &fixture.presentation.review_id,
            "obsolete",
            |_| -> Result<(), ReviewLifecycleError> {
                panic!("过期 Snapshot 不应执行工作")
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, ReviewLifecycleError::StaleSnapshot));
    let error = fixture
        .lifecycle
        .run_in_workspace(
            &OperationContext::background(),
            &fixture.presentation.review_id,
            &fixture.presentation.snapshot_id,
            |workspace| {
                std::fs::write(
                    workspace.head_path.join("lib.rs"),
                    "fn changed_externally() {}",
                )
                .unwrap();
                Ok::<_, ReviewLifecycleError>(())
            },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, ReviewLifecycleError::Git(_)));
    fixture.lifecycle.shutdown().await.unwrap();
    let error = fixture
        .lifecycle
        .run_in_workspace(
            &OperationContext::background(),
            &fixture.presentation.review_id,
            &fixture.presentation.snapshot_id,
            |_| -> Result<(), ReviewLifecycleError> { panic!("退出后不应执行工作") },
        )
        .await
        .unwrap_err();
    assert!(matches!(error, ReviewLifecycleError::ShuttingDown));
}

/// 真实错误与用户取消同时到达仍保留根因；明确的纯取消维持取消类别。
#[tokio::test]
async fn preserves_failure_when_user_cancellation_arrives_at_the_same_time() {
    for fails in [false, true] {
        let fixture = Fixture::new().await;
        let cancellation = CancellationToken::new();
        let operation = OperationContext::new(cancellation.clone());
        let application = StructureReview::new(Arc::new(move |input, cancelled, _| {
            if input.repository_root.ends_with("before") {
                cancellation.cancel();
                return Err(if fails {
                    AnalysisError::new("original failure")
                } else {
                    AnalysisError::cancelled("requested cancellation")
                });
            }
            wait_for_cancellation(&cancelled);
            Err(AnalysisError::cancelled("peer cancelled"))
        }));
        let error = application
            .analyze(
                &fixture.lifecycle,
                &operation,
                &fixture.presentation.review_id,
                &fixture.presentation.snapshot_id,
                AnalysisOptions::default(),
                RustDependencyScope::default(),
            )
            .await
            .unwrap_err();
        assert_eq!(
            error.code,
            if fails {
                "analysis-failed"
            } else {
                "cancelled"
            }
        );
        if fails {
            assert_eq!(error.detail, "original failure");
        }
        assert!(!fixture.head.parent().unwrap().join("before").exists());
    }
}

/// 注入真实目录所有权变化，分析失败或 panic 后都必须同时报告根因与清理失败。
#[cfg(unix)]
#[tokio::test]
async fn reports_cleanup_failure_together_with_analysis_error_or_panic() {
    for panic in [false, true] {
        let fixture = Fixture::new().await;
        let foreign = tempfile::tempdir().unwrap();
        std::fs::write(foreign.path().join("keep"), "unrelated").unwrap();
        let foreign_path = foreign.path().to_owned();
        let before = fixture.head.parent().unwrap().join("before");
        let saved_before = before.with_extension("saved");
        let application = StructureReview::new(Arc::new(move |input, cancelled, _| {
            if input.repository_root.ends_with("before") {
                std::fs::rename(
                    &input.repository_root,
                    input.repository_root.with_extension("saved"),
                )
                .unwrap();
                std::os::unix::fs::symlink(&foreign_path, &input.repository_root).unwrap();
                assert!(!panic, "original panic");
                return Err(AnalysisError::new("original failure"));
            }
            wait_for_cancellation(&cancelled);
            Err(AnalysisError::cancelled("peer cancelled"))
        }));
        let error = application
            .analyze(
                &fixture.lifecycle,
                &OperationContext::background(),
                &fixture.presentation.review_id,
                &fixture.presentation.snapshot_id,
                AnalysisOptions::default(),
                RustDependencyScope::default(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "cleanup-failed");
        assert!(error.detail.contains("所有权验证失败"));
        assert!(error.detail.contains(if panic {
            "original panic"
        } else {
            "original failure"
        }));
        assert_eq!(
            std::fs::read_to_string(foreign.path().join("keep")).unwrap(),
            "unrelated"
        );
        assert!(fixture.head.exists());
        std::fs::remove_file(&before).unwrap();
        std::fs::rename(&saved_before, &before).unwrap();
        fixture
            .lifecycle
            .close(&fixture.presentation.review_id)
            .await
            .unwrap();
        assert!(!before.exists());
    }
}

/// 测试 Adapter 有界等待协作停止，避免错误路径让测试进程永久挂起。
fn wait_for_cancellation(cancelled: &CancellationCheck) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !cancelled() {
        assert!(std::time::Instant::now() < deadline, "另一侧必须收到取消");
        std::thread::sleep(Duration::from_millis(5));
    }
}
