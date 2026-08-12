use super::*;

impl RepositoryCatalog {
    /// 打开应用数据库并执行显式 schema migration。
    pub fn open(database_path: &Path) -> Result<Self, RepositoryCatalogError> {
        if let Some(parent) = database_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                RepositoryCatalogError::new(format!("无法创建 Repository Catalog 目录：{error}"))
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
            .map_err(database_recovery_error)?;
        migrate(&mut connection).map_err(database_recovery_error)?;
        Ok(Self {
            connection: Mutex::new(connection),
            workspace_root: database_path
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("reviews"),
        })
    }

    /// 验证并注册一个具体 working-tree clone；重复添加同一路径返回原入口。
    pub fn register(
        &self,
        selected_path: &Path,
    ) -> Result<RepositoryEntry, RepositoryCatalogError> {
        let inspected = inspect_repository(selected_path)?;
        let now = unix_timestamp_millis()?;
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(database_recovery_error)?;
        transaction
            .execute(
                "INSERT INTO repository_entries (
                    canonical_path, common_directory, created_at, last_opened_at
                 ) VALUES (?1, ?2, ?3, ?3)
                 ON CONFLICT(canonical_path) DO UPDATE SET
                    common_directory = excluded.common_directory,
                    last_opened_at = excluded.last_opened_at",
                params![
                    inspected.canonical_path.to_string_lossy(),
                    inspected.common_directory.to_string_lossy(),
                    now
                ],
            )
            .map_err(database_recovery_error)?;
        let entry_query =
            format!("SELECT {ENTRY_COLUMNS} FROM repository_entries WHERE canonical_path = ?1");
        let entry = transaction
            .query_row(
                &entry_query,
                [inspected.canonical_path.to_string_lossy()],
                saved_entry_from_row,
            )
            .map_err(database_recovery_error)?;
        transaction.commit().map_err(database_recovery_error)?;
        Ok(entry)
    }

    /// 按添加顺序倒序列出全部入口，打开活动不改变仓库位置。
    pub fn list(&self) -> Result<Vec<RepositoryEntry>, RepositoryCatalogError> {
        let connection = self.connection()?;
        let list_query = format!(
            "SELECT {ENTRY_COLUMNS} FROM repository_entries
             ORDER BY id DESC"
        );
        let mut statement = connection
            .prepare(&list_query)
            .map_err(database_recovery_error)?;
        let saved = statement
            .query_map([], saved_entry_from_row)
            .map_err(database_recovery_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_recovery_error)?;
        Ok(saved.into_iter().map(resolve_availability).collect())
    }

    /// 打开一个入口；可用时原子更新最后打开时间，不可用时返回历史入口。
    pub fn open_entry(&self, id: i64) -> Result<RepositoryEntry, RepositoryCatalogError> {
        let mut entry = self.inspect_entry(id)?;
        if entry.available {
            let now = unix_timestamp_millis()?;
            let mut connection = self.connection()?;
            let transaction = connection.transaction().map_err(database_recovery_error)?;
            transaction
                .execute(
                    "UPDATE repository_entries SET last_opened_at = ?1 WHERE id = ?2",
                    params![now, id],
                )
                .map_err(database_recovery_error)?;
            transaction.commit().map_err(database_recovery_error)?;
            entry.last_opened_at = now;
        }
        Ok(entry)
    }

    /// 读取 Repository Entry 的当前可用性，不改变最近打开时间。
    pub fn inspect_entry(&self, id: i64) -> Result<RepositoryEntry, RepositoryCatalogError> {
        let connection = self.connection()?;
        let entry_query = format!("SELECT {ENTRY_COLUMNS} FROM repository_entries WHERE id = ?1");
        connection
            .query_row(&entry_query, [id], saved_entry_from_row)
            .optional()
            .map_err(database_recovery_error)?
            .map(resolve_availability)
            .ok_or_else(|| RepositoryCatalogError::new("Repository Entry 不存在。"))
    }

    /// 返回 Repository Entry 是否已经获得持久化 Workspace Trust。
    pub fn is_workspace_trusted(&self, id: i64) -> Result<bool, RepositoryCatalogError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT EXISTS(
                    SELECT 1 FROM workspace_trust WHERE repository_entry_id = ?1
                 )",
                [id],
                |row| row.get(0),
            )
            .map_err(database_recovery_error)
    }

    /// 持久化用户对一个 Repository Entry 授予的 Workspace Trust。
    pub fn grant_workspace_trust(&self, id: i64) -> Result<(), RepositoryCatalogError> {
        let now = unix_timestamp_millis()?;
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "INSERT INTO workspace_trust (repository_entry_id, granted_at)
                 SELECT id, ?2 FROM repository_entries WHERE id = ?1
                 ON CONFLICT(repository_entry_id) DO NOTHING",
                params![id, now],
            )
            .map_err(database_recovery_error)?;
        if changed == 0 {
            let exists: bool = connection
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM repository_entries WHERE id = ?1)",
                    [id],
                    |row| row.get(0),
                )
                .map_err(database_recovery_error)?;
            if !exists {
                return Err(RepositoryCatalogError::new("Repository Entry 不存在。"));
            }
        }
        Ok(())
    }
}
