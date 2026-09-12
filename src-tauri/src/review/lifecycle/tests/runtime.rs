/// 验证 A→B→A 保留每个 Open Review 的 Reader 与 Workspace，并让重复 present 无副作用。
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
        .open_review_runtime(&first.review_id)
        .unwrap()
        .unwrap()
        .reader;
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
        .open_review_runtime(&second.review_id)
        .unwrap()
        .unwrap()
        .reader;

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
    let git = GitProcessAdapter::new(fixture.repository.path());
    assert!(git
        .is_registered_worktree(
            &first_workspace,
            OperationContext::background().cancellation()
        )
        .unwrap());
    assert!(git
        .is_registered_worktree(
            &second_workspace,
            OperationContext::background().cancellation()
        )
        .unwrap());

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

    let repository_path = fixture.repository.path().to_path_buf();
    let unavailable_repository = fixture
        ._application_data
        .path()
        .join("repository-unavailable");
    std::fs::rename(&repository_path, &unavailable_repository).unwrap();
    let warm_present = lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: first.review_id.clone(),
            },
        )
        .await;
    std::fs::rename(&unavailable_repository, &repository_path).unwrap();
    let warm_present = warm_present.unwrap();
    assert!(warm_present.cleanup_warnings.is_empty());
    let first_reader_again = lifecycle
        .open_review_runtime(&first.review_id)
        .unwrap()
        .unwrap()
        .reader;
    let second_reader_again = lifecycle
        .open_review_runtime(&second.review_id)
        .unwrap()
        .unwrap()
        .reader;
    assert!(Arc::ptr_eq(&first_reader, &first_reader_again));
    assert!(Arc::ptr_eq(&second_reader, &second_reader_again));
    assert!(first_workspace.exists());
    assert!(second_workspace.exists());
    let runtime = lifecycle.runtime.lock().unwrap();
    assert_eq!(runtime.open_reviews.len(), 2);
    assert_eq!(
        runtime.presented_review_id.as_deref(),
        Some(first.review_id.as_str())
    );
}

/// 验证过期失败恢复不会覆盖其后完成、最终又指向相同 Review 的 presentation。
#[tokio::test]
async fn stale_presentation_restore_cannot_overwrite_newer_successful_presents() {
    let fixture = LifecycleFixture::new();
    run_git(
        fixture.repository.path(),
        &["branch", "alternate", "refs/heads/topic"],
    );
    let first = fixture.save_current_review().await;
    let second = fixture
        .save_review_with_refs("refs/heads/topic", "refs/heads/base")
        .await;
    let third = fixture
        .save_review_with_refs("refs/heads/base", "refs/heads/alternate")
        .await;
    for review in [&first, &second, &third] {
        fixture.store.activate_initial_snapshot(review).unwrap();
    }
    let lifecycle = ReviewLifecycle::new(fixture.store.clone());
    for review in [&second, &third, &first] {
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

    let operation_guard = lifecycle.enter_operation().await.unwrap();
    let stale_transition = lifecycle
        .present_guarded(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: second.review_id.clone(),
            },
        )
        .await
        .unwrap()
        .transition;
    drop(operation_guard);
    lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: third.review_id.clone(),
            },
        )
        .await
        .unwrap();
    lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: second.review_id.clone(),
            },
        )
        .await
        .unwrap();

    lifecycle.restore_presentation(stale_transition).unwrap();

    assert_eq!(
        lifecycle
            .runtime
            .lock()
            .unwrap()
            .presented_review_id
            .as_deref(),
        Some(second.review_id.as_str())
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

    let outcome = lifecycle.close(&saved.review_id).await.unwrap();

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
    assert_eq!(
        content.after.as_ref().and_then(FileContent::text),
        Some("refreshed\n")
    );
}
