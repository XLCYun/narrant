use super::*;
use std::process::Command;

/// 创建一个带初始提交的真实 working-tree repository。
fn create_repository() -> tempfile::TempDir {
    let fixture = tempfile::TempDir::new().unwrap();
    for arguments in [
        vec!["init", "-q"],
        vec!["config", "user.email", "test@narrant.invalid"],
        vec!["config", "user.name", "Narrant Test"],
    ] {
        assert!(Command::new("git")
            .args(arguments)
            .current_dir(fixture.path())
            .status()
            .unwrap()
            .success());
    }
    std::fs::write(fixture.path().join("README.md"), "fixture").unwrap();
    assert!(Command::new("git")
        .args(["add", "README.md"])
        .current_dir(fixture.path())
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["commit", "-q", "-m", "initial"])
        .current_dir(fixture.path())
        .status()
        .unwrap()
        .success());
    fixture
}

#[test]
fn registered_repository_survives_catalog_reopen() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let repository = create_repository();

    let entry_id = {
        let catalog = RepositoryCatalog::open(database.path()).unwrap();
        catalog.register(repository.path()).unwrap().id
    };
    let catalog = RepositoryCatalog::open(database.path()).unwrap();
    let entries = catalog.list().unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, entry_id);
    assert_eq!(
        entries[0].canonical_path,
        repository.path().canonicalize().unwrap()
    );
    assert!(entries[0].available);
    assert_eq!(entries[0].created_at, entries[0].last_opened_at);
}

#[test]
fn separate_clones_of_the_same_remote_remain_separate_entries() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let source = create_repository();
    let clone_parent = tempfile::TempDir::new().unwrap();
    let clone_path = clone_parent.path().join("clone");
    assert!(Command::new("git")
        .args(["clone", "-q"])
        .arg(source.path())
        .arg(&clone_path)
        .status()
        .unwrap()
        .success());
    let catalog = RepositoryCatalog::open(database.path()).unwrap();

    let source_entry = catalog.register(source.path()).unwrap();
    let clone_entry = catalog.register(&clone_path).unwrap();

    assert_ne!(source_entry.id, clone_entry.id);
    assert_eq!(catalog.list().unwrap().len(), 2);
}

#[test]
fn opening_repository_does_not_change_repository_list_order() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let catalog = RepositoryCatalog::open(database.path()).unwrap();
    let older = create_repository();
    let newer = create_repository();
    let older_entry = catalog.register(older.path()).unwrap();
    let newer_entry = catalog.register(newer.path()).unwrap();

    assert_eq!(
        catalog
            .list()
            .unwrap()
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec![newer_entry.id, older_entry.id],
    );
    catalog.open_entry(older_entry.id).unwrap();
    assert_eq!(
        catalog
            .list()
            .unwrap()
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec![newer_entry.id, older_entry.id],
    );
}

#[test]
fn registering_the_same_clone_reopens_the_existing_entry() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let repository = create_repository();
    let catalog = RepositoryCatalog::open(database.path()).unwrap();

    let first = catalog.register(repository.path()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let reopened = catalog.register(repository.path()).unwrap();

    assert_eq!(reopened.id, first.id);
    assert_eq!(reopened.created_at, first.created_at);
    assert!(reopened.last_opened_at > first.last_opened_at);
    assert_eq!(catalog.list().unwrap().len(), 1);
}

#[test]
fn missing_repository_remains_visible_and_unavailable() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let repository = create_repository();
    let catalog = RepositoryCatalog::open(database.path()).unwrap();
    let entry = catalog.register(repository.path()).unwrap();
    drop(repository);

    let saved = catalog.open_entry(entry.id).unwrap();

    assert!(!saved.available);
    assert!(saved.unavailable_reason.unwrap().contains("路径不存在"));
}

#[test]
fn registering_a_non_working_tree_reports_an_actionable_error() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let directory = tempfile::TempDir::new().unwrap();
    let catalog = RepositoryCatalog::open(database.path()).unwrap();

    let error = catalog.register(directory.path()).unwrap_err();

    assert!(error.to_string().contains("working-tree Git 仓库"));
}

#[test]
fn corrupt_database_reports_a_recovery_error() {
    let database = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(database.path(), "not sqlite").unwrap();

    let error = RepositoryCatalog::open(database.path()).unwrap_err();

    assert!(error.to_string().contains("Repository Catalog 无法打开"));
    assert!(error.to_string().contains("移走或删除"));
}

