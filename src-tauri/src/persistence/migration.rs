use super::*;

/// 为新安装创建当前完整 schema；已升级的数据库无需再执行历史迁移。
pub(super) fn migrate(connection: &mut Connection) -> rusqlite::Result<()> {
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version == CURRENT_SCHEMA_VERSION {
        return Ok(());
    }
    if version != 0 {
        return Err(rusqlite::Error::InvalidQuery);
    }

    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "CREATE TABLE repositories (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            canonical_path TEXT NOT NULL UNIQUE,
            git_common_directory TEXT NOT NULL,
            created_at_millis INTEGER NOT NULL,
            last_opened_at_millis INTEGER NOT NULL
         );
         CREATE TABLE workspace_trust (
            repository_id INTEGER PRIMARY KEY
                REFERENCES repositories(id) ON DELETE CASCADE,
            granted_at_millis INTEGER NOT NULL
         );
         CREATE TABLE reviews (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            repository_id INTEGER NOT NULL
                REFERENCES repositories(id) ON DELETE CASCADE,
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
         CREATE TABLE review_snapshots (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            review_id INTEGER NOT NULL REFERENCES reviews(id) ON DELETE CASCADE,
            base_revision TEXT NOT NULL,
            comparison_base TEXT NOT NULL,
            head_revision TEXT NOT NULL,
            created_at_millis INTEGER NOT NULL,
            activated_at_millis INTEGER,
            pull_request_metadata TEXT
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
         CREATE UNIQUE INDEX reviews_branch_selection_unique
            ON reviews(repository_id, branch_base_ref, branch_head_ref)
            WHERE selection_kind = 'branch';
         CREATE UNIQUE INDEX reviews_pull_request_selection_unique
            ON reviews(repository_id, github_repository_owner,
                       github_repository_name, pull_request_number)
            WHERE selection_kind = 'pull_request';
         PRAGMA user_version = 5;",
    )?;
    transaction.commit()
}
