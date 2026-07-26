use super::*;

#[test]
fn closing_a_review_session_is_idempotent_and_keeps_in_flight_references_alive() {
    let fixture = tempfile::TempDir::new().unwrap();
    let repository = git2::Repository::init(fixture.path()).unwrap();
    let old_blob = repository.blob(b"old\n").unwrap();
    let mut tree_builder = repository.treebuilder(None).unwrap();
    tree_builder
        .insert("README.md", old_blob, 0o100644)
        .unwrap();
    let tree_id = tree_builder.write().unwrap();
    drop(tree_builder);
    let tree = repository.find_tree(tree_id).unwrap();
    let signature = git2::Signature::now("Narrant test", "test@narrant.invalid").unwrap();
    let commit_id = repository
        .commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])
        .unwrap();
    let commit = repository.find_commit(commit_id).unwrap();
    repository.branch("base", &commit, true).unwrap();
    let new_blob = repository.blob(b"new\n").unwrap();
    let mut tree_builder = repository.treebuilder(Some(&tree)).unwrap();
    tree_builder
        .insert("README.md", new_blob, 0o100644)
        .unwrap();
    let head_tree_id = tree_builder.write().unwrap();
    let head_tree = repository.find_tree(head_tree_id).unwrap();
    let head_id = repository
        .commit(None, &signature, &signature, "head", &head_tree, &[&commit])
        .unwrap();
    let head = repository.find_commit(head_id).unwrap();
    repository.branch("head", &head, true).unwrap();
    drop(head);
    drop(head_tree);
    drop(tree_builder);
    drop(commit);
    drop(tree);
    drop(repository);
    let review = change_content::local::LocalOpenedReview::open(
        fixture.path(),
        "refs/heads/base",
        "refs/heads/head",
    )
    .unwrap();
    let response = map_local_snapshot("review-42".to_owned(), review.snapshot());
    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["source"], "local");
    assert_eq!(json["reviewId"], "review-42");
    assert!(json.get("sessionId").is_none());
    assert_eq!(json["fileChanges"][0]["path"], "README.md");
    assert!(json["fileChanges"][0].get("contentKind").is_none());
    assert!(json.get("snapshotId").is_none());
    let store = ReviewSessionStore::default();
    let id = store.insert(OpenedReview::Local(Box::new(review))).unwrap();
    let in_flight = store.get(&id).unwrap();
    let github_id = store
        .insert(OpenedReview::GitHub(Box::new(
            change_content::github::GitHubOpenedReview::empty_for_test(),
        )))
        .unwrap();
    store.close_github_sessions().unwrap();
    assert!(store.get(&id).is_ok());
    assert_eq!(
        store.get(&github_id).err().unwrap(),
        "Review Session 已关闭或失效，请重新打开。"
    );
    store.close(&id).unwrap();
    store.close(&id).unwrap();
    assert_eq!(
        store.get(&id).err().unwrap(),
        "Review Session 已关闭或失效，请重新打开。"
    );
    let in_flight = in_flight.lock().unwrap();
    let OpenedReview::Local(review) = &*in_flight else {
        panic!("expected Local Review");
    };
    let change = review.read_file_change("README.md").unwrap();
    assert_eq!(change.before.as_deref(), Some("old\n"));
    assert_eq!(change.after.as_deref(), Some("new\n"));
}

#[test]
fn loaded_file_change_mapper_serializes_every_status_and_content_kind() {
    let statuses = [
        (change_content::ChangeStatus::Added, "added"),
        (change_content::ChangeStatus::Modified, "modified"),
        (change_content::ChangeStatus::Deleted, "deleted"),
        (change_content::ChangeStatus::Renamed, "renamed"),
    ];
    let kinds = [
        (change_content::ContentKind::Text, "text"),
        (change_content::ContentKind::Binary, "binary"),
        (change_content::ContentKind::Submodule, "submodule"),
        (change_content::ContentKind::Symlink, "symlink"),
    ];
    for ((status, expected_status), (kind, expected_kind)) in statuses.into_iter().zip(kinds) {
        let response = map_file_content(change_content::LoadedFileChange {
            path: "new.txt".to_owned(),
            old_path: Some("old.txt".to_owned()),
            status,
            content_kind: kind,
            before_size: Some(3),
            after_size: Some(4),
            before: Some("old".to_owned()),
            after: Some("new\n".to_owned()),
        });
        let json = serde_json::to_value(response).unwrap();
        assert_eq!(json["status"], expected_status);
        assert_eq!(json["contentKind"], expected_kind);
        assert_eq!(json["oldPath"], "old.txt");
        assert_eq!(json["beforeSize"], 3);
        assert_eq!(json["afterSize"], 4);
    }
}

#[test]
fn github_snapshot_mapper_produces_a_tagged_summary_without_content_placeholders() {
    let snapshot = change_content::github::GitHubSnapshot {
        owner: "octo-org".to_owned(),
        repository: "octo-repo".to_owned(),
        title: "Review".to_owned(),
        number: 42,
        author: "octocat".to_owned(),
        state: "open".to_owned(),
        draft: false,
        base_ref: "main".to_owned(),
        base_revision: "base".to_owned(),
        comparison_base: "merge-base".to_owned(),
        head_ref: "topic".to_owned(),
        head_revision: "head".to_owned(),
        description: String::new(),
        changed_files: 1,
        additions: 2,
        deletions: 1,
        file_changes: vec![change_content::github::GitHubFileChange {
            path: "README.md".to_owned(),
            old_path: None,
            status: change_content::ChangeStatus::Modified,
        }],
        github_url: "https://github.com/octo-org/octo-repo/pull/42".to_owned(),
    };
    let response = map_github_snapshot("session-7".to_owned(), &snapshot);
    let json = serde_json::to_value(response).unwrap();
    assert_eq!(json["source"], "github");
    assert_eq!(json["sessionId"], "session-7");
    assert_eq!(json["fileChanges"][0]["path"], "README.md");
    assert!(json["fileChanges"][0].get("contentKind").is_none());
    assert!(json["fileChanges"][0].get("before").is_none());
}