#[test]
fn workspace_trust_survives_catalog_reopen_for_one_repository_entry() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let repository = create_repository();

    let entry_id = {
        let catalog = RepositoryCatalog::open(database.path()).unwrap();
        let entry = catalog.register(repository.path()).unwrap();
        assert!(!catalog.is_workspace_trusted(entry.id).unwrap());
        catalog.grant_workspace_trust(entry.id).unwrap();
        entry.id
    };

    let catalog = RepositoryCatalog::open(database.path()).unwrap();
    assert!(catalog.is_workspace_trusted(entry_id).unwrap());
}

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
    let snapshot = crate::change_content::local::LocalOpenedReview::open(
        repository.path(),
        &branch_ref,
        &branch_ref,
    )
    .unwrap();

    let first = catalog
        .save_branch_review(entry.id, snapshot.snapshot())
        .unwrap();
    catalog.activate_snapshot(&first).unwrap();
    let reopened = catalog
        .save_branch_review(entry.id, snapshot.snapshot())
        .unwrap();
    let loaded = catalog.open_review(&first.review_id).unwrap();

    assert_eq!(reopened.review_id, first.review_id);
    assert_eq!(loaded.review_id, first.review_id);
    assert_eq!(loaded.base_ref, branch_ref);
    assert_eq!(loaded.head_ref, branch_ref);
    assert_eq!(loaded.head_revision, snapshot.snapshot().head_revision);
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
    let snapshot = crate::change_content::local::LocalOpenedReview::open(
        repository.path(),
        &branch_ref,
        &branch_ref,
    )
    .unwrap();
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
        synchronized_at: 1,
    };

    let first = catalog
        .save_pull_request_review(entry.id, &first_metadata, snapshot.snapshot())
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
            snapshot.snapshot(),
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
    let opened = crate::change_content::local::LocalOpenedReview::open(
        repository.path(),
        &branch_ref,
        &branch_ref,
    )
    .unwrap();
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
        synchronized_at: 1,
    };
    let first = catalog
        .save_pull_request_review(entry.id, &metadata, opened.snapshot())
        .unwrap();
    catalog.activate_snapshot(&first).unwrap();
    let mut next_snapshot = opened.snapshot().clone();
    next_snapshot.head_revision = "2222222222222222222222222222222222222222".to_owned();

    let staged = catalog
        .stage_pull_request_snapshot(&first, &metadata, &next_snapshot)
        .unwrap();
    assert_eq!(
        catalog.open_review(&first.review_id).unwrap().head_revision,
        first.head_revision
    );

    catalog.activate_snapshot(&staged).unwrap();
    assert_eq!(
        catalog.open_review(&first.review_id).unwrap().head_revision,
        next_snapshot.head_revision
    );
}

#[test]
fn refresh_candidate_stays_non_current_until_explicit_activation() {
    let application_data = tempfile::TempDir::new().unwrap();
    let database_path = application_data.path().join("catalog.sqlite3");
    let repository = create_repository();
    let catalog = RepositoryCatalog::open(&database_path).unwrap();
    let entry = catalog.register(repository.path()).unwrap();
    catalog.grant_workspace_trust(entry.id).unwrap();
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
    let initial = crate::change_content::local::LocalOpenedReview::open(
        repository.path(),
        base_ref,
        head_ref,
    )
    .unwrap();
    let saved = catalog
        .save_branch_review(entry.id, initial.snapshot())
        .unwrap();
    catalog.activate_snapshot(&saved).unwrap();
    let old_head = saved.head_revision.clone();
    let workspace = catalog.review_workspace_path(&saved.review_id).unwrap();
    GitProcessAdapter::new(repository.path())
        .create_detached_worktree(&workspace, &old_head, &CancellationToken::default())
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
    let latest = crate::change_content::local::LocalOpenedReview::open(
        repository.path(),
        base_ref,
        head_ref,
    )
    .unwrap();

    let candidate = catalog
        .save_refresh_snapshot(&saved.review_id, latest.snapshot())
        .unwrap();
    assert_eq!(
        catalog.open_review(&saved.review_id).unwrap().head_revision,
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
            &entry.common_directory,
            &old_head,
            &candidate.head_revision,
            &CancellationToken::default(),
        )
        .unwrap();
    catalog
        .activate_refresh_snapshot(&candidate, &saved)
        .unwrap();
    let refreshed = catalog.open_review(&saved.review_id).unwrap();
    assert_eq!(refreshed.head_revision, latest.snapshot().head_revision);
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
    let catalog = RepositoryCatalog::open(database.path()).unwrap();
    let first_entry = catalog.register(first_repository.path()).unwrap();
    let second_entry = catalog.register(second_repository.path()).unwrap();
    catalog.grant_workspace_trust(first_entry.id).unwrap();
    catalog.grant_workspace_trust(second_entry.id).unwrap();

    let first_snapshot = snapshot_for_head(&first_repository);
    let first_review = catalog
        .save_branch_review(first_entry.id, &first_snapshot)
        .unwrap();
    catalog.activate_snapshot(&first_review).unwrap();
    let second_snapshot = snapshot_for_head(&second_repository);
    let second_review = catalog
        .save_branch_review(second_entry.id, &second_snapshot)
        .unwrap();
    catalog.activate_snapshot(&second_review).unwrap();

    let repository_reviews = catalog.list_reviews(Some(first_entry.id)).unwrap();
    let recent_reviews = catalog.list_reviews(None).unwrap();

    assert_eq!(repository_reviews.len(), 1);
    assert_eq!(repository_reviews[0].review_id, first_review.review_id);
    assert_eq!(repository_reviews[0].repository_entry_id, first_entry.id);
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
) -> crate::change_content::local::LocalSnapshot {
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
    crate::change_content::local::LocalOpenedReview::open(
        repository.path(),
        &branch_ref,
        &branch_ref,
    )
    .unwrap()
    .snapshot()
    .clone()
}
