#[test]
fn compares_committed_base_and_head_refs_as_a_pinned_file_change() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    commit_file(&repository, "welcome.txt", "你好，旧内容\n", "base");
    repository
        .branch(
            "base",
            &repository.head().unwrap().peel_to_commit().unwrap(),
            true,
        )
        .unwrap();
    repository
        .branch(
            "head",
            &repository.head().unwrap().peel_to_commit().unwrap(),
            true,
        )
        .unwrap();
    repository.set_head("refs/heads/head").unwrap();
    repository
        .checkout_head(Some(CheckoutBuilder::new().force()))
        .unwrap();
    let expected_head = commit_file(&repository, "welcome.txt", "你好，新内容\n", "head");

    let refs = branch_refs(fixture.path()).expect("list refs").refs;
    assert_eq!(
        refs,
        vec!["refs/heads/base", "refs/heads/head", "refs/heads/master"]
    );

    let content = open_comparison(fixture.path());
    let comparison = &content.comparison;

    assert_eq!(comparison.base_ref, "refs/heads/base");
    assert_eq!(comparison.head_ref, "refs/heads/head");
    assert_eq!(comparison.comparison_base, comparison.base_revision);
    assert_eq!(comparison.head_revision, expected_head.to_string());
    assert_eq!(comparison.file_changes.len(), 1);
    assert_eq!(comparison.file_changes[0].path, "welcome.txt");
    assert_eq!(comparison.file_changes[0].status, FileChangeStatus::Modified);

    commit_file(
        &repository,
        "welcome.txt",
        "分支移动后的内容\n",
        "move head after opening review",
    );
    let content = content
        .read("welcome.txt")
        .expect("read pinned File Change");
    assert_eq!(content.before.as_ref().and_then(FileContent::text), Some("你好，旧内容\n"));
    assert_eq!(content.after.as_ref().and_then(FileContent::text), Some("你好，新内容\n"));
}

#[test]
fn lists_local_and_existing_remote_tracking_refs() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    commit_file(&repository, "welcome.txt", "hello\n", "base");
    let commit = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("topic", &commit, true).unwrap();
    repository
        .reference("refs/remotes/origin/main", commit.id(), true, "fixture")
        .unwrap();
    repository
        .reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            true,
            "fixture",
        )
        .unwrap();

    let refs = branch_refs(fixture.path()).expect("list refs").refs;

    assert_eq!(
        refs,
        vec![
            "refs/heads/master",
            "refs/heads/topic",
            "refs/remotes/origin/main"
        ]
    );
}

#[test]
fn branch_refs_report_remote_default_and_current_branch_independently_of_ref_order() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    commit_file(&repository, "welcome.txt", "hello\n", "base");
    let commit = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("aaa-topic", &commit, true).unwrap();
    repository.set_head("refs/heads/aaa-topic").unwrap();
    repository
        .reference("refs/remotes/origin/main", commit.id(), true, "fixture")
        .unwrap();
    repository
        .reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            true,
            "fixture",
        )
        .unwrap();

    let branches = branch_refs(fixture.path()).expect("list branch refs");

    assert_eq!(branches.default_ref, "refs/remotes/origin/main");
    assert_eq!(branches.current_ref, "refs/heads/aaa-topic");
    assert_eq!(
        branches.refs,
        vec![
            "refs/heads/aaa-topic",
            "refs/heads/master",
            "refs/remotes/origin/main",
        ]
    );
}

#[test]
fn branch_refs_report_detached_head_as_an_actionable_repository_error() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    let commit = commit_file(&repository, "welcome.txt", "hello\n", "base");
    repository.set_head_detached(commit).unwrap();

    let error = branch_refs(fixture.path()).unwrap_err();

    assert!(error.to_string().contains("当前未检出本地分支"));
}

