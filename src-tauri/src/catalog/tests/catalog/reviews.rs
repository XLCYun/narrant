#[test]
fn same_branch_selection_reuses_review_and_preserves_snapshot_metadata() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let repository = create_repository();
    let catalog = RepositoryCatalog::open(database.path()).unwrap();
    let entry = catalog.register(repository.path()).unwrap();
    catalog.grant_workspace_trust(entry.id).unwrap();
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
    let snapshot = snapshot_for_refs(repository.path(), &branch_ref, &branch_ref);

    let first = catalog.save_branch_review(entry.id, &snapshot).unwrap();
    catalog.activate_snapshot(&first).unwrap();
    let reopened = catalog.save_branch_review(entry.id, &snapshot).unwrap();
    let loaded = catalog.open_review(&first.review_id).unwrap();

    assert_eq!(reopened.review_id, first.review_id);
    assert_eq!(loaded.review_id, first.review_id);
    assert_eq!(loaded.base_ref, branch_ref);
    assert_eq!(loaded.head_ref, branch_ref);
    assert_eq!(loaded.head_revision, snapshot.head_revision);
    assert!(loaded.file_changes.is_empty());
}
#[test]
fn same_pull_request_selection_reuses_review_when_base_branch_changes() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let repository = create_repository();
    let catalog = RepositoryCatalog::open(database.path()).unwrap();
    let entry = catalog.register(repository.path()).unwrap();
    catalog.grant_workspace_trust(entry.id).unwrap();
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
    let snapshot = snapshot_for_refs(repository.path(), &branch_ref, &branch_ref);
    let first_metadata = PullRequestMetadata {
        owner: "octo-org".to_owned(),
        repository: "octo-repo".to_owned(),
        number: 42,
        title: "Initial title".to_owned(),
        description: "Initial description".to_owned(),
        author: "octocat".to_owned(),
        state: "open".to_owned(),
        draft: false,
        url: "https://github.com/octo-org/octo-repo/pull/42".to_owned(),
        base_ref: "main".to_owned(),
        head_ref: "feature".to_owned(),
        updated_at: Some("2026-07-31T08:00:00Z".to_owned()),
        synchronized_at: 1,
    };

    let first = catalog
        .save_pull_request_review(entry.id, &first_metadata, &snapshot)
        .unwrap();
    catalog.activate_snapshot(&first).unwrap();
    let reopened = catalog
        .save_pull_request_review(
            entry.id,
            &PullRequestMetadata {
                title: "Updated title".to_owned(),
                base_ref: "release".to_owned(),
                synchronized_at: 2,
                ..first_metadata
            },
            &snapshot,
        )
        .unwrap();

    assert_eq!(reopened.review_id, first.review_id);
    assert!(!reopened.created);
    assert_eq!(
        reopened.pull_request.as_ref().unwrap().title,
        "Updated title"
    );
    assert_eq!(reopened.pull_request.as_ref().unwrap().base_ref, "release");
    let listed = catalog.list_saved_review_snapshots(entry.id).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].review_id, first.review_id);
    assert_eq!(
        listed[0].pull_request.as_ref().unwrap().title,
        "Updated title"
    );
}
#[test]
fn staged_pull_request_snapshot_does_not_advance_until_explicit_activation() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let repository = create_repository();
    let catalog = RepositoryCatalog::open(database.path()).unwrap();
    let entry = catalog.register(repository.path()).unwrap();
    catalog.grant_workspace_trust(entry.id).unwrap();
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
    let opened = snapshot_for_refs(repository.path(), &branch_ref, &branch_ref);
    let metadata = PullRequestMetadata {
        owner: "octo-org".to_owned(),
        repository: "octo-repo".to_owned(),
        number: 7,
        title: "Review".to_owned(),
        description: String::new(),
        author: "octocat".to_owned(),
        state: "open".to_owned(),
        draft: false,
        url: "https://github.com/octo-org/octo-repo/pull/7".to_owned(),
        base_ref: "main".to_owned(),
        head_ref: "topic".to_owned(),
        updated_at: Some("2026-07-31T08:00:00Z".to_owned()),
        synchronized_at: 1,
    };
    let first = catalog
        .save_pull_request_review(entry.id, &metadata, &opened)
        .unwrap();
    catalog.activate_snapshot(&first).unwrap();
    let mut next_snapshot = opened.clone();
    next_snapshot.head_revision = "2222222222222222222222222222222222222222".to_owned();

    let refreshed_metadata = PullRequestMetadata {
        title: "Updated Review".to_owned(),
        synchronized_at: 2,
        ..metadata
    };
    let staged = catalog
        .stage_pull_request_snapshot(&first, &refreshed_metadata, &next_snapshot)
        .unwrap();
    let before_activation = catalog.open_review(&first.review_id).unwrap();
    assert_eq!(before_activation.head_revision, first.head_revision);
    assert_eq!(before_activation.pull_request.unwrap().title, "Review");
    assert_eq!(
        catalog
            .list_review_snapshot_identities(&first.review_id)
            .unwrap(),
        vec![first.snapshot_identity()]
    );

    catalog.activate_snapshot(&staged).unwrap();
    let after_activation = catalog.open_review(&first.review_id).unwrap();
    assert_eq!(after_activation.head_revision, next_snapshot.head_revision);
    assert_eq!(
        after_activation.pull_request.unwrap().title,
        "Updated Review"
    );
    assert_eq!(
        catalog
            .list_review_snapshot_identities(&first.review_id)
            .unwrap(),
        vec![first.snapshot_identity(), staged.snapshot_identity()]
    );
}
