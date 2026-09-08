use super::comparison::validate_object_id;
use super::{FileContent, FileContentRef, SnapshotContentReader};
use crate::core_git::{CoreGitFailureKind, GitCommandError, GitProcessAdapter};
use crate::operation::OperationContext;

/// 将通常的整仓库源码合并为少量进程，同时给协议头和安全输出上限留出余量。
const BATCH_BYTES: usize = 8 * 1024 * 1024;
const BATCH_OBJECTS: usize = 512;

/// Snapshot 的实际比较侧：旧侧始终是 Comparison Base，新侧是 Head。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotSide {
    Before,
    After,
}

/// 固定 tree 中的真实路径及对象元数据，不包含 checkout 中的状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SnapshotSourceEntry {
    pub path: String,
    pub object_id: String,
    pub mode: u32,
    /// blob 原始字节数；gitlink 或对象不可用时为 None。
    pub size: Option<usize>,
}

/// 内容读取失败不表示路径不存在；保留对象不可用、模式不支持和进程失败的区别。
#[derive(Clone, Debug)]
pub enum SnapshotSourceError {
    ObjectUnavailable { object_id: String },
    UnsupportedMode { mode: u32 },
    Git(GitCommandError),
}

impl std::fmt::Display for SnapshotSourceError {
    /// 给源文件入口提供诊断文本，调用方按枚举而非文本判断覆盖状态。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ObjectUnavailable { object_id } => {
                write!(formatter, "Snapshot 对象不可用：{object_id}")
            }
            Self::UnsupportedMode { mode } => {
                write!(formatter, "Snapshot 文件模式不支持：{mode:o}")
            }
            Self::Git(error) => std::fmt::Display::fmt(error, formatter),
        }
    }
}

impl std::error::Error for SnapshotSourceError {}

/// 一条请求路径的独立结果；None 只表示完整 tree 确认该路径不存在。
#[derive(Debug)]
pub struct SnapshotSourceRead {
    pub path: String,
    pub content: Result<Option<FileContent>, SnapshotSourceError>,
}

/// 一侧完整且不可变的来源目录，只有成功枚举后才能确认路径缺失。
#[derive(Clone)]
pub struct SnapshotSourceTree {
    adapter: GitProcessAdapter,
    revision: String,
    entries: Vec<SnapshotSourceEntry>,
}

impl SnapshotContentReader {
    /// 按 Snapshot 的既有 File Content Reference 读取内容，无需其 commit/tree 仍可枚举。
    pub(crate) fn read_reference(
        &self,
        operation: &OperationContext,
        reference: &FileContentRef,
    ) -> Result<FileContent, SnapshotSourceError> {
        check_cancelled(operation).map_err(SnapshotSourceError::Git)?;
        validate_object_id(&reference.object_id).map_err(SnapshotSourceError::Git)?;
        match reference.mode {
            0o160000 => {
                return Ok(FileContent::Submodule {
                    commit_id: reference.object_id.clone(),
                })
            }
            0o100644 | 0o100755 | 0o120000 => {}
            mode => return Err(SnapshotSourceError::UnsupportedMode { mode }),
        }
        let entry = SnapshotSourceEntry {
            path: String::new(),
            object_id: reference.object_id.clone(),
            mode: reference.mode,
            size: None,
        };
        read_chunk(&self.adapter, operation, &[&entry])
            .map_err(SnapshotSourceError::Git)?
            .pop()
            .expect("one object request produces one result")
            .content?
            .ok_or_else(|| {
                SnapshotSourceError::Git(GitCommandError::new("固定内容引用不能表示缺侧"))
            })
    }

    /// 从 Snapshot 精确 commit 枚举整侧来源，包含未修改文件和项目配置。
    pub fn source_tree(
        &self,
        operation: &OperationContext,
        side: SnapshotSide,
    ) -> Result<SnapshotSourceTree, GitCommandError> {
        check_cancelled(operation)?;
        let revision = match side {
            SnapshotSide::Before => &self.comparison.comparison_base,
            SnapshotSide::After => &self.comparison.head_revision,
        };
        validate_object_id(revision)?;
        let output = self.adapter.run(
            [
                "--no-replace-objects",
                "ls-tree",
                "-r",
                "-l",
                "-z",
                "--full-tree",
                revision,
            ],
            operation.cancellation(),
        )?;
        let entries = parse_entries(&output.stdout)?;
        check_cancelled(operation)?;
        Ok(SnapshotSourceTree {
            adapter: self.adapter.clone(),
            revision: revision.clone(),
            entries,
        })
    }
}

