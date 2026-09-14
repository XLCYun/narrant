/// 验证 shutdown 等待正在收敛的 refresh 激活完成，再释放目标 Workspace。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_waits_for_refresh_convergence_before_workspace_cleanup() {
    let fixture = LifecycleFixture::new();
    run_git(fixture.repository.path(), &["checkout", "-q", "topic"]);
    std::fs::write(fixture.repository.path().join("old-only.txt"), "old\n").unwrap();
    run_git(fixture.repository.path(), &["add", "old-only.txt"]);
    run_git(
        fixture.repository.path(),
        &["commit", "-q", "-m", "old shutdown workspace"],
    );
    let saved = fixture.save_current_review().await;
    fixture.store.activate_initial_snapshot(&saved).unwrap();
    let lifecycle = Arc::new(ReviewLifecycle::new(fixture.store.clone()));
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
    run_git(fixture.repository.path(), &["add", "."]);
    run_git(
        fixture.repository.path(),
        &["commit", "-q", "-m", "shutdown refresh"],
    );
    let update = lifecycle
        .check_update(&OperationContext::background(), &saved.review_id)
        .await
        .unwrap();
    let confirmation = ConfirmedReviewUpdate {
        current_base_revision: update.current_base_revision.clone(),
        current_head_revision: update.current_head_revision.clone(),
        available_base_revision: update.available_base_revision.clone(),
        available_head_revision: update.available_head_revision.clone(),
    };
    let rejected_confirmation = ConfirmedReviewUpdate {
        current_base_revision: update.current_base_revision,
        current_head_revision: update.current_head_revision,
        available_base_revision: update.available_base_revision,
        available_head_revision: update.available_head_revision,
    };
    let workspace = fixture
        .store
        .review_workspace_path(&saved.review_id)
        .unwrap();
    let barrier = Arc::new(LifecycleOperationGateBarrier::new());
    lifecycle
        .install_operation_gate_barrier(barrier.clone())
        .unwrap();
    let refresh_lifecycle = lifecycle.clone();
    let refresh_review_id = saved.review_id.clone();
    let refresh_task = tokio::spawn(async move {
        refresh_lifecycle
            .refresh(
                &OperationContext::background(),
                &refresh_review_id,
                confirmation,
            )
            .await
    });
    barrier.wait_until_operation_paused().await;

    let shutdown_lifecycle = lifecycle.clone();
    let shutdown_task = tokio::spawn(async move { shutdown_lifecycle.shutdown().await });
    while !lifecycle.is_shutting_down() {
        tokio::task::yield_now().await;
    }
    assert!(
        !shutdown_task.is_finished(),
        "refresh 持有 read gate 时 shutdown 不得越过 write gate"
    );
    let rejected = match lifecycle
        .refresh(
            &OperationContext::background(),
            &saved.review_id,
            rejected_confirmation,
        )
        .await
    {
        Ok(_) => panic!("shutdown 开始后不应接受新的 refresh"),
        Err(error) => error,
    };
    assert!(matches!(rejected, ReviewLifecycleError::ShuttingDown));
    barrier.release_operation().await;
    let refreshed = refresh_task.await.unwrap().unwrap();
    let warnings = shutdown_task.await.unwrap().unwrap();

    assert!(warnings.is_empty());
    assert_eq!(
        fixture
            .store
            .open_review(&saved.review_id)
            .unwrap()
            .snapshot_identity(),
        refreshed.snapshot_id
    );
    assert!(!workspace.exists());
    assert!(lifecycle
        .open_review_runtime(&saved.review_id)
        .unwrap()
        .is_none());
}

/// 验证刷新一个 Open Review 只替换目标 Reader 与 Workspace binding。
#[tokio::test]
async fn refreshing_one_open_review_replaces_only_its_runtime_reader_and_workspace() {
    let fixture = LifecycleFixture::new();
    let first = fixture.save_current_review().await;
    let second = fixture
        .save_review_with_refs("refs/heads/topic", "refs/heads/base")
        .await;
    fixture.store.activate_initial_snapshot(&first).unwrap();
    fixture.store.activate_initial_snapshot(&second).unwrap();
    let lifecycle = ReviewLifecycle::new(fixture.store.clone());
    for review in [&first, &second] {
        lifecycle
            .present(
                &OperationContext::background(),
                PresentReview::Saved {
                    review_id: review.review_id.clone(),
                },
            )
            .await
            .unwrap();
    }
    let second_reader = lifecycle
        .open_review_runtime(&second.review_id)
        .unwrap()
        .unwrap()
        .reader;
    let second_workspace = fixture
        .store
        .review_workspace_path(&second.review_id)
        .unwrap();
    let second_workspace_head = git_text(&second_workspace, &["rev-parse", "HEAD"]);

    run_git(fixture.repository.path(), &["checkout", "-q", "topic"]);
    std::fs::write(fixture.repository.path().join("README.md"), "refreshed\n").unwrap();
    run_git(
        fixture.repository.path(),
        &["commit", "-q", "-am", "refresh first review"],
    );
    let update = lifecycle
        .check_update(&OperationContext::background(), &first.review_id)
        .await
        .unwrap();
    let refreshed = lifecycle
        .refresh(
            &OperationContext::background(),
            &first.review_id,
            ConfirmedReviewUpdate {
                current_base_revision: update.current_base_revision,
                current_head_revision: update.current_head_revision,
                available_base_revision: update.available_base_revision,
                available_head_revision: update.available_head_revision,
            },
        )
        .await
        .unwrap();

    assert_ne!(refreshed.snapshot_id, first.snapshot_identity());
    assert_eq!(
        git_text(&second_workspace, &["rev-parse", "HEAD"]),
        second_workspace_head
    );
    let second_reader_after = lifecycle
        .open_review_runtime(&second.review_id)
        .unwrap()
        .unwrap()
        .reader;
    assert!(Arc::ptr_eq(&second_reader, &second_reader_after));
}

/// 验证复合刷新失败后仍保留已物化 Review，并恢复此前展示状态。
#[tokio::test]
async fn failed_reopen_and_refresh_retains_materialized_review_runtime() {
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
    assert!(workspace.exists());
    let content = lifecycle
        .read_file_change(
            &OperationContext::background(),
            &saved.review_id,
            &saved.snapshot_identity(),
            "README.md",
        )
        .await
        .unwrap();
    assert_eq!(content.path, "README.md");
    assert!(lifecycle
        .runtime
        .lock()
        .unwrap()
        .presented_review_id
        .is_none());
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
    let cancellation = CancellationToken::new();
    let operation = OperationContext::new(cancellation.clone());
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
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let operation = OperationContext::new(cancellation.clone());

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

#[tokio::test]
async fn cancellation_after_candidate_staging_abandons_candidate_before_workspace_transition() {
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
    let workspace = fixture
        .store
        .review_workspace_path(&saved.review_id)
        .unwrap();
    let workspace_head = git_text(&workspace, &["rev-parse", "HEAD"]);
    let candidate = fixture
        .store
        .stage_branch_refresh_candidate(&saved.review_id, &saved.to_change_comparison())
        .unwrap();
    let cancellation = CancellationToken::new();
    cancellation.cancel();
    let operation = OperationContext::new(cancellation.clone());

    let error = lifecycle
        .cancel_staged_refresh(&operation, &candidate)
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
    assert_eq!(git_text(&workspace, &["rev-parse", "HEAD"]), workspace_head);
    assert_eq!(
        fixture
            .store
            .list_review_snapshot_identities(&saved.review_id)
            .unwrap(),
        vec![current.snapshot_id]
    );
}
