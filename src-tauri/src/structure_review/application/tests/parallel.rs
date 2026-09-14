use super::*;
use std::sync::{atomic::AtomicUsize, Condvar};

/// 有界屏障要求两个真实分析入口重叠；串行实现会超时失败。
#[tokio::test]
async fn analyzes_both_sides_concurrently_after_preparing_their_own_workspace() {
    let fixture = Fixture::new().await;
    let started = Arc::new((Mutex::new(0), Condvar::new()));
    let captured = started.clone();
    let application = StructureReview::new(Arc::new(move |input, cancelled, scope| {
        assert!(input.repository_root.join("lib.rs").is_file());
        let (count, changed) = &*captured;
        let mut count = count.lock().unwrap();
        *count += 1;
        changed.notify_all();
        let (count, timeout) = changed
            .wait_timeout_while(count, Duration::from_secs(5), |count| *count < 2)
            .unwrap();
        drop(count);
        assert!(!timeout.timed_out(), "前后两侧分析必须重叠");
        analyze_files(input, cancelled, scope)
    }));
    application
        .analyze(
            &fixture.lifecycle,
            &OperationContext::background(),
            &fixture.presentation.review_id,
            &fixture.presentation.snapshot_id,
            AnalysisOptions::default(),
            RustDependencyScope::StandardLibrary,
        )
        .await
        .unwrap();
    assert_eq!(*started.0.lock().unwrap(), 2);
    assert!(fixture.head.exists());
    assert!(!fixture.head.parent().unwrap().join("before").exists());
}

/// 任一侧失败或 panic 都会停止另一侧；返回前必须排空 worker 并回收 before。
#[tokio::test]
async fn failure_and_panic_cancel_the_peer_and_preserve_the_head() {
    for failed_side in ["before", "after"] {
        for panic in [false, true] {
            let fixture = Fixture::new().await;
            let started = Arc::new((Mutex::new(0), Condvar::new()));
            let captured = started.clone();
            let finished = Arc::new(AtomicUsize::new(0));
            let observed = finished.clone();
            let application = StructureReview::new(Arc::new(move |input, cancelled, _| {
                let (count, changed) = &*captured;
                let mut count = count.lock().unwrap();
                *count += 1;
                changed.notify_all();
                let (count, timeout) = changed
                    .wait_timeout_while(count, Duration::from_secs(5), |count| *count < 2)
                    .unwrap();
                drop(count);
                assert!(!timeout.timed_out());
                let is_before = input.repository_root.ends_with("before");
                if is_before == (failed_side == "before") {
                    assert!(!panic, "模拟分析器异常");
                    return Err(AnalysisError::new("original failure"));
                }
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                while !cancelled() && std::time::Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(5));
                }
                assert!(cancelled(), "失败必须通知仍在运行的另一侧");
                observed.fetch_add(1, Ordering::Release);
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
            assert_eq!(error.code, "analysis-failed");
            if !panic {
                assert_eq!(error.detail, "original failure");
            }
            assert_eq!(finished.load(Ordering::Acquire), 1);
            assert!(fixture.head.exists());
            assert!(!fixture.head.parent().unwrap().join("before").exists());
            // 失败结果返回后 lease 已释放，关闭不会再等待一个遗留 worker。
            fixture
                .lifecycle
                .close(&fixture.presentation.review_id)
                .await
                .unwrap();
        }
    }
}