impl SnapshotSourceTree {
    /// 返回此目录所属的精确 Snapshot revision。
    pub fn revision(&self) -> &str {
        &self.revision
    }

    /// 返回完整 tree 的真实文件目录；不根据语言或 File Changes 裁剪。
    pub fn entries(&self) -> &[SnapshotSourceEntry] {
        &self.entries
    }

    /// 读取真实路径的完整内容；只有 tree 确认缺失才返回 None。
    pub fn read(
        &self,
        operation: &OperationContext,
        path: &str,
    ) -> Result<Option<FileContent>, SnapshotSourceError> {
        self.read_batch(operation, &[path])
            .map_err(SnapshotSourceError::Git)?
            .pop()
            .expect("one request produces one result")
            .content
    }

    /// 按请求顺序批量读取固定 blob；单文件故障不隐藏同批可读文件，取消则终止操作。
    pub fn read_batch(
        &self,
        operation: &OperationContext,
        paths: &[&str],
    ) -> Result<Vec<SnapshotSourceRead>, GitCommandError> {
        check_cancelled(operation)?;
        let mut results = Vec::with_capacity(paths.len());
        let mut pending = Vec::new();
        let mut bytes = 0_usize;
        for path in paths {
            check_cancelled(operation)?;
            let entry = self.entries.iter().find(|entry| entry.path == *path);
            if let Some(entry) =
                entry.filter(|entry| matches!(entry.mode, 0o100644 | 0o100755 | 0o120000))
            {
                let size = entry.size.unwrap_or(0).saturating_add(128);
                if !pending.is_empty()
                    && (bytes.saturating_add(size) > BATCH_BYTES || pending.len() == BATCH_OBJECTS)
                {
                    results.extend(read_chunk(&self.adapter, operation, &pending)?);
                    pending.clear();
                    bytes = 0;
                }
                pending.push(entry);
                bytes = bytes.saturating_add(size);
                continue;
            }
            results.extend(read_chunk(&self.adapter, operation, &pending)?);
            pending.clear();
            bytes = 0;
            let content = match entry {
                None => Ok(None),
                Some(entry) if entry.mode == 0o160000 => Ok(Some(FileContent::Submodule {
                    commit_id: entry.object_id.clone(),
                })),
                Some(entry) => Err(SnapshotSourceError::UnsupportedMode { mode: entry.mode }),
            };
            results.push(SnapshotSourceRead {
                path: (*path).to_owned(),
                content,
            });
        }
        results.extend(read_chunk(&self.adapter, operation, &pending)?);
        check_cancelled(operation)?;
        Ok(results)
    }
}

/// 使用固定十六进制 ID 请求一批对象；协议损坏或超限时隔离故障文件。
fn read_chunk(
    adapter: &GitProcessAdapter,
    operation: &OperationContext,
    entries: &[&SnapshotSourceEntry],
) -> Result<Vec<SnapshotSourceRead>, GitCommandError> {
    check_cancelled(operation)?;
    if entries.is_empty() {
        return Ok(Vec::new());
    }
    let input = entries
        .iter()
        .flat_map(|entry| entry.object_id.bytes().chain([b'\n']))
        .collect();
    let output = adapter
        .run_with_input(
            ["--no-replace-objects", "cat-file", "--batch"],
            input,
            operation.cancellation(),
        )
        .and_then(|output| parse_batch(&output.stdout, entries));
    check_cancelled(operation)?;
    match output {
        Ok(results) => Ok(results),
        Err(error) if error.is_cancelled() => Err(error),
        Err(error)
            if entries.len() > 1
                && matches!(
                    error.kind(),
                    CoreGitFailureKind::Exited { .. }
                        | CoreGitFailureKind::InvalidData
                        | CoreGitFailureKind::InputWriteFailed
                        | CoreGitFailureKind::OutputLimitExceeded { .. }
                ) =>
        {
            let mut results = Vec::with_capacity(entries.len());
            for entry in entries {
                results.extend(read_chunk(adapter, operation, &[*entry])?);
            }
            Ok(results)
        }
        Err(error) => Ok(entries
            .iter()
            .map(|entry| SnapshotSourceRead {
                path: entry.path.clone(),
                content: Err(SnapshotSourceError::Git(error.clone())),
            })
            .collect()),
    }
}

