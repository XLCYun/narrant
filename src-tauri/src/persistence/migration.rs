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
    if version < 5 {
        connection.execute_batch("PRAGMA foreign_keys = OFF;")?;
        let transaction = connection.transaction()?;
        transaction.execute_batch(
            "CREATE TABLE repositories_v5 (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                canonical_path TEXT NOT NULL UNIQUE,
                git_common_directory TEXT NOT NULL,
                created_at_millis INTEGER NOT NULL,
                last_opened_at_millis INTEGER NOT NULL
             );
             INSERT INTO repositories_v5
                (id, canonical_path, git_common_directory, created_at_millis, last_opened_at_millis)
             SELECT id, canonical_path, common_directory, created_at, last_opened_at
             FROM repository_entries;

             CREATE TABLE workspace_trust_v5 (
                repository_id INTEGER PRIMARY KEY
                    REFERENCES repositories_v5(id) ON DELETE CASCADE,
                granted_at_millis INTEGER NOT NULL
             );
             INSERT INTO workspace_trust_v5 (repository_id, granted_at_millis)
             SELECT repository_entry_id, granted_at FROM workspace_trust;

             CREATE TABLE reviews_v5 (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                repository_id INTEGER NOT NULL
                    REFERENCES repositories_v5(id) ON DELETE CASCADE,
                selection_kind TEXT NOT NULL CHECK(selection_kind IN ('branch', 'pull_request')),
                branch_base_ref TEXT,
                branch_head_ref TEXT,
                github_repository_owner TEXT,
                github_repository_name TEXT,
                pull_request_number INTEGER,
                current_snapshot_id INTEGER,
                created_at_millis INTEGER NOT NULL,
                last_opened_at_millis INTEGER NOT NULL,
                pull_request_metadata TEXT,
                CHECK(
                    (selection_kind = 'branch'
                     AND branch_base_ref IS NOT NULL AND branch_head_ref IS NOT NULL
                     AND github_repository_owner IS NULL AND github_repository_name IS NULL
                     AND pull_request_number IS NULL)
                    OR
                    (selection_kind = 'pull_request'
                     AND branch_base_ref IS NULL AND branch_head_ref IS NULL
                     AND github_repository_owner IS NOT NULL AND github_repository_name IS NOT NULL
                     AND pull_request_number IS NOT NULL)
                )
             );
             INSERT INTO reviews_v5 (
                id, repository_id, selection_kind, branch_base_ref, branch_head_ref,
                github_repository_owner, github_repository_name, pull_request_number,
                current_snapshot_id, created_at_millis, last_opened_at_millis,
                pull_request_metadata
             )
             SELECT id, repository_entry_id, selection_kind,
                    CASE WHEN selection_kind = 'branch' THEN base_ref END,
                    CASE WHEN selection_kind = 'branch' THEN head_ref END,
                    CASE WHEN selection_kind = 'pull_request'
                         THEN substr(base_ref, 1, instr(base_ref, '/') - 1) END,
                    CASE WHEN selection_kind = 'pull_request'
                         THEN substr(base_ref, instr(base_ref, '/') + 1) END,
                    CASE WHEN selection_kind = 'pull_request' THEN CAST(head_ref AS INTEGER) END,
                    current_snapshot_id, created_at, last_opened_at, pull_request_metadata
             FROM reviews;

             CREATE TABLE review_snapshots_v5 (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                review_id INTEGER NOT NULL REFERENCES reviews_v5(id) ON DELETE CASCADE,
                base_revision TEXT NOT NULL,
                comparison_base TEXT NOT NULL,
                head_revision TEXT NOT NULL,
                created_at_millis INTEGER NOT NULL,
                activated_at_millis INTEGER,
                pull_request_metadata TEXT
             );
             INSERT INTO review_snapshots_v5
                (id, review_id, base_revision, comparison_base, head_revision,
                 created_at_millis, activated_at_millis, pull_request_metadata)
             SELECT id, review_id, base_revision, comparison_base, head_revision,
                    created_at, activated_at, pull_request_metadata
             FROM review_snapshots;

             CREATE TABLE file_changes_v5 (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                snapshot_id INTEGER NOT NULL
                    REFERENCES review_snapshots_v5(id) ON DELETE CASCADE,
                path TEXT NOT NULL,
                old_path TEXT,
                status TEXT NOT NULL,
                before_mode INTEGER,
                after_mode INTEGER,
                before_object_id TEXT,
                after_object_id TEXT
             );
             INSERT INTO file_changes_v5 SELECT * FROM file_changes;

             DROP TABLE file_changes;
             DROP TABLE review_snapshots;
             DROP TABLE reviews;
             DROP TABLE workspace_trust;
             DROP TABLE repository_entries;
             ALTER TABLE repositories_v5 RENAME TO repositories;
             ALTER TABLE workspace_trust_v5 RENAME TO workspace_trust;
             ALTER TABLE reviews_v5 RENAME TO reviews;
             ALTER TABLE review_snapshots_v5 RENAME TO review_snapshots;
             ALTER TABLE file_changes_v5 RENAME TO file_changes;
             CREATE UNIQUE INDEX reviews_branch_selection_unique
                ON reviews(repository_id, branch_base_ref, branch_head_ref)
                WHERE selection_kind = 'branch';
             CREATE UNIQUE INDEX reviews_pull_request_selection_unique
                ON reviews(repository_id, github_repository_owner,
                           github_repository_name, pull_request_number)
                WHERE selection_kind = 'pull_request';
             PRAGMA user_version = 5;",
        )?;
        transaction.commit()?;
        connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    }
    Ok(())
}
