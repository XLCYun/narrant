use narrant_lib::repository_catalog::RepositoryCatalog;
use std::process::Command;

/// 创建供 Catalog 黑盒 integration 使用的真实 working-tree repository。
fn create_repository() -> tempfile::TempDir {
    let repository = tempfile::TempDir::new().unwrap();
    for arguments in [
        &["init", "-q"][..],
        &["config", "user.email", "integration@narrant.invalid"][..],
        &["config", "user.name", "Narrant Integration"][..],
    ] {
        assert!(Command::new("git")
            .args(arguments)
            .current_dir(repository.path())
            .status()
            .unwrap()
            .success());
    }
    repository
}

#[test]
fn add_and_reopen_repository_entry_through_the_public_catalog_boundary() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let repository = create_repository();
    let entry_id = RepositoryCatalog::open(database.path())
        .unwrap()
        .register(repository.path())
        .unwrap()
        .id;

    let reopened = RepositoryCatalog::open(database.path())
        .unwrap()
        .open_entry(entry_id)
        .unwrap();

    assert!(reopened.available);
    assert_eq!(
        reopened.canonical_path,
        repository.path().canonicalize().unwrap()
    );
}
