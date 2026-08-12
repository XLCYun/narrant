use super::review::{map_file_content, map_snapshot};
use crate::operation;
use crate::review;

#[tokio::test]
async fn snapshot_mapper_preserves_the_existing_ipc_contract() {
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
    let review = review::BranchSelectionResolver
        .resolve(
            fixture.path(),
            &review::BranchSelection {
                base_ref: "refs/heads/base".to_owned(),
                head_ref: "refs/heads/head".to_owned(),
            },
            &operation::OperationContext::non_cancellable(),
        )
        .await
        .unwrap();

    let response = map_snapshot(
        "review-42".to_owned(),
        "snapshot-7".to_owned(),
        &review.snapshot,
    );
    let json = serde_json::to_value(&response).unwrap();

    assert_eq!(json["reviewId"], "review-42");
    assert_eq!(json["snapshotId"], "snapshot-7");
    assert_eq!(json["fileChanges"][0]["path"], "README.md");
    assert!(json["fileChanges"][0].get("contentKind").is_none());
}

#[test]
fn loaded_file_change_mapper_serializes_every_status_and_content_kind() {
    let statuses = [
        (review::ChangeStatus::Added, "added"),
        (review::ChangeStatus::Modified, "modified"),
        (review::ChangeStatus::Deleted, "deleted"),
        (review::ChangeStatus::Renamed, "renamed"),
    ];
    let kinds = [
        (review::ContentKind::Text, "text"),
        (review::ContentKind::Binary, "binary"),
        (review::ContentKind::Submodule, "submodule"),
        (review::ContentKind::Symlink, "symlink"),
    ];
    for ((status, expected_status), (kind, expected_kind)) in statuses.into_iter().zip(kinds) {
        let response = map_file_content(review::LoadedFileChange {
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
