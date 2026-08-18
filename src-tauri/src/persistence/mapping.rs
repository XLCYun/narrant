use super::*;

/// 将一个 Snapshot 的稳定 File Change metadata 写入当前事务。
pub(super) fn insert_file_changes(
    transaction: &rusqlite::Transaction<'_>,
    snapshot_id: i64,
    file_changes: &[FileChange],
) -> Result<(), PersistenceError> {
    for change in file_changes {
        transaction
            .execute(
                "INSERT INTO file_changes (
                    snapshot_id, path, old_path, status, before_mode, after_mode,
                    before_object_id, after_object_id
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    snapshot_id,
                    change.path,
                    change.old_path,
                    status_text(change.status),
                    change.before.as_ref().map(|content| content.mode),
                    change.after.as_ref().map(|content| content.mode),
                    change.before.as_ref().map(|content| &content.object_id),
                    change.after.as_ref().map(|content| &content.object_id)
                ],
            )
            .map_err(map_sqlite_error)?;
    }
    Ok(())
}

/// 将 durable string identity 解析为数据库主键。
pub(super) fn parse_review_id(review_id: &str) -> Result<i64, PersistenceError> {
    review_id
        .strip_prefix("review-")
        .and_then(|value| value.parse().ok())
        .ok_or_else(|| PersistenceError::new("reviewId 格式无效。"))
}

/// 将 domain status 保存成稳定的数据库标签。
pub(super) fn status_text(status: FileChangeStatus) -> &'static str {
    match status {
        FileChangeStatus::Added => "added",
        FileChangeStatus::Modified => "modified",
        FileChangeStatus::Deleted => "deleted",
        FileChangeStatus::Renamed => "renamed",
    }
}

/// 从数据库状态标签恢复 domain status。
pub(super) fn parse_status(status: &str) -> rusqlite::Result<FileChangeStatus> {
    match status {
        "added" => Ok(FileChangeStatus::Added),
        "modified" => Ok(FileChangeStatus::Modified),
        "deleted" => Ok(FileChangeStatus::Deleted),
        "renamed" => Ok(FileChangeStatus::Renamed),
        _ => Err(rusqlite::Error::InvalidQuery),
    }
}

/// 用持久化 object ID 与 mode 恢复 File Content Reference。
pub(super) fn content_ref_from_metadata(
    object_id: &Option<String>,
    mode: Option<u32>,
) -> Option<FileContentRef> {
    Some(FileContentRef {
        object_id: object_id.clone()?,
        mode: mode?,
    })
}

/// 从稳定的列顺序重建尚未探测可用性的入口。
pub(super) fn repository_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Repository> {
    Ok(Repository {
        id: row.get(0)?,
        canonical_path: PathBuf::from(row.get::<_, String>(1)?),
        git_common_directory: PathBuf::from(row.get::<_, String>(2)?),
        created_at_millis: row.get(3)?,
        last_opened_at_millis: row.get(4)?,
    })
}

/// 返回可排序且无需额外时间依赖的 Unix 毫秒时间。
pub(super) fn current_unix_time_millis() -> Result<i64, PersistenceError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| PersistenceError::new(format!("系统时间无效：{error}")))?;
    i64::try_from(duration.as_millis()).map_err(|_| PersistenceError::new("系统时间超出支持范围。"))
}

/// 将 SQLite 故障转换为明确的恢复指引。
pub(super) fn map_sqlite_error(error: rusqlite::Error) -> PersistenceError {
    PersistenceError::new(format!(
        "Persistence 数据库无法打开或迁移：{error}。请完整退出 Narrant，备份后移走或删除数据库以重建数据库。"
    ))
}
