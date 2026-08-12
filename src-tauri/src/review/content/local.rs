use super::*;
use crate::core_git::{CancellationToken, GitCommandError, GitProcessAdapter};
use crate::operation::OperationContext;
use std::path::Path;

/// 从不可变 Snapshot 恢复并按固定对象 ID 读取内容的运行时 reader。
#[derive(Clone)]
pub struct LocalSnapshotReader {
    adapter: GitProcessAdapter,
    snapshot: LocalSnapshot,
}

impl LocalSnapshotReader {
    /// 从持久化 Snapshot 恢复 reader，不重新解析任何可移动 ref。
    pub fn restore(repository_path: &Path, snapshot: LocalSnapshot) -> Self {
        Self {
            adapter: GitProcessAdapter::new(repository_path),
            snapshot,
        }
    }

    /// 使用显式操作上下文读取 Snapshot 中一个 File Change 的固定内容。
    pub async fn read(
        &self,
        path: &str,
        operation: &OperationContext,
    ) -> Result<LoadedFileChange, GitCommandError> {
        read_file_change(
            &self.adapter,
            &self.snapshot,
            path,
            operation.cancellation(),
        )
    }
}

/// 固定 base、Comparison Base、head 与 File Changes 的本地 Snapshot。
#[derive(Clone)]
pub struct LocalSnapshot {
    pub base_ref: String,
    pub head_ref: String,
    pub base_revision: String,
    pub comparison_base: String,
    pub head_revision: String,
    pub file_changes: Vec<LocalFileChange>,
}

/// 不含实际内容、可持久化的 File Change 元数据。
#[derive(Clone)]
pub struct LocalFileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub status: ChangeStatus,
    pub before_mode: Option<u32>,
    pub after_mode: Option<u32>,
    pub before_object_id: Option<String>,
    pub after_object_id: Option<String>,
    pub(crate) before_entry: Option<LocalEntry>,
    pub(crate) after_entry: Option<LocalEntry>,
}

/// 固定到 Git 对象和 tree mode 的单侧文件条目。
#[derive(Clone)]
pub(crate) struct LocalEntry {
    pub(crate) object_id: String,
    pub(crate) mode: u32,
}

/// 已获取的精确 revisions 及 Snapshot 需要保留的展示 refs。
pub(crate) struct ExactComparison {
    pub(crate) base_ref: String,
    pub(crate) head_ref: String,
    pub(crate) base_revision: String,
    pub(crate) head_revision: String,
}

/// 验证精确 revisions 并构建不可变的 three-dot Snapshot。
pub(crate) async fn compare_exact(
    repository_path: &Path,
    comparison: ExactComparison,
    operation: &OperationContext,
) -> Result<LocalSnapshot, GitCommandError> {
    compare_revisions(
        &GitProcessAdapter::new(repository_path),
        &comparison.base_ref,
        &comparison.head_ref,
        &comparison.base_revision,
        &comparison.head_revision,
        operation.cancellation(),
    )
}

/// 校验精确 commit，计算 merge-base，并从 Comparison Base 到 head 生成 File Changes。
fn compare_revisions(
    adapter: &GitProcessAdapter,
    base_ref: &str,
    head_ref: &str,
    base_revision: &str,
    head_revision: &str,
    cancellation: &CancellationToken,
) -> Result<LocalSnapshot, GitCommandError> {
    validate_object_id(base_revision)?;
    validate_object_id(head_revision)?;
    let base_revision = resolve_object_commit(adapter, base_revision, cancellation)?;
    let head_revision = resolve_object_commit(adapter, head_revision, cancellation)?;
    let comparison_base = text_output(
        adapter.run(["merge-base", &base_revision, &head_revision], cancellation)?,
        "merge-base",
    )?;
    validate_object_id(&comparison_base)?;
    let raw = adapter.run(
        [
            "diff",
            "--raw",
            "-z",
            "--no-abbrev",
            "--find-renames",
            &comparison_base,
            &head_revision,
            "--",
        ],
        cancellation,
    )?;
    Ok(LocalSnapshot {
        base_ref: base_ref.to_owned(),
        head_ref: head_ref.to_owned(),
        base_revision,
        comparison_base,
        head_revision,
        file_changes: parse_raw_diff(&raw.stdout)?,
    })
}

/// 将已校验的精确对象 ID peel 为 commit，不依赖任何持久 ref。
fn resolve_object_commit(
    adapter: &GitProcessAdapter,
    object_id: &str,
    cancellation: &CancellationToken,
) -> Result<String, GitCommandError> {
    validate_object_id(object_id)?;
    let revision = format!("{object_id}^{{commit}}");
    let commit = text_output(
        adapter.run(["rev-parse", "--verify", &revision], cancellation)?,
        "revision",
    )?;
    validate_object_id(&commit)?;
    Ok(commit)
}

/// 从显式单行机器输出提取文本。
fn text_output(output: crate::core_git::GitOutput, label: &str) -> Result<String, GitCommandError> {
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| GitCommandError::new(format!("{label} 输出不是 UTF-8")))?
        .trim();
    if text.is_empty() || text.lines().count() != 1 {
        return Err(GitCommandError::new(format!("{label} 输出格式无效")));
    }
    Ok(text.to_owned())
}

/// 验证 object ID 只包含 Git 十六进制机器格式。
fn validate_object_id(object_id: &str) -> Result<(), GitCommandError> {
    if (40..=64).contains(&object_id.len())
        && object_id.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        Ok(())
    } else {
        Err(GitCommandError::new("Core Git 返回了无效 object ID"))
    }
}

