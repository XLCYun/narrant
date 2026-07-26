use crate::core_git::{CancellationToken, GitProcessAdapter};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const CURRENT_SCHEMA_VERSION: i64 = 1;
const ENTRY_COLUMNS: &str = "id, canonical_path, common_directory, created_at, last_opened_at";

/// 已注册的具体本地 clone，以及当前是否仍可访问。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryEntry {
    pub id: i64,
    pub canonical_path: PathBuf,
    pub common_directory: PathBuf,
    pub created_at: i64,
    pub last_opened_at: i64,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

/// 经 Core Git 验证后的 working-tree repository 身份。
#[derive(Clone, Debug, PartialEq, Eq)]
struct InspectedRepository {
    canonical_path: PathBuf,
    common_directory: PathBuf,
}

/// Repository Catalog 的持久化或仓库验证错误。
#[derive(Debug)]
pub struct RepositoryCatalogError {
    message: String,
}

impl RepositoryCatalogError {
    /// 创建可直接显示给用户的 Catalog 错误。
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for RepositoryCatalogError {
    /// 输出稳定且可恢复的中文诊断。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RepositoryCatalogError {}

/// 应用级 SQLite Repository Catalog；所有写入通过事务原子完成。
pub struct RepositoryCatalog {
    connection: Mutex<Connection>,
}

impl std::fmt::Debug for RepositoryCatalog {
    /// 避免调试输出泄露数据库内部状态。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("RepositoryCatalog")
            .finish_non_exhaustive()
    }
}

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

    /// 按最近打开顺序列出全部入口，缺失路径仍保留在结果中。
    pub fn list(&self) -> Result<Vec<RepositoryEntry>, RepositoryCatalogError> {
        let connection = self.connection()?;
        let list_query = format!(
            "SELECT {ENTRY_COLUMNS} FROM repository_entries
             ORDER BY last_opened_at DESC, id DESC"
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
        let mut connection = self.connection()?;
        let entry_query = format!("SELECT {ENTRY_COLUMNS} FROM repository_entries WHERE id = ?1");
        let saved = connection
            .query_row(&entry_query, [id], saved_entry_from_row)
            .optional()
            .map_err(database_recovery_error)?
            .ok_or_else(|| RepositoryCatalogError::new("Repository Entry 不存在。"))?;
        let mut entry = resolve_availability(saved);
        if entry.available {
            let now = unix_timestamp_millis()?;
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

    /// 统一处理 Catalog mutex 状态，避免调用点重复泄露同步细节。
    fn connection(&self) -> Result<std::sync::MutexGuard<'_, Connection>, RepositoryCatalogError> {
        self.connection
            .lock()
            .map_err(|_| RepositoryCatalogError::new("Repository Catalog 状态不可用。"))
    }
}

/// 执行第一版 schema migration，不在 SQLite 中创建任何 secret 字段。
fn migrate(connection: &mut Connection) -> rusqlite::Result<()> {
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
    Ok(())
}

/// 从稳定的列顺序重建尚未探测可用性的入口。
fn saved_entry_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RepositoryEntry> {
    Ok(RepositoryEntry {
        id: row.get(0)?,
        canonical_path: PathBuf::from(row.get::<_, String>(1)?),
        common_directory: PathBuf::from(row.get::<_, String>(2)?),
        created_at: row.get(3)?,
        last_opened_at: row.get(4)?,
        available: true,
        unavailable_reason: None,
    })
}

/// 用 Core Git 检查保存的路径，且不改变任何 repository 状态。
fn resolve_availability(mut entry: RepositoryEntry) -> RepositoryEntry {
    match inspect_repository(&entry.canonical_path) {
        Ok(inspected)
            if inspected.canonical_path == entry.canonical_path
                && inspected.common_directory == entry.common_directory => {}
        Ok(_) => {
            entry.available = false;
            entry.unavailable_reason = Some("保存的路径不再指向原 Repository Entry。".to_owned());
        }
        Err(error) => {
            entry.available = false;
            entry.unavailable_reason = Some(error.to_string());
        }
    }
    entry
}

/// 通过机器可读 Core Git 输出验证 working tree、canonical path、common directory 和能力。
fn inspect_repository(selected_path: &Path) -> Result<InspectedRepository, RepositoryCatalogError> {
    let selected_path = selected_path.canonicalize().map_err(|error| {
        RepositoryCatalogError::new(format!("Repository Entry 路径不存在或不可访问：{error}"))
    })?;
    let adapter = GitProcessAdapter::new(&selected_path);
    let cancellation = CancellationToken::default();
    let output = adapter
        .run(
            [
                "rev-parse",
                "--is-inside-work-tree",
                "--show-toplevel",
                "--git-common-dir",
                "--show-object-format",
            ],
            &cancellation,
        )
        .map_err(|error| {
            RepositoryCatalogError::new(format!(
                "所选路径不是兼容的 working-tree Git 仓库；请确认已安装支持所需能力的 Core Git：{error}"
            ))
        })?;
    let lines = String::from_utf8(output.stdout)
        .map_err(|_| RepositoryCatalogError::new("Core Git 返回了无效的仓库路径编码。"))?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if lines.len() != 4 || lines[0] != "true" || lines[3].is_empty() {
        return Err(RepositoryCatalogError::new(
            "所选路径不是兼容的 working-tree Git 仓库。",
        ));
    }
    let canonical_path = PathBuf::from(&lines[1]).canonicalize().map_err(|error| {
        RepositoryCatalogError::new(format!("无法解析 canonical repository path：{error}"))
    })?;
    let common_directory = {
        let path = PathBuf::from(&lines[2]);
        let path = if path.is_absolute() {
            path
        } else {
            canonical_path.join(path)
        };
        path.canonicalize().map_err(|error| {
            RepositoryCatalogError::new(format!("无法解析 Git common directory：{error}"))
        })?
    };
    probe_required_capabilities(&adapter, &cancellation)?;
    Ok(InspectedRepository {
        canonical_path,
        common_directory,
    })
}

/// 只读探测 Repository Entry 页面立即依赖的 refs、remotes 与 worktree 能力。
fn probe_required_capabilities(
    adapter: &GitProcessAdapter,
    cancellation: &CancellationToken,
) -> Result<(), RepositoryCatalogError> {
    for arguments in [
        vec![
            "for-each-ref",
            "--format=%(refname)%00",
            "refs/heads",
            "refs/remotes",
        ],
        vec!["remote"],
        vec!["worktree", "list", "--porcelain", "-z"],
    ] {
        adapter.run(arguments, cancellation).map_err(|error| {
            RepositoryCatalogError::new(format!(
                "Core Git 缺少 Repository Entry 所需的 refs、remotes 或 worktree 能力：{error}"
            ))
        })?;
    }
    Ok(())
}

/// 返回可排序且无需额外时间依赖的 Unix 毫秒时间。
fn unix_timestamp_millis() -> Result<i64, RepositoryCatalogError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| RepositoryCatalogError::new(format!("系统时间无效：{error}")))?;
    i64::try_from(duration.as_millis())
        .map_err(|_| RepositoryCatalogError::new("系统时间超出支持范围。"))
}

/// 将 SQLite 故障转换为明确的恢复指引。
fn database_recovery_error(error: rusqlite::Error) -> RepositoryCatalogError {
    RepositoryCatalogError::new(format!(
        "Repository Catalog 无法打开或迁移：{error}。请完整退出 Narrant，备份后移走或删除数据库以重建 Catalog。"
    ))
}

#[cfg(test)]
#[path = "repository_catalog_tests.rs"]
mod tests;
