#[tokio::test]
async fn presenting_multiple_reviews_retains_each_runtime_and_reuses_reader() {
    let fixture = LifecycleFixture::new();
    let first = fixture.save_current_review().await;
    let second = fixture
        .save_review_with_refs("refs/heads/topic", "refs/heads/base")
        .await;
    fixture.store.activate_initial_snapshot(&first).unwrap();
    fixture.store.activate_initial_snapshot(&second).unwrap();
    let lifecycle = ReviewLifecycle::new(fixture.store.clone());

    lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: first.review_id.clone(),
            },
        )
        .await
        .unwrap();
    let first_reader = lifecycle
        .runtime_review(&first.review_id)
        .unwrap()
        .unwrap()
        .1;
    lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: second.review_id.clone(),
            },
        )
        .await
        .unwrap();
    let second_reader = lifecycle
        .runtime_review(&second.review_id)
        .unwrap()
        .unwrap()
        .1;

    let first_workspace = fixture
        .store
        .review_workspace_path(&first.review_id)
        .unwrap();
    let second_workspace = fixture
        .store
        .review_workspace_path(&second.review_id)
        .unwrap();
    assert!(first_workspace.exists());
    assert!(second_workspace.exists());
    let worktree_list = git_text(
        fixture.repository.path(),
        &["worktree", "list", "--porcelain"],
    );
    assert!(worktree_list.contains(&first_workspace.to_string_lossy().to_string()));
    assert!(worktree_list.contains(&second_workspace.to_string_lossy().to_string()));

    let before_duplicate = fixture
        .store
        .list_reviews(None)
        .unwrap()
        .into_iter()
        .find(|review| review.review_id == second.review_id)
        .unwrap()
        .last_opened_at_millis;
    lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: second.review_id.clone(),
            },
        )
        .await
        .unwrap();
    let after_duplicate = fixture
        .store
        .list_reviews(None)
        .unwrap()
        .into_iter()
        .find(|review| review.review_id == second.review_id)
        .unwrap()
        .last_opened_at_millis;
    assert_eq!(before_duplicate, after_duplicate);

    lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: first.review_id.clone(),
            },
        )
        .await
        .unwrap();
    let first_reader_again = lifecycle
        .runtime_review(&first.review_id)
        .unwrap()
        .unwrap()
        .1;
    let second_reader_again = lifecycle
        .runtime_review(&second.review_id)
        .unwrap()
        .unwrap()
        .1;
    assert!(Arc::ptr_eq(&first_reader, &first_reader_again));
    assert!(Arc::ptr_eq(&second_reader, &second_reader_again));
    let runtime = lifecycle.runtime.lock().unwrap();
    assert_eq!(runtime.reviews.len(), 2);
    assert_eq!(
        runtime.presented_review_id.as_deref(),
        Some(first.review_id.as_str())
    );
}

#[tokio::test]
async fn close_is_logical_success_when_workspace_cleanup_must_be_deferred() {
    let fixture = LifecycleFixture::new();
    let lifecycle = ReviewLifecycle::new(fixture.store.clone());
    let saved = fixture.save_current_review().await;
    fixture.store.activate_initial_snapshot(&saved).unwrap();
    let presentation = lifecycle
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
            &OperationContext::background(),
            &saved.review_id,
            &presentation.snapshot_id,
            "README.md",
        )
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "Review 已关闭或失效，请重新打开。");
}

#[tokio::test]
async fn recovery_abandons_unactivated_candidate_without_adding_history() {
    let fixture = LifecycleFixture::new();
    let saved = fixture.save_current_review().await;
    fixture.store.activate_initial_snapshot(&saved).unwrap();
    let candidate = fixture
        .store
        .stage_branch_refresh_candidate(&saved.review_id, &saved.to_change_comparison())
        .unwrap();
    let lifecycle = ReviewLifecycle::new(fixture.store.clone());

    let report = lifecycle.recover().unwrap();

    assert_eq!(
        report.abandoned_snapshot_ids,
        vec![candidate.snapshot_identity()]
    );
    assert_eq!(
        fixture
            .store
            .list_review_snapshot_identities(&saved.review_id)
            .unwrap(),
        vec![saved.snapshot_identity()]
    );
}

#[tokio::test]
async fn branch_refresh_converges_persistence_workspace_and_runtime_reader() {
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
    run_git(fixture.repository.path(), &["checkout", "-q", "topic"]);
    std::fs::write(fixture.repository.path().join("README.md"), "refreshed\n").unwrap();
    run_git(
        fixture.repository.path(),
        &["commit", "-q", "-am", "refresh"],
    );
    let update = lifecycle
        .check_update(&OperationContext::background(), &saved.review_id)
        .await
        .unwrap();

    let refreshed = lifecycle
        .refresh(
            &OperationContext::background(),
            &saved.review_id,
            ConfirmedReviewUpdate {
                current_base_revision: update.current_base_revision,
                current_head_revision: update.current_head_revision,
                available_base_revision: update.available_base_revision,
                available_head_revision: update.available_head_revision,
            },
        )
        .await
        .unwrap();

    assert_ne!(refreshed.snapshot_id, current.snapshot_id);
    assert_eq!(
        fixture
            .store
            .open_review(&saved.review_id)
            .unwrap()
            .snapshot_identity(),
        refreshed.snapshot_id
    );
    let workspace = fixture
        .store
        .review_workspace_path(&saved.review_id)
        .unwrap();
    assert_eq!(
        git_text(&workspace, &["rev-parse", "HEAD"]),
        refreshed.snapshot.head_revision
    );
    let stale_error = lifecycle
        .read_file_change(
            &OperationContext::background(),
            &saved.review_id,
            &current.snapshot_id,
            "README.md",
        )
        .await
        .unwrap_err();
    assert!(matches!(stale_error, ReviewLifecycleError::StaleSnapshot));
    let content = lifecycle
        .read_file_change(
            &OperationContext::background(),
            &saved.review_id,
            &refreshed.snapshot_id,
            "README.md",
        )
        .await
        .unwrap();
    assert_eq!(content.after.as_ref().and_then(FileContent::text), Some("refreshed\n"));
}
