use git2::{build::CheckoutBuilder, Repository, Signature};
use httpmock::prelude::*;
use narrant_lib::change_content::local::{
    fetch_selected_remote_refs, list_refs, LocalOpenedReview,
};
use narrant_lib::change_content::{ChangeStatus, ContentKind};
use narrant_lib::core_git::CancellationToken;
use std::process::Command;
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

fn open_comparison(repository_path: &std::path::Path) -> LocalOpenedReview {
    LocalOpenedReview::open(repository_path, "refs/heads/base", "refs/heads/head").unwrap()
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
    assert_eq!(
        refs,
        vec!["refs/heads/base", "refs/heads/head", "refs/heads/master"]
    );

    let content = open_comparison(fixture.path());
    let snapshot = content.snapshot();

    assert_eq!(snapshot.base_ref, "refs/heads/base");
    assert_eq!(snapshot.head_ref, "refs/heads/head");
    assert_eq!(snapshot.comparison_base, snapshot.base_revision);
    assert_eq!(snapshot.head_revision, expected_head.to_string());
    assert_eq!(snapshot.file_changes.len(), 1);
    assert_eq!(snapshot.file_changes[0].path, "welcome.txt");
    assert_eq!(snapshot.file_changes[0].status, ChangeStatus::Modified);

    commit_file(
        &repository,
        "welcome.txt",
        "分支移动后的内容\n",
        "move head after opening review",
    );
    let content = content
        .read_file_change("welcome.txt")
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
    repository
        .reference_symbolic(
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
            true,
            "fixture",
        )
        .unwrap();

    let refs = list_refs(fixture.path()).expect("list refs");

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
    let snapshot = content.snapshot();

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

    let content = open_comparison(fixture.path());
    let snapshot = content.snapshot();
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
    let renamed_content = content.read_file_change("new-name.txt").unwrap();
    assert_eq!(renamed_content.before.as_deref(), Some("rename me\n"));
    assert_eq!(renamed_content.after.as_deref(), Some("rename me\n"));

    let old_path_error = content.read_file_change("old-name.txt").unwrap_err();
    assert_eq!(
        old_path_error.message(),
        "File Change is not part of the Review Snapshot"
    );

    let added = content.read_file_change("added.txt").unwrap();
    assert_eq!(added.before, None);
    assert_eq!(added.after.as_deref(), Some("added\n"));

    let deleted = content.read_file_change("deleted.txt").unwrap();
    assert_eq!(deleted.before.as_deref(), Some("deleted\n"));
    assert_eq!(deleted.after, None);

    let modified = content.read_file_change("modified.txt").unwrap();
    assert_eq!(modified.before.as_deref(), Some("before\n"));
    assert_eq!(modified.after.as_deref(), Some("after\n"));
}

#[test]
fn opens_an_empty_comparison_and_rejects_unknown_paths() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).unwrap();
    commit_file(&repository, "README.md", "same\n", "initial");
    let commit = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("base", &commit, true).unwrap();
    repository.branch("head", &commit, true).unwrap();

    let review = open_comparison(fixture.path());

    assert!(review.snapshot().file_changes.is_empty());
    assert_eq!(
        review
            .read_file_change("missing.txt")
            .unwrap_err()
            .message(),
        "File Change is not part of the Review Snapshot"
    );
}

#[test]
fn rejects_missing_repositories_and_unknown_refs() {
    let fixture = TempDir::new().unwrap();
    assert!(LocalOpenedReview::open(fixture.path(), "refs/heads/base", "refs/heads/head").is_err());

    let repository = Repository::init(fixture.path()).unwrap();
    commit_file(&repository, "README.md", "same\n", "initial");
    assert!(
        LocalOpenedReview::open(fixture.path(), "refs/heads/missing", "refs/heads/master").is_err()
    );
    assert!(
        LocalOpenedReview::open(fixture.path(), "refs/heads/master~1", "refs/heads/master")
            .is_err()
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
    let change = content.read_file_change("image.bin").unwrap();

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
    let content = content.read_file_change("asset.dat").unwrap();

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
    let content = content.read_file_change("link").unwrap();

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
    let content = content.read_file_change("vendor").unwrap();

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
    let snapshot = content.snapshot();
    let content = content.read_file_change("tracked.txt").unwrap();

    assert_eq!(snapshot.file_changes.len(), 1);
    assert_eq!(content.after.as_deref(), Some("committed head\n"));
}

#[test]
fn preserves_spaces_newlines_and_unicode_in_nul_delimited_paths() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    commit_file(&repository, "seed.txt", "seed\n", "base");
    let base = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("base", &base, true).unwrap();

    for path in ["space name.txt", "line\nbreak.txt", "你好.txt"] {
        commit_file(&repository, path, "special\n", "special path");
    }
    let head = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("head", &head, true).unwrap();

    let review = open_comparison(fixture.path());
    let mut paths = review
        .snapshot()
        .file_changes
        .iter()
        .map(|change| change.path.as_str())
        .collect::<Vec<_>>();
    paths.sort();

    assert_eq!(paths, vec!["line\nbreak.txt", "space name.txt", "你好.txt"]);
    assert_eq!(
        review
            .read_file_change("line\nbreak.txt")
            .unwrap()
            .after
            .as_deref(),
        Some("special\n")
    );
}

