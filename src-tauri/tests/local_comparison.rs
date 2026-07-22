use git2::{build::CheckoutBuilder, Repository, Signature};
use narrant_lib::change_content::local::{compare_refs, list_refs, read_file_change};
use narrant_lib::change_content::{ChangeStatus, ContentKind};
use tempfile::TempDir;

fn commit_file(repository: &Repository, path: &str, contents: &str, message: &str) -> git2::Oid {
    commit_bytes(repository, path, contents.as_bytes(), message)
}

fn commit_bytes(repository: &Repository, path: &str, contents: &[u8], message: &str) -> git2::Oid {
    let workdir = repository.workdir().expect("fixture has a worktree");
    std::fs::write(workdir.join(path), contents).expect("write fixture file");
    let mut index = repository.index().expect("open index");
    index
        .add_path(std::path::Path::new(path))
        .expect("stage file");
    index.write().expect("write index");
    let tree_id = index.write_tree().expect("write tree");
    let tree = repository.find_tree(tree_id).expect("find tree");
    let signature = Signature::now("Narrant test", "test@narrant.invalid").expect("signature");
    let parents = repository
        .head()
        .ok()
        .and_then(|head| head.peel_to_commit().ok())
        .into_iter()
        .collect::<Vec<_>>();
    repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents.iter().collect::<Vec<_>>(),
        )
        .expect("commit")
}

fn commit_tree_entry(
    repository: &Repository,
    path: &str,
    object_id: git2::Oid,
    filemode: i32,
    message: &str,
) -> git2::Oid {
    let parent = repository.head().unwrap().peel_to_commit().unwrap();
    let parent_tree = parent.tree().unwrap();
    let mut builder = repository.treebuilder(Some(&parent_tree)).unwrap();
    builder.insert(path, object_id, filemode).unwrap();
    let tree_id = builder.write().unwrap();
    let tree = repository.find_tree(tree_id).unwrap();
    let signature = Signature::now("Narrant test", "test@narrant.invalid").unwrap();
    repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &[&parent],
        )
        .unwrap()
}

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

    let refs = list_refs(fixture.path()).expect("list refs");
    assert_eq!(refs, vec!["base", "head", "master"]);

    let snapshot = compare_refs(fixture.path(), "base", "head").expect("compare refs");

    assert_eq!(snapshot.base_ref, "base");
    assert_eq!(snapshot.head_ref, "head");
    assert_eq!(snapshot.comparison_base, snapshot.base_revision);
    assert_eq!(snapshot.head_revision, expected_head.to_string());
    assert_eq!(snapshot.file_changes.len(), 1);
    assert_eq!(snapshot.file_changes[0].path, "welcome.txt");
    assert_eq!(snapshot.file_changes[0].before, None);
    assert_eq!(snapshot.file_changes[0].after, None);
    assert_eq!(snapshot.file_changes[0].status, ChangeStatus::Modified);

    let content = read_file_change(
        fixture.path(),
        &snapshot.comparison_base,
        &snapshot.head_revision,
        "welcome.txt",
    )
    .expect("read pinned File Change");
    assert_eq!(content.before.as_deref(), Some("你好，旧内容\n"));
    assert_eq!(content.after.as_deref(), Some("你好，新内容\n"));
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

    let refs = list_refs(fixture.path()).expect("list refs");

    assert_eq!(refs, vec!["master", "origin/main", "topic"]);
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

    let snapshot = compare_refs(fixture.path(), "base", "head").expect("compare refs");

    assert_eq!(snapshot.base_revision, base_tip.to_string());
    assert_eq!(snapshot.comparison_base, common.to_string());
    assert_eq!(snapshot.file_changes.len(), 1);
    assert_eq!(snapshot.file_changes[0].path, "head-only.txt");
    assert_eq!(snapshot.file_changes[0].status, ChangeStatus::Added);
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

    let snapshot = compare_refs(fixture.path(), "base", "head").expect("compare refs");
    let statuses = snapshot
        .file_changes
        .iter()
        .map(|change| (change.path.as_str(), change.status))
        .collect::<Vec<_>>();

    assert_eq!(
        statuses,
        vec![
            ("added.txt", ChangeStatus::Added),
            ("deleted.txt", ChangeStatus::Deleted),
            ("modified.txt", ChangeStatus::Modified),
            ("new-name.txt", ChangeStatus::Renamed),
        ]
    );
    let renamed = snapshot
        .file_changes
        .iter()
        .find(|change| change.status == ChangeStatus::Renamed)
        .unwrap();
    assert_eq!(renamed.old_path.as_deref(), Some("old-name.txt"));
    let renamed_content = read_file_change(
        fixture.path(),
        &snapshot.comparison_base,
        &snapshot.head_revision,
        "new-name.txt",
    )
    .unwrap();
    assert_eq!(renamed_content.before.as_deref(), Some("rename me\n"));
    assert_eq!(renamed_content.after.as_deref(), Some("rename me\n"));
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

    let snapshot = compare_refs(fixture.path(), "base", "head").expect("compare refs");
    let change = &snapshot.file_changes[0];

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

    let snapshot = compare_refs(fixture.path(), "base", "head").unwrap();
    let content = read_file_change(
        fixture.path(),
        &snapshot.comparison_base,
        &snapshot.head_revision,
        "asset.dat",
    )
    .unwrap();

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

    let snapshot = compare_refs(fixture.path(), "base", "head").unwrap();
    let content = read_file_change(
        fixture.path(),
        &snapshot.comparison_base,
        &snapshot.head_revision,
        "link",
    )
    .unwrap();

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

    let snapshot = compare_refs(fixture.path(), "base", "head").unwrap();
    let content = read_file_change(
        fixture.path(),
        &snapshot.comparison_base,
        &snapshot.head_revision,
        "vendor",
    )
    .unwrap();

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

    let snapshot = compare_refs(fixture.path(), "base", "head").unwrap();
    let content = read_file_change(
        fixture.path(),
        &snapshot.comparison_base,
        &snapshot.head_revision,
        "tracked.txt",
    )
    .unwrap();

    assert_eq!(snapshot.file_changes.len(), 1);
    assert_eq!(content.after.as_deref(), Some("committed head\n"));
}
