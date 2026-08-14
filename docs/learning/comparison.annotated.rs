/*
类图与阅读路线（学习稿，不参与产品编译）

ResolvedRevisions --build_comparison--> ChangeComparison
SnapshotContentReader --read--> FileChangeContent
ChangeComparison --contains--> FileChange --before/after--> FileContentRef
FileContentRef --read_content--> FileContent

推荐阅读顺序：
1. ResolvedRevisions → ChangeComparison → FileChange → FileContentRef
2. build_comparison → parse_raw_diff → file_content_ref
3. SnapshotContentReader::restore → read → read_content
*/

use super::*;
// 取消令牌贯穿每次 Git I/O；错误统一成 GitCommandError；适配器绑定具体仓库路径。
use crate::core_git::{CancellationToken, GitCommandError, GitProcessAdapter};
// OperationContext 表达这次业务操作的取消策略，reader 不自行创建策略。
use crate::operation::OperationContext;
use std::path::Path;

/// 从不可变 Snapshot 恢复并按固定对象 ID 读取内容的运行时 reader。
#[derive(Clone)]
pub struct SnapshotContentReader {
    // 真正执行 rev-parse、merge-base、diff、cat-file 的本地 Core Git 边界。
    adapter: GitProcessAdapter,
    // 已经固定 revisions 和 File Content References 的比较，不会重新解析移动 ref。
    comparison: ChangeComparison,
}

impl SnapshotContentReader {
    /// 从持久化 Snapshot 恢复 reader，不重新解析任何可移动 ref。
    pub fn restore(repository_path: &Path, comparison: ChangeComparison) -> Self {
        Self {
            adapter: GitProcessAdapter::new(repository_path),
            comparison,
        }
    }

    /// 使用显式操作上下文读取 Snapshot 中一个 File Change 的固定内容。
    pub fn read(
        &self,
        path: &str,
        operation: &OperationContext,
    ) -> Result<FileChangeContent, GitCommandError> {
        // path 是 Snapshot 对外暴露的稳定查找键；rename 使用新路径。
        let change = self
            .comparison
            .file_changes
            .iter()
            .find(|change| change.path == path)
            .ok_or_else(|| {
                GitCommandError::new("File Change is not part of the Review Snapshot")
            })?;
        // 新增文件的 before、删除文件的 after 会自然传入 None，并返回 Ok(None)。
        let before = read_content(
            &self.adapter,
            change.before.as_ref(),
            operation.cancellation(),
        )?;
        let after = read_content(
            &self.adapter,
            change.after.as_ref(),
            operation.cancellation(),
        )?;
        Ok(FileChangeContent {
            // 这里保留结构化 FileContent；只有 Tauri 映射边界才展开成文本、大小和类别。
            path: change.path.clone(),
            old_path: change.old_path.clone(),
            status: change.status,
            before,
            after,
        })
    }
}

/// 固定 base、Comparison Base、head 与 File Changes 的不可变 Change Comparison。
#[derive(Clone)]
pub struct ChangeComparison {
    // ref 用于展示和保持 Selection 语义；revision 才是固定内容身份。
    pub base_ref: String,
    pub head_ref: String,
    pub base_revision: String,
    // Comparison Base 是 merge-base；three-dot File Changes 从它比较到 head_revision。
    pub comparison_base: String,
    pub head_revision: String,
    pub file_changes: Vec<FileChange>,
}

/// 不含实际内容、可持久化的 File Change 元数据。
#[derive(Clone)]
pub struct FileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub status: FileChangeStatus,
    // 两侧直接持有唯一的内容引用，避免 mode/object ID 与另一份 entry 重复、失配。
    pub before: Option<FileContentRef>,
    pub after: Option<FileContentRef>,
}

/// File Change 单侧固定到 Git 对象和 tree mode 的内容引用。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileContentRef {
    // object ID 指向 blob；gitlink/submodule 时指向 commit。
    pub(crate) object_id: String,
    // mode 决定普通文件、symlink 或 submodule 的读取方式。
    pub(crate) mode: u32,
}

/// 已获取的精确 revisions 及 Change Comparison 需要保留的展示 refs。
pub(crate) struct ResolvedRevisions {
    pub(crate) base_ref: String,
    pub(crate) head_ref: String,
    pub(crate) base_revision: String,
    pub(crate) head_revision: String,
}

