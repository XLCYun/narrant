use super::*;

impl ReviewStore {
    /// 打开应用数据库并执行显式 schema migration。
    pub fn open(database_path: &Path) -> Result<Self, PersistenceError> {
        if let Some(parent) = database_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                PersistenceError::new(format!("无法创建 Persistence 数据库目录：{error}"))
            })?;
        }
        let mut connection = Connection::open(database_path)
            .and_then(|connection| {
                connection.execute_batch(
                    "PRAGMA foreign_keys = ON;
                     PRAGMA journal_mode = WAL;
                     PRAGMA synchronous = FULL;",
                )?;
                Ok(connection)
            })
            .map_err(map_sqlite_error)?;
        migrate(&mut connection).map_err(map_sqlite_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
            workspace_root: database_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("reviews"),
        })
    }

    /// 保存经 Repository Application 验证的仓库位置；重复路径更新原记录。
    pub(crate) fn upsert_repository(
        &self,
        location: &RepositoryLocation,
    ) -> Result<Repository, PersistenceError> {
        let now = current_unix_time_millis()?;
        let mut connection = self.lock_connection()?;
        let transaction = connection.transaction().map_err(map_sqlite_error)?;
        transaction
            .execute(
                "INSERT INTO repositories (
                    canonical_path, git_common_directory, created_at_millis, last_opened_at_millis
                 ) VALUES (?1, ?2, ?3, ?3)
                 ON CONFLICT(canonical_path) DO UPDATE SET
                    git_common_directory = excluded.git_common_directory,
                    last_opened_at_millis = excluded.last_opened_at_millis",
                params![
                    location.canonical_path.to_string_lossy(),
                    location.git_common_directory.to_string_lossy(),
                    now
                ],
            )
            .map_err(map_sqlite_error)?;
        let repository_query =
            format!("SELECT {REPOSITORY_COLUMNS} FROM repositories WHERE canonical_path = ?1");
        let repository = transaction
            .query_row(
                &repository_query,
                [location.canonical_path.to_string_lossy()],
                repository_from_row,
            )
            .map_err(map_sqlite_error)?;
        transaction.commit().map_err(map_sqlite_error)?;
        Ok(repository)
    }

    /// 按添加顺序倒序列出全部入口，打开活动不改变仓库位置。
    pub fn list_repositories(&self) -> Result<Vec<Repository>, PersistenceError> {
        let connection = self.lock_connection()?;
        let list_query = format!(
            "SELECT {REPOSITORY_COLUMNS} FROM repositories
             ORDER BY id DESC"
        );
        let mut statement = connection.prepare(&list_query).map_err(map_sqlite_error)?;
        let repositories = statement
            .query_map([], repository_from_row)
            .map_err(map_sqlite_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(map_sqlite_error)?;
        Ok(repositories)
    }

    /// 按稳定身份读取 Repository，不执行文件系统或 Git 检查。
    pub fn get_repository(&self, id: i64) -> Result<Repository, PersistenceError> {
        let connection = self.lock_connection()?;
        let repository_query =
            format!("SELECT {REPOSITORY_COLUMNS} FROM repositories WHERE id = ?1");
        connection
            .query_row(&repository_query, [id], repository_from_row)
            .optional()
            .map_err(map_sqlite_error)?
            .ok_or_else(|| PersistenceError::new("Repository 不存在。"))
    }

    /// 记录一次成功打开 Repository 的活动时间。
    pub fn record_repository_opened(&self, id: i64) -> Result<(), PersistenceError> {
        let now = current_unix_time_millis()?;
        let connection = self.lock_connection()?;
        let changed = connection
            .execute(
                "UPDATE repositories SET last_opened_at_millis = ?1 WHERE id = ?2",
                params![now, id],
            )
            .map_err(map_sqlite_error)?;
        if changed == 1 {
            Ok(())
        } else {
            Err(PersistenceError::new("Repository 不存在。"))
        }
    }

    /// 返回 Repository 是否已经存在持久化 Workspace Trust 记录。
    pub fn has_workspace_trust(&self, id: i64) -> Result<bool, PersistenceError> {
        let connection = self.lock_connection()?;
        connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM workspace_trust WHERE repository_id = ?1
                 )",
                [id],
                |row| row.get(0),
            )
            .map_err(map_sqlite_error)
    }

    /// 幂等插入 Repository 的 Workspace Trust 记录。
    pub fn insert_workspace_trust(&self, id: i64) -> Result<(), PersistenceError> {
        let now = current_unix_time_millis()?;
        let connection = self.lock_connection()?;
        let changed = connection
            .execute(
                "INSERT INTO workspace_trust (repository_id, granted_at_millis)
                 SELECT id, ?2 FROM repositories WHERE id = ?1
                 ON CONFLICT(repository_id) DO NOTHING",
                params![id, now],
            )
            .map_err(map_sqlite_error)?;
        if changed == 0 {
            let exists: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM repositories WHERE id = ?1)",
                    [id],
                    |row| row.get(0),
                )
                .map_err(map_sqlite_error)?;
            if !exists {
                return Err(PersistenceError::new("Repository 不存在。"));
            }
        }
        Ok(())
    }
}
