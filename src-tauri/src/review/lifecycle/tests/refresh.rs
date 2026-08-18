#[tokio::test]
async fn failed_reopen_and_refresh_cleans_newly_materialized_review() {
    let fixture = LifecycleFixture::new();
    let saved = fixture.save_current_review().await;
    fixture.store.activate_initial_snapshot(&saved).unwrap();
    let lifecycle = ReviewLifecycle::new(fixture.store.clone());

    let error = match lifecycle
        .reopen_and_refresh(
            &OperationContext::background(),
            &saved.review_id,
            ConfirmedReviewUpdate {
                current_base_revision: "stale-base".to_owned(),
                current_head_revision: "stale-head".to_owned(),
                available_base_revision: "next-base".to_owned(),
                available_head_revision: "next-head".to_owned(),
            },
        )
        .await
    {
        Ok(_) => panic!("过期确认不应刷新成功"),
        Err(error) => error,
    };

    assert!(matches!(error, ReviewLifecycleError::StaleCurrent));
    let workspace = fixture
        .store
        .review_workspace_path(&saved.review_id)
        .unwrap();
    assert!(!workspace.exists());
    let read_error = lifecycle
        .read_file_change(
            &OperationContext::background(),
            &saved.review_id,
            &saved.snapshot_identity(),
            "README.md",
        )
        .await
        .unwrap_err();
    assert!(matches!(
        read_error,
        ReviewLifecycleError::ReviewUnavailable(_)
    ));
}
#[tokio::test]
async fn cancellation_after_workspace_mutation_does_not_interrupt_convergence() {
    let fixture = LifecycleFixture::new();
    run_git(fixture.repository.path(), &["checkout", "-q", "topic"]);
    std::fs::write(fixture.repository.path().join("old-only.txt"), "old\n").unwrap();
    run_git(fixture.repository.path(), &["add", "old-only.txt"]);
    run_git(
        fixture.repository.path(),
        &["commit", "-q", "-m", "old workspace"],
    );
    let saved = fixture.save_current_review().await;
    fixture.store.activate_initial_snapshot(&saved).unwrap();
    let lifecycle = ReviewLifecycle::new(fixture.store.clone());
    lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: saved.review_id.clone(),
            },
        )
        .await
        .unwrap();
    std::fs::remove_file(fixture.repository.path().join("old-only.txt")).unwrap();
    for index in 0..1_000 {
        std::fs::write(
            fixture
                .repository
                .path()
                .join(format!("refresh-{index}.txt")),
            format!("{index}\n"),
        )
        .unwrap();
    }
    run_git(fixture.repository.path(), &["add", "."]);
    run_git(
        fixture.repository.path(),
        &["commit", "-q", "-m", "large refresh"],
    );
    let update = lifecycle
        .check_update(&OperationContext::background(), &saved.review_id)
        .await
        .unwrap();
    let workspace = fixture
        .store
        .review_workspace_path(&saved.review_id)
        .unwrap();
    let cancellation = crate::operation::TestCancellation::default();
    let operation = OperationContext::from_test_cancellation(&cancellation);
    let cancellation_request = cancellation.clone();
    let refresh_completed = Arc::new(AtomicBool::new(false));
    let completion_observer = refresh_completed.clone();
    let old_workspace_file = workspace.join("old-only.txt");
    let observer = thread::spawn(move || {
        for _ in 0..5_000 {
            if !old_workspace_file.exists() {
                cancellation_request.cancel();
                return !completion_observer.load(Ordering::Acquire);
            }
            thread::sleep(Duration::from_millis(1));
        }
        false
    });

    let refresh_result = lifecycle
        .refresh(
            &operation,
            &saved.review_id,
            ConfirmedReviewUpdate {
                current_base_revision: update.current_base_revision,
                current_head_revision: update.current_head_revision,
                available_base_revision: update.available_base_revision,
                available_head_revision: update.available_head_revision,
            },
        )
        .await;
    refresh_completed.store(true, Ordering::Release);
    let refreshed = refresh_result.unwrap();

    assert!(
        observer.join().unwrap(),
        "取消未发生在 Workspace convergence 完成之前"
    );
    assert!(cancellation.is_cancelled());
    assert_eq!(
        fixture
            .store
            .open_review(&saved.review_id)
            .unwrap()
            .snapshot_identity(),
        refreshed.snapshot_id
    );
    assert_eq!(
        git_text(&workspace, &["rev-parse", "HEAD"]),
        refreshed.snapshot.head_revision
    );
}
#[tokio::test]
async fn cancelled_update_check_keeps_current_snapshot_and_workspace() {
    let fixture = LifecycleFixture::new();
    let saved = fixture.save_current_review().await;
    fixture.store.activate_initial_snapshot(&saved).unwrap();
    let lifecycle = ReviewLifecycle::new(fixture.store.clone());
    let current = lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: saved.review_id.clone(),
            },
        )
        .await
        .unwrap();
    let cancellation = crate::operation::TestCancellation::default();
    cancellation.cancel();
    let operation = OperationContext::from_test_cancellation(&cancellation);

    let error = lifecycle
        .check_update(&operation, &saved.review_id)
        .await
        .unwrap_err();

    assert!(matches!(error, ReviewLifecycleError::Cancelled(_)));
    assert_eq!(
        fixture
            .store
            .open_review(&saved.review_id)
            .unwrap()
            .snapshot_identity(),
        current.snapshot_id
    );
    let workspace = fixture
        .store
        .review_workspace_path(&saved.review_id)
        .unwrap();
    assert_eq!(
        git_text(&workspace, &["rev-parse", "HEAD"]),
        current.snapshot.head_revision
    );
}