#[test]
fn compares_from_merge_base_when_base_and_head_have_diverged() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    let common = commit_file(&repository, "common.txt", "common\n", "common ancestor");
    repository
        .branch("head", &repository.find_commit(common).unwrap(), true)
        .unwrap();
    let base_tip = commit_file(&repository, "base-only.txt", "base\n", "base advances");
    repository
        .branch("base", &repository.find_commit(base_tip).unwrap(), true)
        .unwrap();
    repository.set_head("refs/heads/head").unwrap();
    repository
        .checkout_head(Some(CheckoutBuilder::new().force()))
        .unwrap();
    commit_file(&repository, "head-only.txt", "head\n", "head advances");

    let content = open_comparison(fixture.path());
    let comparison = &content.comparison;

    assert_eq!(comparison.base_revision, base_tip.to_string());
    assert_eq!(comparison.comparison_base, common.to_string());
    assert_eq!(comparison.file_changes.len(), 1);
    assert_eq!(comparison.file_changes[0].path, "head-only.txt");
    assert_eq!(comparison.file_changes[0].status, FileChangeStatus::Added);
}

#[test]
fn reports_added_modified_deleted_and_renamed_file_changes() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    commit_file(&repository, "modified.txt", "before\n", "add modified file");
    commit_file(&repository, "deleted.txt", "deleted\n", "add deleted file");
    commit_file(
        &repository,
        "old-name.txt",
        "rename me\n",
        "add renamed file",
    );
    repository
        .branch(
            "base",
            &repository.head().unwrap().peel_to_commit().unwrap(),
            true,
        )
        .unwrap();

    let workdir = repository.workdir().unwrap();
    std::fs::write(workdir.join("modified.txt"), "after\n").unwrap();
    std::fs::write(workdir.join("added.txt"), "added\n").unwrap();
    std::fs::rename(workdir.join("old-name.txt"), workdir.join("new-name.txt")).unwrap();
    std::fs::remove_file(workdir.join("deleted.txt")).unwrap();
    let mut index = repository.index().unwrap();
    index
        .add_path(std::path::Path::new("modified.txt"))
        .unwrap();
    index.add_path(std::path::Path::new("added.txt")).unwrap();
    index
        .add_path(std::path::Path::new("new-name.txt"))
        .unwrap();
    index
        .remove_path(std::path::Path::new("old-name.txt"))
        .unwrap();
    index
        .remove_path(std::path::Path::new("deleted.txt"))
        .unwrap();
    index.write().unwrap();
    let tree_id = index.write_tree().unwrap();
    let tree = repository.find_tree(tree_id).unwrap();
    let parent = repository.head().unwrap().peel_to_commit().unwrap();
    let signature = Signature::now("Narrant test", "test@narrant.invalid").unwrap();
    repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            "all statuses",
            &tree,
            &[&parent],
        )
        .unwrap();
    repository
        .branch(
            "head",
            &repository.head().unwrap().peel_to_commit().unwrap(),
            true,
        )
        .unwrap();

    let content = open_comparison(fixture.path());
    let comparison = &content.comparison;
    let statuses = comparison
        .file_changes
        .iter()
        .map(|change| (change.path.as_str(), change.status))
        .collect::<Vec<_>>();

    assert_eq!(
        statuses,
        vec![
            ("added.txt", FileChangeStatus::Added),
            ("deleted.txt", FileChangeStatus::Deleted),
            ("modified.txt", FileChangeStatus::Modified),
            ("new-name.txt", FileChangeStatus::Renamed),
        ]
    );
    let renamed = comparison
        .file_changes
        .iter()
        .find(|change| change.status == FileChangeStatus::Renamed)
        .unwrap();
    assert_eq!(renamed.old_path.as_deref(), Some("old-name.txt"));
    let renamed_content = content.read("new-name.txt").unwrap();
    assert_eq!(renamed_content.before.as_ref().and_then(FileContent::text), Some("rename me\n"));
    assert_eq!(renamed_content.after.as_ref().and_then(FileContent::text), Some("rename me\n"));

    let old_path_error = content.read("old-name.txt").unwrap_err();
    assert_eq!(
        old_path_error.message(),
        "File Change is not part of the Review Snapshot"
    );

    let added = content.read("added.txt").unwrap();
    assert_eq!(added.before, None);
    assert_eq!(added.after.as_ref().and_then(FileContent::text), Some("added\n"));

    let deleted = content.read("deleted.txt").unwrap();
    assert_eq!(deleted.before.as_ref().and_then(FileContent::text), Some("deleted\n"));
    assert_eq!(deleted.after, None);

    let modified = content.read("modified.txt").unwrap();
    assert_eq!(modified.before.as_ref().and_then(FileContent::text), Some("before\n"));
    assert_eq!(modified.after.as_ref().and_then(FileContent::text), Some("after\n"));
}
