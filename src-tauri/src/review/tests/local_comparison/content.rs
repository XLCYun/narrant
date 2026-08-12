#[test]
fn opens_an_empty_comparison_and_rejects_unknown_paths() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).unwrap();
    commit_file(&repository, "README.md", "same\n", "initial");
    let commit = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("base", &commit, true).unwrap();
    repository.branch("head", &commit, true).unwrap();

    let review = open_comparison(fixture.path());

    assert!(review.snapshot.file_changes.is_empty());
    assert_eq!(
        review.read("missing.txt").unwrap_err().message(),
        "File Change is not part of the Review Snapshot"
    );
}

#[test]
fn rejects_missing_repositories_and_unknown_refs() {
    let fixture = TempDir::new().unwrap();
    assert!(resolve_comparison(fixture.path(), "refs/heads/base", "refs/heads/head").is_err());

    let repository = Repository::init(fixture.path()).unwrap();
    commit_file(&repository, "README.md", "same\n", "initial");
    assert!(resolve_comparison(fixture.path(), "refs/heads/missing", "refs/heads/master").is_err());
    assert!(
        resolve_comparison(fixture.path(), "refs/heads/master~1", "refs/heads/master").is_err()
    );
}

#[test]
fn represents_binary_changes_with_metadata_and_without_text() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    commit_bytes(&repository, "image.bin", &[0, 1, 2], "base binary");
    repository
        .branch(
            "base",
            &repository.head().unwrap().peel_to_commit().unwrap(),
            true,
        )
        .unwrap();
    commit_bytes(&repository, "image.bin", &[0, 3, 4, 5], "head binary");
    repository
        .branch(
            "head",
            &repository.head().unwrap().peel_to_commit().unwrap(),
            true,
        )
        .unwrap();

    let content = open_comparison(fixture.path());
    let change = content.read("image.bin").unwrap();

    assert_eq!(change.content_kind, ContentKind::Binary);
    assert_eq!(change.before_size, Some(3));
    assert_eq!(change.after_size, Some(4));
    assert_eq!(change.before, None);
    assert_eq!(change.after, None);
}

#[test]
fn reads_lfs_pointer_as_text_from_pinned_revisions() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    let old_pointer = "version https://git-lfs.github.com/spec/v1\noid sha256:old\nsize 3\n";
    let new_pointer = "version https://git-lfs.github.com/spec/v1\noid sha256:new\nsize 4\n";
    commit_file(&repository, "asset.dat", old_pointer, "base LFS pointer");
    repository
        .branch(
            "base",
            &repository.head().unwrap().peel_to_commit().unwrap(),
            true,
        )
        .unwrap();
    commit_file(&repository, "asset.dat", new_pointer, "head LFS pointer");
    repository
        .branch(
            "head",
            &repository.head().unwrap().peel_to_commit().unwrap(),
            true,
        )
        .unwrap();

    let content = open_comparison(fixture.path());
    let content = content.read("asset.dat").unwrap();

    assert_eq!(content.content_kind, ContentKind::Text);
    assert_eq!(content.before.as_deref(), Some(old_pointer));
    assert_eq!(content.after.as_deref(), Some(new_pointer));
}

#[test]
fn reads_symlink_targets_without_following_them() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    commit_file(&repository, "seed.txt", "seed\n", "seed");
    let old_target = repository.blob(b"missing-old-target").unwrap();
    commit_tree_entry(&repository, "link", old_target, 0o120000, "base symlink");
    repository
        .branch(
            "base",
            &repository.head().unwrap().peel_to_commit().unwrap(),
            true,
        )
        .unwrap();
    let new_target = repository.blob(b"missing-new-target").unwrap();
    commit_tree_entry(&repository, "link", new_target, 0o120000, "head symlink");
    repository
        .branch(
            "head",
            &repository.head().unwrap().peel_to_commit().unwrap(),
            true,
        )
        .unwrap();

    let content = open_comparison(fixture.path());
    let content = content.read("link").unwrap();

    assert_eq!(content.content_kind, ContentKind::Symlink);
    assert_eq!(content.before.as_deref(), Some("missing-old-target"));
    assert_eq!(content.after.as_deref(), Some("missing-new-target"));
}

#[test]
fn reads_submodule_commit_ids_from_pinned_trees() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    let old_submodule = commit_file(&repository, "seed.txt", "old\n", "old submodule commit");
    let new_submodule = commit_file(&repository, "seed.txt", "new\n", "new submodule commit");
    commit_tree_entry(
        &repository,
        "vendor",
        old_submodule,
        0o160000,
        "base gitlink",
    );
    repository
        .branch(
            "base",
            &repository.head().unwrap().peel_to_commit().unwrap(),
            true,
        )
        .unwrap();
    commit_tree_entry(
        &repository,
        "vendor",
        new_submodule,
        0o160000,
        "head gitlink",
    );
    repository
        .branch(
            "head",
            &repository.head().unwrap().peel_to_commit().unwrap(),
            true,
        )
        .unwrap();

    let content = open_comparison(fixture.path());
    let content = content.read("vendor").unwrap();

    assert_eq!(content.content_kind, ContentKind::Submodule);
    assert_eq!(
        content.before.as_deref(),
        Some(old_submodule.to_string().as_str())
    );
    assert_eq!(
        content.after.as_deref(),
        Some(new_submodule.to_string().as_str())
    );
}

#[test]
fn ignores_staged_unstaged_and_untracked_worktree_content() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    commit_file(&repository, "tracked.txt", "committed base\n", "base");
    repository
        .branch(
            "base",
            &repository.head().unwrap().peel_to_commit().unwrap(),
            true,
        )
        .unwrap();
    commit_file(&repository, "tracked.txt", "committed head\n", "head");
    repository
        .branch(
            "head",
            &repository.head().unwrap().peel_to_commit().unwrap(),
            true,
        )
        .unwrap();
    let workdir = repository.workdir().unwrap();
    std::fs::write(workdir.join("tracked.txt"), "unstaged\n").unwrap();
    std::fs::write(workdir.join("staged.txt"), "staged\n").unwrap();
    std::fs::write(workdir.join("untracked.txt"), "untracked\n").unwrap();
    let mut index = repository.index().unwrap();
    index.add_path(std::path::Path::new("staged.txt")).unwrap();
    index.write().unwrap();

    let content = open_comparison(fixture.path());
    let snapshot = &content.snapshot;
    let content = content.read("tracked.txt").unwrap();

    assert_eq!(snapshot.file_changes.len(), 1);
    assert_eq!(content.after.as_deref(), Some("committed head\n"));
}
