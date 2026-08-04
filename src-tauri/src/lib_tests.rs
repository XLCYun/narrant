use super::*;
use std::process::Command;

#[test]
fn same_review_cannot_begin_two_refreshes_concurrently() {
    let store = OpenReviewStore::default();

    store.begin_refresh("review-1").unwrap();
    let error = store.begin_refresh("review-1").unwrap_err();
    store.begin_refresh("review-2").unwrap();

    assert!(error.contains("正在刷新"));
    store.finish_refresh("review-1");
    store.begin_refresh("review-1").unwrap();
}

#[test]
fn closing_an_open_review_is_idempotent_and_keeps_in_flight_references_alive() {
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
    let response = map_snapshot(
        "review-42".to_owned(),
        "snapshot-7".to_owned(),
        review.snapshot(),
    );
    let json = serde_json::to_value(&response).unwrap();
    assert_eq!(json["reviewId"], "review-42");
    assert_eq!(json["snapshotId"], "snapshot-7");
    assert_eq!(json["fileChanges"][0]["path"], "README.md");
    assert!(json["fileChanges"][0].get("contentKind").is_none());
    let store = OpenReviewStore::default();
    let id = "review-42".to_owned();
    store
        .insert_with_id(id.clone(), OpenedReview::Local(Box::new(review)))
        .unwrap();
    let in_flight = store.get(&id).unwrap();
    store.close(&id).unwrap();
    store.close(&id).unwrap();
    assert_eq!(
        store.get(&id).err().unwrap(),
        "Review 已关闭或失效，请重新打开。"
    );
    let in_flight = in_flight.lock().unwrap();
    let OpenedReview::Local(review) = &*in_flight;
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
fn startup_recovery_removes_only_catalog_owned_registered_workspace() {
    let application_data = tempfile::TempDir::new().unwrap();
    let repository = test_repository_with_commit();
    let catalog = repository_catalog::RepositoryCatalog::open(
        &application_data.path().join("catalog.sqlite3"),
    )
    .unwrap();
    let entry = catalog.register(repository.path()).unwrap();
    catalog.grant_workspace_trust(entry.id).unwrap();
    let branch = git_text(repository.path(), &["symbolic-ref", "--short", "HEAD"]);
    let branch_ref = format!("refs/heads/{branch}");
    let opened =
        change_content::local::LocalOpenedReview::open(repository.path(), &branch_ref, &branch_ref)
            .unwrap();
    let saved = catalog
        .save_branch_review(entry.id, opened.snapshot())
        .unwrap();
    catalog.activate_snapshot(&saved).unwrap();
    let workspace = catalog.review_workspace_path(&saved.review_id).unwrap();
    core_git::GitProcessAdapter::new(repository.path())
        .create_detached_worktree(
            &workspace,
            &saved.head_revision,
            &core_git::CancellationToken::default(),
        )
        .unwrap();
    let user_worktree = application_data.path().join("user-worktree");
    Command::new("git")
        .args(["worktree", "add", "--detach", "--"])
        .arg(&user_worktree)
        .arg(&saved.head_revision)
        .current_dir(repository.path())
        .status()
        .unwrap();

    recover_interrupted_review_workspaces(&catalog);

    assert!(!workspace.exists());
    assert!(user_worktree.exists());
}

#[test]
fn failed_review_switch_keeps_the_current_review_readable() {
    let application_data = tempfile::TempDir::new().unwrap();
    let first_repository = test_repository_with_commit();
    let target_repository = test_repository_with_commit();
    let catalog = repository_catalog::RepositoryCatalog::open(
        &application_data.path().join("catalog.sqlite3"),
    )
    .unwrap();
    let first_entry = catalog.register(first_repository.path()).unwrap();
    let target_entry = catalog.register(target_repository.path()).unwrap();
    catalog.grant_workspace_trust(first_entry.id).unwrap();
    catalog.grant_workspace_trust(target_entry.id).unwrap();
    let first_saved = save_current_branch_review(&catalog, first_entry.id, first_repository.path());
    let target_saved =
        save_current_branch_review(&catalog, target_entry.id, target_repository.path());
    let store = OpenReviewStore::default();

    open_saved_branch_review(
        &store,
        &catalog,
        first_repository.path().to_path_buf(),
        first_saved.clone(),
    )
    .unwrap();
    let first_workspace = catalog
        .review_workspace_path(&first_saved.review_id)
        .unwrap();
    assert!(first_workspace.exists());

    let unavailable_target = application_data.path().join("missing-target-repository");
    let error = match open_saved_branch_review(&store, &catalog, unavailable_target, target_saved) {
        Ok(_) => panic!("目标 Review materialization 应失败"),
        Err(error) => error,
    };

    assert!(!error.is_empty());
    assert!(store.get(&first_saved.review_id).is_ok());
    assert!(first_workspace.exists());
}

/// 创建供 runtime 生命周期测试使用的含提交 working-tree repository。
fn test_repository_with_commit() -> tempfile::TempDir {
    let repository = tempfile::TempDir::new().unwrap();
    Command::new("git")
        .args(["init", "-q"])
        .current_dir(repository.path())
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.email", "test@narrant.invalid"])
        .current_dir(repository.path())
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Narrant Test"])
        .current_dir(repository.path())
        .status()
        .unwrap();
    std::fs::write(repository.path().join("README.md"), "fixture").unwrap();
    Command::new("git")
        .args(["add", "README.md"])
        .current_dir(repository.path())
        .status()
        .unwrap();
    Command::new("git")
        .args(["commit", "-q", "-m", "initial"])
        .current_dir(repository.path())
        .status()
        .unwrap();
    repository
}

/// 保存一个以当前分支作为 Base Ref 与 Head Ref 的测试 Review。
fn save_current_branch_review(
    catalog: &repository_catalog::RepositoryCatalog,
    repository_entry_id: i64,
    repository_path: &std::path::Path,
) -> repository_catalog::SavedBranchReview {
    let branch = git_text(repository_path, &["symbolic-ref", "--short", "HEAD"]);
    let branch_ref = format!("refs/heads/{branch}");
    let opened =
        change_content::local::LocalOpenedReview::open(repository_path, &branch_ref, &branch_ref)
            .unwrap();
    let saved = catalog
        .save_branch_review(repository_entry_id, opened.snapshot())
        .unwrap();
    saved
}

/// 返回生命周期测试所需的单行 Core Git 输出。
fn git_text(repository: &std::path::Path, arguments: &[&str]) -> String {
    String::from_utf8(
        Command::new("git")
            .args(arguments)
            .current_dir(repository)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned()
}
