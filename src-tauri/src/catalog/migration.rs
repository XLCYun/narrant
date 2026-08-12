use super::*;

/// 执行第一版 schema migration，不在 SQLite 中创建任何 secret 字段。
pub(super) fn migrate(connection: &mut Connection) -> rusqlite::Result<()> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version > CURRENT_SCHEMA_VERSION {
        return Err(rusqlite::Error::InvalidQuery);
    }
    if version == 0 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "CREATE TABLE repository_entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                canonical_path TEXT NOT NULL UNIQUE,
                common_directory TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_opened_at INTEGER NOT NULL
             );
             PRAGMA user_version = 1;",
        )?;
        transaction.commit()?;
    }
    if version < 2 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "CREATE TABLE workspace_trust (
                repository_entry_id INTEGER PRIMARY KEY
                    REFERENCES repository_entries(id) ON DELETE CASCADE,
                granted_at INTEGER NOT NULL
             );
             CREATE TABLE reviews (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                repository_entry_id INTEGER NOT NULL
                    REFERENCES repository_entries(id) ON DELETE CASCADE,
                selection_kind TEXT NOT NULL CHECK(selection_kind IN ('branch', 'pull_request')),
                base_ref TEXT NOT NULL,
                head_ref TEXT NOT NULL,
                current_snapshot_id INTEGER,
                created_at INTEGER NOT NULL,
                last_opened_at INTEGER NOT NULL,
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
             PRAGMA user_version = 2;",
        )?;
        transaction.commit()?;
    }
    if version < 3 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "ALTER TABLE reviews ADD COLUMN pull_request_metadata TEXT;
             PRAGMA user_version = 3;",
        )?;
        transaction.commit()?;
    }
    if version < 4 {
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "ALTER TABLE review_snapshots ADD COLUMN activated_at INTEGER;
             ALTER TABLE review_snapshots ADD COLUMN pull_request_metadata TEXT;
             UPDATE review_snapshots SET activated_at = created_at;
             PRAGMA user_version = 4;",
        )?;
        transaction.commit()?;
    }
    Ok(())
}
