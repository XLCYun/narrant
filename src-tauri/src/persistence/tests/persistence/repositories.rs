#[test]
fn registered_repository_survives_persistence_reopen() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let repository = create_repository();

    let entry_id = {
        let store = ReviewStore::open(database.path()).unwrap();
        store.upsert_repository(&verify_repository_location(repository.path()).unwrap()).unwrap().id
    };
    let store = ReviewStore::open(database.path()).unwrap();
    let entries = store.list_repositories().unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].id, entry_id);
    assert_eq!(
        entries[0].canonical_path,
        repository.path().canonicalize().unwrap()
    );
    assert_eq!(entries[0].created_at_millis, entries[0].last_opened_at_millis);
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
    let store = ReviewStore::open(database.path()).unwrap();

    let source_entry = store.upsert_repository(&verify_repository_location(source.path()).unwrap()).unwrap();
    let clone_entry = store.upsert_repository(&verify_repository_location(&clone_path).unwrap()).unwrap();

    assert_ne!(source_entry.id, clone_entry.id);
    assert_eq!(store.list_repositories().unwrap().len(), 2);
}

#[test]
fn opening_repository_does_not_change_repository_list_order() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let store = ReviewStore::open(database.path()).unwrap();
    let older = create_repository();
    let newer = create_repository();
    let older_entry = store.upsert_repository(&verify_repository_location(older.path()).unwrap()).unwrap();
    let newer_entry = store.upsert_repository(&verify_repository_location(newer.path()).unwrap()).unwrap();

    assert_eq!(
        store
            .list_repositories()
            .unwrap()
            .iter()
            .map(|entry| entry.id)
            .collect::<Vec<_>>(),
        vec![newer_entry.id, older_entry.id],
    );
    store.get_repository(older_entry.id).unwrap();
    assert_eq!(
        store
            .list_repositories()
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
    let store = ReviewStore::open(database.path()).unwrap();

    let first = store.upsert_repository(&verify_repository_location(repository.path()).unwrap()).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let reopened = store.upsert_repository(&verify_repository_location(repository.path()).unwrap()).unwrap();

    assert_eq!(reopened.id, first.id);
    assert_eq!(reopened.created_at_millis, first.created_at_millis);
    assert!(reopened.last_opened_at_millis > first.last_opened_at_millis);
    assert_eq!(store.list_repositories().unwrap().len(), 1);
}

#[test]
fn missing_repository_remains_persisted_without_runtime_validation() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let repository = create_repository();
    let store = ReviewStore::open(database.path()).unwrap();
    let entry = store.upsert_repository(&verify_repository_location(repository.path()).unwrap()).unwrap();
    drop(repository);

    let saved = store.get_repository(entry.id).unwrap();

    assert_eq!(saved.id, entry.id);
}

#[test]
fn registering_a_non_working_tree_reports_an_actionable_error() {
    let directory = tempfile::TempDir::new().unwrap();
    let error = verify_repository_location(directory.path()).unwrap_err();

    assert!(error.to_string().contains("working-tree Git 仓库"));
}

#[test]
fn corrupt_database_reports_a_recovery_error() {
    let database = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(database.path(), "not sqlite").unwrap();

    let error = ReviewStore::open(database.path()).unwrap_err();

    assert!(error.to_string().contains("Persistence 数据库无法打开"));
    assert!(error.to_string().contains("移走或删除"));
}

#[test]
fn workspace_trust_survives_persistence_reopen_for_one_repository() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let repository = create_repository();

    let entry_id = {
        let store = ReviewStore::open(database.path()).unwrap();
        let entry = store.upsert_repository(&verify_repository_location(repository.path()).unwrap()).unwrap();
        assert!(!store.has_workspace_trust(entry.id).unwrap());
        store.insert_workspace_trust(entry.id).unwrap();
        entry.id
    };

    let store = ReviewStore::open(database.path()).unwrap();
    assert!(store.has_workspace_trust(entry_id).unwrap());
}