/// 即使本次请求没有 Git I/O，也不能把取消误报为路径不存在或正常完成。
fn check_cancelled(operation: &OperationContext) -> Result<(), GitCommandError> {
    if operation.cancellation().is_cancelled() {
        Err(GitCommandError::cancelled())
    } else {
        Ok(())
    }
}

/// 解析 NUL 分隔的 tree 路径；路径内的空格、TAB 和换行均保持原样。
fn parse_entries(bytes: &[u8]) -> Result<Vec<SnapshotSourceEntry>, GitCommandError> {
    if !bytes.is_empty() && bytes.last() != Some(&0) {
        return Err(GitCommandError::new("Snapshot tree 输出缺少 NUL 终止符"));
    }
    bytes
        .split(|byte| *byte == 0)
        .filter(|record| !record.is_empty())
        .map(|record| {
            let tab = record
                .iter()
                .position(|byte| *byte == b'\t')
                .ok_or_else(|| GitCommandError::new("Snapshot tree 记录缺少路径边界"))?;
            let header = std::str::from_utf8(&record[..tab])
                .map_err(|_| GitCommandError::new("Snapshot tree header 不是 UTF-8"))?;
            let fields = header.split_ascii_whitespace().collect::<Vec<_>>();
            if fields.len() != 4 {
                return Err(GitCommandError::new("Snapshot tree header 字段数量无效"));
            }
            validate_object_id(fields[2])?;
            let mode = u32::from_str_radix(fields[0], 8)
                .map_err(|_| GitCommandError::new("Snapshot tree mode 无效"))?;
            let size = match fields[3] {
                "-" | "BAD" => None,
                size => Some(
                    size.parse::<usize>()
                        .map_err(|_| GitCommandError::new("Snapshot blob size 无效"))?,
                ),
            };
            let path = std::str::from_utf8(&record[tab + 1..])
                .map_err(|_| GitCommandError::new("Snapshot source 路径不是有效的 UTF-8"))?
                .to_owned();
            Ok(SnapshotSourceEntry {
                path,
                object_id: fields[2].to_owned(),
                mode,
                size,
            })
        })
        .collect()
}

/// 按 header 声明的字节数解帧，正文内任何换行或 NUL 都不参与协议分隔。
fn parse_batch(
    bytes: &[u8],
    entries: &[&SnapshotSourceEntry],
) -> Result<Vec<SnapshotSourceRead>, GitCommandError> {
    let mut remaining = bytes;
    let mut results = Vec::with_capacity(entries.len());
    for entry in entries {
        let newline = remaining
            .iter()
            .position(|byte| *byte == b'\n')
            .ok_or_else(|| GitCommandError::new("Snapshot batch 缺少对象 header"))?;
        let header = std::str::from_utf8(&remaining[..newline])
            .map_err(|_| GitCommandError::new("Snapshot batch header 不是 UTF-8"))?;
        let fields = header.split_ascii_whitespace().collect::<Vec<_>>();
        remaining = &remaining[newline + 1..];
        if fields.first().copied() != Some(entry.object_id.as_str()) {
            return Err(GitCommandError::new("Snapshot batch object ID 与请求不符"));
        }
        let content = if fields.len() == 2 && fields[1] == "missing" {
            Err(SnapshotSourceError::ObjectUnavailable {
                object_id: entry.object_id.clone(),
            })
        } else {
            if fields.len() != 3 || fields[1] != "blob" {
                return Err(GitCommandError::new("Snapshot batch 对象不是 blob"));
            }
            let size = fields[2]
                .parse::<usize>()
                .map_err(|_| GitCommandError::new("Snapshot batch size 无效"))?;
            if size >= remaining.len() || remaining[size] != b'\n' {
                return Err(GitCommandError::new("Snapshot batch blob 长度无效"));
            }
            let blob = remaining[..size].to_vec();
            remaining = &remaining[size + 1..];
            super::comparison::decode_blob(entry.mode, size, blob)
                .map(Some)
                .map_err(SnapshotSourceError::Git)
        };
        results.push(SnapshotSourceRead {
            path: entry.path.clone(),
            content,
        });
    }
    if !remaining.is_empty() {
        return Err(GitCommandError::new("Snapshot batch 含额外对象输出"));
    }
    Ok(results)
}

#[cfg(test)]
#[path = "tests/source_tree.rs"]
mod tests;