/// 解析 `git diff --raw -z`，路径始终由 NUL 边界确定。
fn parse_raw_diff(bytes: &[u8]) -> Result<Vec<LocalFileChange>, GitCommandError> {
    let mut records = bytes.split(|byte| *byte == 0).peekable();
    let mut changes = Vec::new();
    while let Some(header) = records.next() {
        if header.is_empty() {
            continue;
        }
        let header = std::str::from_utf8(header)
            .map_err(|_| GitCommandError::new("raw diff header 不是 UTF-8"))?;
        let fields = header
            .strip_prefix(':')
            .ok_or_else(|| GitCommandError::new("raw diff header 缺少前缀"))?
            .split_ascii_whitespace()
            .collect::<Vec<_>>();
        if fields.len() != 5 {
            return Err(GitCommandError::new("raw diff header 字段数量无效"));
        }
        let old_mode = parse_mode(fields[0])?;
        let new_mode = parse_mode(fields[1])?;
        validate_object_id(fields[2])?;
        validate_object_id(fields[3])?;
        let status_code = fields[4]
            .as_bytes()
            .first()
            .copied()
            .ok_or_else(|| GitCommandError::new("raw diff status 为空"))?;
        let first_path = parse_path(
            records
                .next()
                .ok_or_else(|| GitCommandError::new("raw diff 缺少路径"))?,
        )?;
        let (path, old_path, status) = match status_code {
            b'A' => (first_path, None, ChangeStatus::Added),
            b'D' => (first_path, None, ChangeStatus::Deleted),
            b'R' => (
                parse_path(
                    records
                        .next()
                        .ok_or_else(|| GitCommandError::new("rename 缺少新路径"))?,
                )?,
                Some(first_path),
                ChangeStatus::Renamed,
            ),
            _ => (first_path, None, ChangeStatus::Modified),
        };
        let before_entry = entry(fields[2], old_mode);
        let after_entry = entry(fields[3], new_mode);
        changes.push(LocalFileChange {
            path,
            old_path,
            status,
            before_mode: before_entry.as_ref().map(|entry| entry.mode),
            after_mode: after_entry.as_ref().map(|entry| entry.mode),
            before_object_id: before_entry.as_ref().map(|entry| entry.object_id.clone()),
            after_object_id: after_entry.as_ref().map(|entry| entry.object_id.clone()),
            before_entry,
            after_entry,
        });
    }
    Ok(changes)
}

/// 将 raw diff 的八进制 mode 转成数值。
fn parse_mode(mode: &str) -> Result<u32, GitCommandError> {
    u32::from_str_radix(mode, 8).map_err(|_| GitCommandError::new("raw diff mode 无效"))
}

/// 将 NUL 分隔路径转换为界面模型使用的 UTF-8 字符串。
fn parse_path(path: &[u8]) -> Result<String, GitCommandError> {
    String::from_utf8(path.to_vec())
        .map_err(|_| GitCommandError::new("File Change 路径不是有效的 UTF-8"))
}

/// 将非空 tree entry 元数据转换为可按需读取的条目。
fn entry(object_id: &str, mode: u32) -> Option<LocalEntry> {
    (mode != 0).then(|| LocalEntry {
        object_id: object_id.to_owned(),
        mode,
    })
}

/// 从固定 Snapshot 的对象记录读取并组合一个 File Change。
fn read_file_change(
    adapter: &GitProcessAdapter,
    snapshot: &LocalSnapshot,
    path: &str,
    cancellation: &CancellationToken,
) -> Result<LoadedFileChange, GitCommandError> {
    let stored = snapshot
        .file_changes
        .iter()
        .find(|change| change.path == path)
        .ok_or_else(|| GitCommandError::new("File Change is not part of the Review Snapshot"))?;
    let before = read_entry(adapter, stored.before_entry.as_ref(), cancellation)?;
    let after = read_entry(adapter, stored.after_entry.as_ref(), cancellation)?;
    Ok(LoadedFileChange {
        path: stored.path.clone(),
        old_path: stored.old_path.clone(),
        status: stored.status,
        content_kind: super::resolved_content_kind([
            before.as_ref().map(|content| content.kind),
            after.as_ref().map(|content| content.kind),
        ]),
        before_size: before.as_ref().and_then(|entry| entry.size),
        after_size: after.as_ref().and_then(|entry| entry.size),
        before: before.and_then(|entry| entry.text),
        after: after.and_then(|entry| entry.text),
    })
}

/// 按对象 ID 读取 blob；gitlink 直接以 commit ID 表示内容。
fn read_entry(
    adapter: &GitProcessAdapter,
    entry: Option<&LocalEntry>,
    cancellation: &CancellationToken,
) -> Result<Option<EntryContent>, GitCommandError> {
    let Some(entry) = entry else { return Ok(None) };
    if entry.mode == 0o160000 {
        return Ok(Some(EntryContent {
            kind: ContentKind::Submodule,
            size: None,
            text: Some(entry.object_id.clone()),
        }));
    }
    let size_text = text_output(
        adapter.run(["cat-file", "-s", &entry.object_id], cancellation)?,
        "blob size",
    )?;
    let size = size_text
        .parse::<usize>()
        .map_err(|_| GitCommandError::new("blob size 输出无效"))?;
    let content = adapter
        .run(["cat-file", "blob", &entry.object_id], cancellation)?
        .stdout;
    let kind = if entry.mode == 0o120000 {
        ContentKind::Symlink
    } else if content.contains(&0) || std::str::from_utf8(&content).is_err() {
        ContentKind::Binary
    } else {
        ContentKind::Text
    };
    Ok(Some(EntryContent {
        kind,
        size: Some(size),
        text: (kind != ContentKind::Binary)
            .then(|| String::from_utf8(content).ok())
            .flatten(),
    }))
}

/// 单侧文件内容及其展示类型。
struct EntryContent {
    kind: ContentKind,
    size: Option<usize>,
    text: Option<String>,
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;