#[test]
fn distinguishes_executable_mode_changes_in_snapshot_metadata() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    let blob = repository.blob(b"#!/bin/sh\n").unwrap();
    commit_file(&repository, "seed.txt", "seed\n", "seed");
    commit_tree_entry(&repository, "script.sh", blob, 0o100644, "non executable");
    let base = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("base", &base, true).unwrap();
    commit_tree_entry(&repository, "script.sh", blob, 0o100755, "executable");
    let head = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("head", &head, true).unwrap();

    let review = open_comparison(fixture.path());
    let change = &review.snapshot().file_changes[0];

    assert_eq!(change.status, ChangeStatus::Modified);
    assert_eq!(change.before_mode, Some(0o100644));
    assert_eq!(change.after_mode, Some(0o100755));
    assert_eq!(change.before_object_id, change.after_object_id);
}

#[test]
fn recovers_after_a_failed_revision_resolution() {
    let fixture = TempDir::new().expect("fixture directory");
    let repository = Repository::init(fixture.path()).expect("initialize fixture repo");
    commit_file(&repository, "README.md", "same\n", "initial");
    let commit = repository.head().unwrap().peel_to_commit().unwrap();
    repository.branch("base", &commit, true).unwrap();
    repository.branch("head", &commit, true).unwrap();

    assert!(
        LocalOpenedReview::open(fixture.path(), "refs/heads/missing", "refs/heads/head").is_err()
    );
    let recovered = open_comparison(fixture.path());

    assert!(recovered.snapshot().file_changes.is_empty());
}

#[test]
fn compares_canonical_refs_in_a_real_shallow_clone() {
    let source_fixture = TempDir::new().expect("source directory");
    let source = Repository::init(source_fixture.path()).expect("initialize source");
    commit_file(&source, "README.md", "base\n", "base");
    let base = source.head().unwrap().peel_to_commit().unwrap();
    source.branch("base", &base, true).unwrap();
    commit_file(&source, "README.md", "head\n", "head");
    let head = source.head().unwrap().peel_to_commit().unwrap();
    source.branch("head", &head, true).unwrap();

    let clone_fixture = TempDir::new().expect("clone parent");
    let clone_path = clone_fixture.path().join("shallow");
    let source_url = format!("file://{}", source_fixture.path().display());
    let status = Command::new("git")
        .args(["clone", "--quiet", "--depth=1", "--branch=head"])
        .arg(source_url)
        .arg(&clone_path)
        .status()
        .expect("run shallow clone");
    assert!(status.success());

    let refs = list_refs(&clone_path).expect("list shallow refs");
    assert!(refs.contains(&"refs/heads/head".to_owned()));
    assert!(refs.contains(&"refs/remotes/origin/head".to_owned()));
    let review =
        LocalOpenedReview::open(&clone_path, "refs/heads/head", "refs/remotes/origin/head")
            .expect("compare shallow refs");

    assert_eq!(
        review.snapshot().base_revision,
        review.snapshot().head_revision
    );
    assert!(review.snapshot().file_changes.is_empty());
}

