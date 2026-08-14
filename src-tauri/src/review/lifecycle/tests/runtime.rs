#[tokio::test]
async fn close_is_logical_success_when_workspace_cleanup_must_be_deferred() {
    let fixture = LifecycleFixture::new();
    let lifecycle = ReviewLifecycle::new(fixture.catalog.clone());
    let saved = fixture.save_current_review().await;
    fixture.catalog.activate_snapshot(&saved).unwrap();
    let presentation = lifecycle
        .present(
            PresentReview::Saved {
                review_id: saved.review_id.clone(),
            },
            &CancellationToken::default(),
        )
        .await
        .unwrap();
    let workspace = fixture
        .catalog
        .review_workspace_path(&saved.review_id)
        .unwrap();
    Command::new("git")
        .args(["worktree", "remove", "--force", "--"])
        .arg(&workspace)
        .current_dir(fixture.repository.path())
        .status()
        .unwrap();
    std::fs::create_dir_all(&workspace).unwrap();

    let outcome = lifecycle.close(&saved.review_id).unwrap();

    assert!(matches!(
        outcome.workspace_cleanup,
        WorkspaceCleanup::Deferred(_)
    ));
    let error = lifecycle
        .read_file_change(
            &saved.review_id,
            &presentation.snapshot_id,
            "README.md",
            &CancellationToken::default(),
        )
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "Review 已关闭或失效，请重新打开。");
}

#[tokio::test]
async fn recovery_abandons_unactivated_candidate_without_adding_history() {
    let fixture = LifecycleFixture::new();
    let saved = fixture.save_current_review().await;
    fixture.catalog.activate_snapshot(&saved).unwrap();
    let candidate = fixture
        .catalog
        .save_refresh_snapshot(&saved.review_id, &saved.snapshot())
        .unwrap();
    let lifecycle = ReviewLifecycle::new(fixture.catalog.clone());

    let report = lifecycle.recover().unwrap();

    assert_eq!(
        report.abandoned_snapshot_ids,
        vec![candidate.snapshot_identity()]
    );
    assert_eq!(
        fixture
            .catalog
            .list_review_snapshot_identities(&saved.review_id)
            .unwrap(),
        vec![saved.snapshot_identity()]
    );
}

#[tokio::test]
async fn branch_refresh_converges_catalog_workspace_and_runtime_reader() {
    let fixture = LifecycleFixture::new();
    let saved = fixture.save_current_review().await;
    fixture.catalog.activate_snapshot(&saved).unwrap();
    let lifecycle = ReviewLifecycle::new(fixture.catalog.clone());
    let current = lifecycle
        .present(
            PresentReview::Saved {
                review_id: saved.review_id.clone(),
            },
            &CancellationToken::default(),
        )
        .await
        .unwrap();
    run_git(fixture.repository.path(), &["checkout", "-q", "topic"]);
    std::fs::write(fixture.repository.path().join("README.md"), "refreshed\n").unwrap();
    run_git(
        fixture.repository.path(),
        &["commit", "-q", "-am", "refresh"],
    );
    let update = lifecycle
        .check_update(&saved.review_id, &CancellationToken::default())
        .await
        .unwrap();

    let refreshed = lifecycle
        .refresh(
            &saved.review_id,
            ConfirmedReviewUpdate {
                current_base_revision: update.current_base_revision,
                current_head_revision: update.current_head_revision,
                available_base_revision: update.available_base_revision,
                available_head_revision: update.available_head_revision,
            },
            &CancellationToken::default(),
        )
        .await
        .unwrap();

    assert_ne!(refreshed.snapshot_id, current.snapshot_id);
    assert_eq!(
        fixture
            .catalog
            .open_review(&saved.review_id)
            .unwrap()
            .snapshot_identity(),
        refreshed.snapshot_id
    );
    let workspace = fixture
        .catalog
        .review_workspace_path(&saved.review_id)
        .unwrap();
    assert_eq!(
        git_text(&workspace, &["rev-parse", "HEAD"]),
        refreshed.snapshot.head_revision
    );
    let stale_error = lifecycle
        .read_file_change(
            &saved.review_id,
            &current.snapshot_id,
            "README.md",
            &CancellationToken::default(),
        )
        .await
        .unwrap_err();
    assert!(matches!(stale_error, ReviewLifecycleError::StaleSnapshot));
    let content = lifecycle
        .read_file_change(
            &saved.review_id,
            &refreshed.snapshot_id,
            "README.md",
            &CancellationToken::default(),
        )
        .await
        .unwrap();
    assert_eq!(content.after.as_ref().and_then(FileContent::text), Some("refreshed\n"));
}
