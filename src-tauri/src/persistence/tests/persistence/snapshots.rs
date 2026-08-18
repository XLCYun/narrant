#[test]
fn refresh_candidate_stays_non_current_until_explicit_activation() {
    let application_data = tempfile::TempDir::new().unwrap();
    let database_path = application_data.path().join("store.sqlite3");
    let repository = create_repository();
    let store = ReviewStore::open(&database_path).unwrap();
    let entry = store.upsert_repository(&verify_repository_location(repository.path()).unwrap()).unwrap();
    store.insert_workspace_trust(entry.id).unwrap();
    for arguments in [
        ["branch", "base"],
        ["branch", "topic"],
        ["checkout", "topic"],
    ] {
        assert!(Command::new("git")
            .args(arguments)
            .current_dir(repository.path())
            .status()
            .unwrap()
            .success());
    }
    let base_ref = "refs/heads/base";
    let head_ref = "refs/heads/topic";
    let initial = snapshot_for_refs(repository.path(), base_ref, head_ref);
    let saved = store.save_branch_review(entry.id, &initial).unwrap();
    store.activate_initial_snapshot(&saved).unwrap();
    let old_head = saved.head_revision.clone();
    let workspace = store.review_workspace_path(&saved.review_id).unwrap();
    GitProcessAdapter::new(repository.path())
        .create_detached_worktree(&workspace, &old_head, OperationContext::background().cancellation())
        .unwrap();

    std::fs::write(repository.path().join("README.md"), "refreshed").unwrap();
    assert!(Command::new("git")
        .args(["add", "README.md"])
        .current_dir(repository.path())
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["commit", "-q", "-m", "refresh"])
        .current_dir(repository.path())
        .status()
        .unwrap()
        .success());
    let latest = snapshot_for_refs(repository.path(), base_ref, head_ref);

    let candidate = store
        .stage_branch_refresh_candidate(&saved.review_id, &latest)
        .unwrap();
    assert_eq!(
        store.open_review(&saved.review_id).unwrap().head_revision,
        old_head
    );
    let checked_workspace_head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&workspace)
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(checked_workspace_head.stdout)
            .unwrap()
            .trim(),
        old_head
    );

    GitProcessAdapter::new(repository.path())
        .transition_detached_worktree(
            &workspace,
            &entry.git_common_directory,
            &old_head,
            &candidate.head_revision,
            OperationContext::background().cancellation(),
        )
        .unwrap();
    store
        .activate_refresh_candidate(&candidate, &saved)
        .unwrap();
    let refreshed = store.open_review(&saved.review_id).unwrap();
    assert_eq!(refreshed.head_revision, latest.head_revision);
    assert_ne!(refreshed.head_revision, old_head);
    assert_eq!(refreshed.file_changes.len(), 1);
    assert_eq!(refreshed.file_changes[0].path, "README.md");
    let workspace_head = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&workspace)
        .output()
        .unwrap();
    assert!(workspace_head.status.success());
    assert_eq!(
        String::from_utf8(workspace_head.stdout).unwrap().trim(),
        refreshed.head_revision
    );
    let internal_refs = Command::new("git")
        .args(["for-each-ref", "--format=%(refname)", "refs/narrant"])
        .current_dir(repository.path())
        .output()
        .unwrap();
    assert!(internal_refs.status.success());
    assert!(internal_refs.stdout.is_empty());
}

#[test]
fn lists_repository_reviews_and_recent_reviews_by_last_opened_time() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let first_repository = create_repository();
    let second_repository = create_repository();
    let store = ReviewStore::open(database.path()).unwrap();
    let first_entry = store.upsert_repository(&verify_repository_location(first_repository.path()).unwrap()).unwrap();
    let second_entry = store.upsert_repository(&verify_repository_location(second_repository.path()).unwrap()).unwrap();
    store.insert_workspace_trust(first_entry.id).unwrap();
    store.insert_workspace_trust(second_entry.id).unwrap();

    let first_snapshot = snapshot_for_head(&first_repository);
    let first_review = store
        .save_branch_review(first_entry.id, &first_snapshot)
        .unwrap();
    store.activate_initial_snapshot(&first_review).unwrap();
    let second_snapshot = snapshot_for_head(&second_repository);
    let second_review = store
        .save_branch_review(second_entry.id, &second_snapshot)
        .unwrap();
    store.activate_initial_snapshot(&second_review).unwrap();

    let repository_reviews = store.list_reviews(Some(first_entry.id)).unwrap();
    let recent_reviews = store.list_reviews(None).unwrap();

    assert_eq!(repository_reviews.len(), 1);
    assert_eq!(repository_reviews[0].review_id, first_review.review_id);
    assert_eq!(repository_reviews[0].repository_id, first_entry.id);
    assert_eq!(recent_reviews.len(), 2);
    assert_eq!(recent_reviews[0].review_id, second_review.review_id);
    assert_eq!(recent_reviews[1].review_id, first_review.review_id);
    assert_eq!(
        recent_reviews[0].repository_path,
        second_entry.canonical_path
    );
}

/// 从真实仓库当前分支构造可持久化的最小 Snapshot。
fn snapshot_for_head(
    repository: &tempfile::TempDir,
) -> crate::review::ChangeComparison {
    let branch = String::from_utf8(
        Command::new("git")
            .args(["symbolic-ref", "--short", "HEAD"])
            .current_dir(repository.path())
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let branch_ref = format!("refs/heads/{}", branch.trim());
    snapshot_for_refs(repository.path(), &branch_ref, &branch_ref)
}

/// 通过 Branch Selection seam 从真实 refs 解析固定 Snapshot。
fn snapshot_for_refs(
    repository_path: &std::path::Path,
    base_ref: &str,
    head_ref: &str,
) -> crate::review::ChangeComparison {
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(crate::review::BranchSelectionResolver.resolve(
            repository_path,
            &crate::review::BranchSelection {
                base_ref: base_ref.to_owned(),
                head_ref: head_ref.to_owned(),
            },
            &crate::operation::OperationContext::background(),
        ))
        .unwrap()
        .comparison
}