#[test]
fn selected_remote_fetch_observes_force_push_without_credentials_injection() {
    let remote_fixture = TempDir::new().expect("remote directory");
    let remote = Repository::init_bare(remote_fixture.path()).expect("initialize bare remote");
    let source_fixture = TempDir::new().expect("source directory");
    let source = Repository::init(source_fixture.path()).expect("initialize source");
    commit_file(&source, "README.md", "first\n", "first");
    source
        .remote("origin", remote_fixture.path().to_str().unwrap())
        .unwrap();
    assert!(Command::new("git")
        .args(["push", "origin", "HEAD:refs/heads/topic"])
        .current_dir(source_fixture.path())
        .status()
        .unwrap()
        .success());
    let clone_fixture = TempDir::new().expect("clone parent");
    let clone_path = clone_fixture.path().join("clone");
    assert!(Command::new("git")
        .args(["clone", "--quiet"])
        .arg(remote_fixture.path())
        .arg(&clone_path)
        .status()
        .unwrap()
        .success());
    let old_head = remote.refname_to_id("refs/heads/topic").unwrap();

    std::fs::write(source_fixture.path().join("README.md"), "forced\n").unwrap();
    assert!(Command::new("git")
        .args(["add", "README.md"])
        .current_dir(source_fixture.path())
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["commit", "--amend", "--quiet", "-m", "forced"])
        .current_dir(source_fixture.path())
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["push", "--force", "origin", "HEAD:refs/heads/topic"])
        .current_dir(source_fixture.path())
        .status()
        .unwrap()
        .success());
    let forced_head = source.head().unwrap().target().unwrap();

    fetch_selected_remote_refs(
        &clone_path,
        "refs/remotes/origin/topic",
        "refs/remotes/origin/topic",
        &CancellationToken::default(),
    )
    .expect("fetch selected remote");

    let clone = Repository::open(&clone_path).unwrap();
    assert_eq!(
        clone.refname_to_id("refs/remotes/origin/topic").unwrap(),
        forced_head
    );
    assert_ne!(forced_head, old_head);
}

#[test]
fn selected_remote_fetch_reports_transport_failure_without_moving_tracking_ref() {
    let fixture = TempDir::new().expect("repository directory");
    let repository = Repository::init(fixture.path()).expect("initialize repository");
    let head = commit_file(&repository, "README.md", "stable\n", "stable");
    repository
        .reference("refs/remotes/origin/topic", head, true, "fixture")
        .unwrap();
    repository
        .remote("origin", "/definitely/missing/narrant-remote")
        .unwrap();

    let error = fetch_selected_remote_refs(
        fixture.path(),
        "refs/remotes/origin/topic",
        "refs/remotes/origin/topic",
        &CancellationToken::default(),
    )
    .unwrap_err();

    assert!(error.to_string().contains("Core Git 失败"));
    assert_eq!(
        repository
            .refname_to_id("refs/remotes/origin/topic")
            .unwrap(),
        head
    );
}

#[test]
fn selected_remote_fetch_reports_http_authentication_failure_without_moving_ref() {
    let server = MockServer::start();
    let unauthorized = server.mock(|when, then| {
        when.method(GET);
        then.status(401)
            .header("WWW-Authenticate", "Basic realm=\"Narrant test\"");
    });
    let fixture = TempDir::new().expect("repository directory");
    let repository = Repository::init(fixture.path()).expect("initialize repository");
    let head = commit_file(&repository, "README.md", "stable\n", "stable");
    repository
        .reference("refs/remotes/origin/topic", head, true, "fixture")
        .unwrap();
    repository
        .remote("origin", &server.url("/private.git"))
        .unwrap();

    let error = fetch_selected_remote_refs(
        fixture.path(),
        "refs/remotes/origin/topic",
        "refs/remotes/origin/topic",
        &CancellationToken::default(),
    )
    .unwrap_err();

    assert!(error.to_string().contains("Core Git 失败"));
    assert_eq!(
        repository
            .refname_to_id("refs/remotes/origin/topic")
            .unwrap(),
        head
    );
    assert!(unauthorized.hits() > 0);
}

#[test]
fn comparison_without_a_common_ancestor_fails_without_mutating_refs() {
    let fixture = TempDir::new().expect("repository directory");
    let repository = Repository::init(fixture.path()).expect("initialize repository");
    let base = commit_file(&repository, "base.txt", "base\n", "base");
    repository
        .branch("base", &repository.find_commit(base).unwrap(), true)
        .unwrap();
    assert!(Command::new("git")
        .args(["checkout", "--orphan", "head"])
        .current_dir(fixture.path())
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["rm", "-rf", "--quiet", "."])
        .current_dir(fixture.path())
        .status()
        .unwrap()
        .success());
    commit_file(&repository, "head.txt", "head\n", "head");
    let head = repository.head().unwrap().target().unwrap();

    let error = match LocalOpenedReview::open(fixture.path(), "refs/heads/base", "refs/heads/head")
    {
        Ok(_) => panic!("unrelated histories must not produce a comparison"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("Core Git"));
    assert_eq!(repository.refname_to_id("refs/heads/base").unwrap(), base);
    assert_eq!(repository.refname_to_id("refs/heads/head").unwrap(), head);
}
