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