/// 验证已解析 revisions 并构建不可变的 three-dot Change Comparison。
pub(crate) fn build_comparison(
    repository_path: &Path,
    revisions: ResolvedRevisions,
    operation: &OperationContext,
) -> Result<ChangeComparison, GitCommandError> {
    // builder 是同步函数：Core Git adapter 当前同步等待子进程，内部没有异步 await 点。
    let adapter = GitProcessAdapter::new(repository_path);
    let cancellation = operation.cancellation();
    // 先验证只含十六进制，再 peel 为 commit，避免重新解释任意 ref/revision 表达式。
    validate_object_id(&revisions.base_revision)?;
    validate_object_id(&revisions.head_revision)?;
    let base_revision = peel_to_commit_id(&adapter, &revisions.base_revision, cancellation)?;
    let head_revision = peel_to_commit_id(&adapter, &revisions.head_revision, cancellation)?;
    // three-dot 语义的比较起点必须是两个 commits 的共同祖先。
    let comparison_base = parse_single_line_output(
        adapter.run(["merge-base", &base_revision, &head_revision], cancellation)?,
        "merge-base",
    )?;
    validate_object_id(&comparison_base)?;
    let raw = adapter.run(
        [
            "diff",
            // raw 提供 mode/object ID/status；-z 让路径由 NUL 定界；no-abbrev 固定完整 ID。
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
    Ok(ChangeComparison {
        base_ref: revisions.base_ref,
        head_ref: revisions.head_ref,
        base_revision,
        comparison_base,
        head_revision,
        file_changes: parse_raw_diff(&raw.stdout)?,
    })
}

/// 将已校验的精确对象 ID peel 为 commit，不依赖任何持久 ref。
fn peel_to_commit_id(
    adapter: &GitProcessAdapter,
    object_id: &str,
    cancellation: &CancellationToken,
) -> Result<String, GitCommandError> {
    validate_object_id(object_id)?;
    let revision = format!("{object_id}^{{commit}}");
    let commit = parse_single_line_output(
        adapter.run(["rev-parse", "--verify", &revision], cancellation)?,
        "revision",
    )?;
    validate_object_id(&commit)?;
    Ok(commit)
}

/// 从显式单行机器输出提取文本。
fn parse_single_line_output(
    output: crate::core_git::GitOutput,
    label: &str,
) -> Result<String, GitCommandError> {
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
fn parse_raw_diff(bytes: &[u8]) -> Result<Vec<FileChange>, GitCommandError> {
    // 先按 NUL 切 record，不能先把整段输出按 UTF-8 或空格拆分，否则特殊路径会被破坏。
    let mut records = bytes.split(|byte| *byte == 0).peekable();
    let mut changes = Vec::new();
    while let Some(header) = records.next() {
        if header.is_empty() {
            continue;
        }
        let header = std::str::from_utf8(header)
            .map_err(|_| GitCommandError::new("raw diff header 不是 UTF-8"))?;
        // raw header 固定为 old mode、new mode、old ID、new ID、status 五项。
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
        // R 后可能带相似度分数（例如 R100），产品状态只需要首字节。
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
        // rename 的 first_path 是旧路径，还必须再消费一条新路径；其他状态只需一条。
        let (path, old_path, status) = match status_code {
            b'A' => (first_path, None, FileChangeStatus::Added),
            b'D' => (first_path, None, FileChangeStatus::Deleted),
            b'R' => (
                parse_path(
                    records
                        .next()
                        .ok_or_else(|| GitCommandError::new("rename 缺少新路径"))?,
                )?,
                Some(first_path),
                FileChangeStatus::Renamed,
            ),
            _ => (first_path, None, FileChangeStatus::Modified),
        };
        let before = file_content_ref(fields[2], old_mode);
        let after = file_content_ref(fields[3], new_mode);
        changes.push(FileChange {
            path,
            old_path,
            status,
            before,
            after,
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

/// 将 raw diff 中非空一侧转换为固定的 File Content Reference。
fn file_content_ref(object_id: &str, mode: u32) -> Option<FileContentRef> {
    // raw diff 用 mode 0 表示这一侧不存在，而不是一个真实 tree entry。
    (mode != 0).then(|| FileContentRef {
        object_id: object_id.to_owned(),
        mode,
    })
}

/// 按对象 ID 读取 blob；gitlink 直接以 commit ID 表示内容。
fn read_content(
    adapter: &GitProcessAdapter,
    content_ref: Option<&FileContentRef>,
    cancellation: &CancellationToken,
) -> Result<Option<FileContent>, GitCommandError> {
    let Some(content_ref) = content_ref else {
        return Ok(None);
    };
    // mode 160000 是 gitlink，object ID 本身就是需要展示的 submodule commit。
    if content_ref.mode == 0o160000 {
        return Ok(Some(FileContent::Submodule {
            commit_id: content_ref.object_id.clone(),
        }));
    }
    let size_text = parse_single_line_output(
        adapter.run(["cat-file", "-s", &content_ref.object_id], cancellation)?,
        "blob size",
    )?;
    let size = size_text
        .parse::<usize>()
        .map_err(|_| GitCommandError::new("blob size 输出无效"))?;
    let content = adapter
        .run(["cat-file", "blob", &content_ref.object_id], cancellation)?
        .stdout;
    let content = if content_ref.mode == 0o120000 {
        // symlink blob 存的是链接目标；界面模型要求 UTF-8，非法目标会返回明确错误。
        FileContent::Symlink {
            size,
            target: String::from_utf8(content)
                .map_err(|_| GitCommandError::new("symlink target 不是有效的 UTF-8"))?,
        }
    } else if content.contains(&0) || std::str::from_utf8(&content).is_err() {
        // 含 NUL 或无法解码为 UTF-8时，保留大小但不制造文本。
        FileContent::Binary { size }
    } else {
        FileContent::Text {
            size,
            text: String::from_utf8(content)
                .map_err(|_| GitCommandError::new("文本 blob 不是有效的 UTF-8"))?,
        }
    };
    Ok(Some(content))
}

#[cfg(test)]
#[path = "comparison_tests.rs"]
mod tests;
    // 普通 blob 先取字节大小，再取原始内容；二进制判断发生在 UTF-8 转换之前。
