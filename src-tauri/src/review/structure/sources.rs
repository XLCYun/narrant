//! 固定 Snapshot 的真实文件配对与独立完整源码 Peek，不依赖解析结果。

use super::model::*;
use crate::core_git::{CoreGitFailureKind, GitCommandError};
use crate::operation::OperationContext;
use crate::review::content::{
    ChangeComparison, FileChange, FileChangeStatus, FileContent, FileContentRef,
    SnapshotContentReader, SnapshotSide, SnapshotSourceError, SnapshotSourceTree,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// 一个 Snapshot 的真实文件对目录，按两侧路径索引，避免 rename 后旧路径复用产生歧义。
pub(crate) struct SourceCatalog {
    pub(crate) sources: Vec<StructurePeekSource>,
    before: HashMap<String, usize>,
    after: HashMap<String, usize>,
}

impl SourceCatalog {
    /// 先保留全部真实 File Changes，再加入任一成功 tree 发现的未修改上下文。
    pub(crate) fn build(
        reader: &SnapshotContentReader,
        before: Option<&SnapshotSourceTree>,
        after: Option<&SnapshotSourceTree>,
    ) -> Self {
        let comparison = reader.comparison();
        // 旧异步发现结果不能把另一份 Snapshot 的路径标成当前版本的上下文。
        let before = before.filter(|tree| tree.revision() == comparison.comparison_base);
        let after = after.filter(|tree| tree.revision() == comparison.head_revision);
        let mut sources = BTreeMap::new();
        let mut changed_paths = HashSet::new();
        for change in &comparison.file_changes {
            let source = changed_source(comparison, change);
            for side in [&source.before, &source.after] {
                if let StructureSourceSide::Present { path, .. } = side {
                    changed_paths.insert(path.clone());
                }
            }
            sources.insert(change.path.clone(), source);
        }
        let paths = before
            .into_iter()
            .chain(after)
            .flat_map(|tree| tree.entries())
            .map(|entry| entry.path.clone())
            .collect::<BTreeSet<_>>();
        for path in paths {
            if !changed_paths.contains(&path) {
                sources.insert(path.clone(), unchanged_source(comparison, &path));
            }
        }
        let sources = sources.into_values().collect::<Vec<_>>();
        let mut before = HashMap::new();
        let mut after = HashMap::new();
        for (index, source) in sources.iter().enumerate() {
            if let StructureSourceSide::Present { path, .. } = &source.before {
                before.insert(path.clone(), index);
            }
            if let StructureSourceSide::Present { path, .. } = &source.after {
                after.insert(path.clone(), index);
            }
        }
        Self {
            sources,
            before,
            after,
        }
    }

    /// 按真实侧与路径查找文件对；前侧 rename 源和后侧同名新增文件分别命中。
    pub(crate) fn lookup(&self, side: StructureSide, path: &str) -> Option<&StructurePeekSource> {
        let index = match side {
            StructureSide::Before => self.before.get(path),
            StructureSide::After => self.after.get(path),
        }?;
        self.sources.get(*index)
    }
}

/// 从实际侧与路径打开完整文件对，无需等待结构分析完成。
pub(crate) fn read_structure_source(
    reader: &SnapshotContentReader,
    operation: &OperationContext,
    review_id: &str,
    snapshot_id: &str,
    request: &StructureSourceRequest,
) -> Result<StructurePeekContent, GitCommandError> {
    check_cancelled(operation)?;
    let comparison = reader.comparison();
    let known_change = comparison
        .file_changes
        .iter()
        .find(|change| match request.side {
            StructureSide::Before => change.before.is_some() && before_path(change) == request.path,
            StructureSide::After => change.after.is_some() && change.path == request.path,
        });
    let (source, before, after) = if let Some(change) = known_change {
        let source = changed_source(comparison, change);
        let before =
            read_reference_side(reader, operation, &source.before, change.before.as_ref())?;
        let after = read_reference_side(reader, operation, &source.after, change.after.as_ref())?;
        (source, before, after)
    } else {
        let before_tree = reader.source_tree(operation, SnapshotSide::Before);
        check_cancelled(operation)?;
        let after_tree = reader.source_tree(operation, SnapshotSide::After);
        check_cancelled(operation)?;
        let catalog =
            SourceCatalog::build(reader, before_tree.as_ref().ok(), after_tree.as_ref().ok());
        let source = catalog
            .lookup(request.side, &request.path)
            .cloned()
            .ok_or_else(|| {
                let error = match request.side {
                    StructureSide::Before => before_tree.as_ref().err(),
                    StructureSide::After => after_tree.as_ref().err(),
                };
                error.cloned().unwrap_or_else(|| {
                    GitCommandError::new("请求路径不属于该 Snapshot 的实际源码侧")
                })
            })?;
        let before = read_tree_side(operation, &source.before, &before_tree)?;
        let after = read_tree_side(operation, &source.after, &after_tree)?;
        (source, before, after)
    };
    check_cancelled(operation)?;
    Ok(StructurePeekContent {
        review_id: review_id.to_owned(),
        snapshot_id: snapshot_id.to_owned(),
        source_id: source.id.clone(),
        source,
        before,
        after,
    })
}

/// 只有真实 rename 使用 old_path；其他状态的旧侧仍属于自身 path。
fn before_path(change: &FileChange) -> &str {
    if change.status == FileChangeStatus::Renamed {
        change.old_path.as_deref().unwrap_or(&change.path)
    } else {
        &change.path
    }
}

/// 将比较已确认的单侧引用转换成实际路径描述符，缺引用才表示真缺侧。
fn changed_source(comparison: &ChangeComparison, change: &FileChange) -> StructurePeekSource {
    StructurePeekSource {
        id: StructureSourceId(format!("source:{}", change.path)),
        before: source_side(
            change.before.is_some(),
            before_path(change),
            &comparison.comparison_base,
        ),
        after: source_side(
            change.after.is_some(),
            &change.path,
            &comparison.head_revision,
        ),
        classification: StructureSourceClassification::FileChange {
            file_change: StructureFileChangeSummary {
                path: change.path.clone(),
                old_path: change.old_path.clone(),
                status: match change.status {
                    FileChangeStatus::Added => StructureFileChangeStatus::Added,
                    FileChangeStatus::Modified => StructureFileChangeStatus::Modified,
                    FileChangeStatus::Deleted => StructureFileChangeStatus::Deleted,
                    FileChangeStatus::Renamed => StructureFileChangeStatus::Renamed,
                },
            },
        },
    }
}

/// 完整比较未记录路径变化且任一 tree 已发现它，可证明两侧都有同一未修改文件。
fn unchanged_source(comparison: &ChangeComparison, path: &str) -> StructurePeekSource {
    StructurePeekSource {
        id: StructureSourceId(format!("source:{path}")),
        before: source_side(true, path, &comparison.comparison_base),
        after: source_side(true, path, &comparison.head_revision),
        classification: StructureSourceClassification::Unchanged {
            path: path.to_owned(),
        },
    }
}

/// 描述符的缺侧仅来自已固定比较记录，而不是读取错误。
fn source_side(present: bool, path: &str, revision: &str) -> StructureSourceSide {
    if present {
        StructureSourceSide::Present {
            path: path.to_owned(),
            revision: revision.to_owned(),
        }
    } else {
        StructureSourceSide::Absent
    }
}

/// 已知 File Change 的固定 blob 可脱离已被回收的 commit/tree 独立读取。
fn read_reference_side(
    reader: &SnapshotContentReader,
    operation: &OperationContext,
    side: &StructureSourceSide,
    reference: Option<&FileContentRef>,
) -> Result<StructureSourceContent, GitCommandError> {
    check_cancelled(operation)?;
    let StructureSourceSide::Present { path, revision } = side else {
        return Ok(absent_content());
    };
    let Some(reference) = reference else {
        return Ok(unavailable(
            StructureSourceUnavailableReason::SourceUnreadable,
            "Snapshot 来源缺少内容引用",
            false,
        ));
    };
    content_result(
        reader.read_reference(operation, reference),
        path,
        revision,
        &reference.object_id,
    )
}

/// 一侧发现失败只影响该侧；成功 tree 的真实对象和路径用于读取未修改文件。
fn read_tree_side(
    operation: &OperationContext,
    side: &StructureSourceSide,
    tree: &Result<SnapshotSourceTree, GitCommandError>,
) -> Result<StructureSourceContent, GitCommandError> {
    check_cancelled(operation)?;
    let StructureSourceSide::Present { path, revision } = side else {
        return Ok(absent_content());
    };
    let tree = match tree {
        Ok(tree) => tree,
        Err(error) => return map_git_error(error.clone()),
    };
    let Some(entry) = tree.entries().iter().find(|entry| entry.path == *path) else {
        // 比较证明此上下文未变化；矛盾的 tree 元数据不能制造一个新的删除结论。
        return Ok(unavailable(
            StructureSourceUnavailableReason::SourceUnreadable,
            "Snapshot tree 与未修改来源记录不一致",
            true,
        ));
    };
    let content = tree.read(operation, path).and_then(|content| {
        content.ok_or_else(|| {
            SnapshotSourceError::Git(GitCommandError::new("已枚举来源读取时缺少内容"))
        })
    });
    content_result(content, path, revision, &entry.object_id)
}

/// 保留完整原始文本和真实 object ID，其他 Git 类型明确表示不可作为源码阅读。
fn content_result(
    content: Result<FileContent, SnapshotSourceError>,
    path: &str,
    revision: &str,
    object_id: &str,
) -> Result<StructureSourceContent, GitCommandError> {
    Ok(match content {
        Ok(FileContent::Text { size, text }) => StructureSourceContent::Available {
            path: path.to_owned(),
            revision: revision.to_owned(),
            object_id: object_id.to_owned(),
            text,
            size: size as u64,
        },
        Ok(FileContent::Binary { .. }) => unavailable(
            StructureSourceUnavailableReason::Binary,
            "该来源是二进制文件",
            false,
        ),
        Ok(FileContent::Symlink { .. }) => unavailable(
            StructureSourceUnavailableReason::Symlink,
            "该来源是符号链接，不是链接目标源码",
            false,
        ),
        Ok(FileContent::Submodule { .. }) => unavailable(
            StructureSourceUnavailableReason::Submodule,
            "该来源是子模块引用",
            false,
        ),
        Err(SnapshotSourceError::ObjectUnavailable { object_id }) => unavailable(
            StructureSourceUnavailableReason::GitObjectUnavailable,
            format!("Snapshot Git 对象不可用：{object_id}"),
            true,
        ),
        Err(SnapshotSourceError::UnsupportedMode { mode }) => unavailable(
            StructureSourceUnavailableReason::SourceUnreadable,
            format!("不支持的 Git 文件模式：{mode:o}"),
            false,
        ),
        Err(SnapshotSourceError::Git(error)) => return map_git_error(error),
    })
}

/// 取消中止整个请求；资源限制、对象不可用和其他读取故障保留独立原因。
fn map_git_error(error: GitCommandError) -> Result<StructureSourceContent, GitCommandError> {
    let reason = match error.kind() {
        CoreGitFailureKind::Cancelled => return Err(error),
        CoreGitFailureKind::OutputLimitExceeded { .. } => {
            StructureSourceUnavailableReason::ResourceLimit
        }
        CoreGitFailureKind::Exited { .. } => StructureSourceUnavailableReason::GitObjectUnavailable,
        _ => StructureSourceUnavailableReason::SourceUnreadable,
    };
    Ok(unavailable(reason, error.to_string(), true))
}

/// 统一构造可展示的来源不可用状态，避免任何空文本回退。
fn unavailable(
    reason: StructureSourceUnavailableReason,
    message: impl Into<String>,
    retryable: bool,
) -> StructureSourceContent {
    StructureSourceContent::Unavailable {
        reason,
        message: message.into(),
        retryable,
    }
}

/// 仅已确认缺侧可构造这一状态。
fn absent_content() -> StructureSourceContent {
    StructureSourceContent::Absent {
        reason: StructureSourceAbsenceReason::GitTreeAbsence,
    }
}

/// 不含实际 I/O 的描述符路径也遵守本次应用操作的取消信号。
fn check_cancelled(operation: &OperationContext) -> Result<(), GitCommandError> {
    if operation.cancellation().is_cancelled() {
        Err(GitCommandError::cancelled())
    } else {
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/sources.rs"]
mod tests;
