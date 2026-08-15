use super::*;
use crate::review::PullRequestState;
use std::process::Command;

#[test]
fn version_three_migration_preserves_existing_snapshots_as_activated_history() {
    let database = tempfile::NamedTempFile::new().unwrap();
    let connection = rusqlite::Connection::open(database.path()).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE repository_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                canonical_path TEXT NOT NULL UNIQUE,
                common_directory TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_opened_at INTEGER NOT NULL
             );
             CREATE TABLE workspace_trust (
                repository_entry_id INTEGER PRIMARY KEY
                    REFERENCES repository_entries(id) ON DELETE CASCADE,
                granted_at INTEGER NOT NULL
             );
             CREATE TABLE reviews (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                repository_entry_id INTEGER NOT NULL
                    REFERENCES repository_entries(id) ON DELETE CASCADE,
                selection_kind TEXT NOT NULL,
                base_ref TEXT NOT NULL,
                head_ref TEXT NOT NULL,
                current_snapshot_id INTEGER,
                created_at INTEGER NOT NULL,
                last_opened_at INTEGER NOT NULL,
                pull_request_metadata TEXT,
                UNIQUE(repository_entry_id, selection_kind, base_ref, head_ref)
             );
             CREATE TABLE review_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                review_id INTEGER NOT NULL REFERENCES reviews(id) ON DELETE CASCADE,
                base_revision TEXT NOT NULL,
                comparison_base TEXT NOT NULL,
                head_revision TEXT NOT NULL,
                created_at INTEGER NOT NULL
             );
             CREATE TABLE file_changes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                snapshot_id INTEGER NOT NULL
                    REFERENCES review_snapshots(id) ON DELETE CASCADE,
                path TEXT NOT NULL,
                old_path TEXT,
                status TEXT NOT NULL,
                before_mode INTEGER,
                after_mode INTEGER,
                before_object_id TEXT,
                after_object_id TEXT
             );
             INSERT INTO repository_entries
                (id, canonical_path, common_directory, created_at, last_opened_at)
                VALUES (1, '/tmp/repository', '/tmp/repository/.git', 1, 1);
             INSERT INTO reviews
                (id, repository_entry_id, selection_kind, base_ref, head_ref,
                 current_snapshot_id, created_at, last_opened_at)
                VALUES (1, 1, 'branch', 'refs/heads/main', 'refs/heads/topic', 1, 1, 1);
             INSERT INTO review_snapshots
                (id, review_id, base_revision, comparison_base, head_revision, created_at)
                VALUES (1, 1, 'base', 'merge-base', 'head', 1);
             PRAGMA user_version = 3;",
        )
        .unwrap();
    drop(connection);

    let store = ReviewStore::open(database.path()).unwrap();

    assert_eq!(
        store.list_review_snapshot_identities("review-1").unwrap(),
        vec!["snapshot-1"]
    );
}

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
