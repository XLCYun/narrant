#[derive(Clone, Debug, PartialEq)]
/// 从固定 Review Snapshot 读取的 File Change 内容。
pub struct FileChangeContent {
    pub path: String,
    pub old_path: Option<String>,
    pub status: FileChangeStatus,
    pub before: Option<FileContent>,
    pub after: Option<FileContent>,
}

impl FileChangeContent {
    /// 返回 before 与 after 两侧中需要更保守展示策略的内容类别。
    pub fn content_kind(&self) -> FileContentKind {
        most_restrictive_content_kind([
            self.before.as_ref().map(FileContent::kind),
            self.after.as_ref().map(FileContent::kind),
        ])
    }
}

#[derive(Clone, Debug, PartialEq)]
/// File Content Reference 被读取后得到的单侧内容。
pub enum FileContent {
    Text { size: usize, text: String },
    Binary { size: usize },
    Symlink { size: usize, target: String },
    Submodule { commit_id: String },
}

impl FileContent {
    /// 一次分派得到内容的类别、大小和可展示文本。
    fn parts(&self) -> (FileContentKind, Option<usize>, Option<&str>) {
        match self {
            Self::Text { size, text } => (FileContentKind::Text, Some(*size), Some(text)),
            Self::Binary { size } => (FileContentKind::Binary, Some(*size), None),
            Self::Symlink { size, target } => (FileContentKind::Symlink, Some(*size), Some(target)),
            Self::Submodule { commit_id } => (FileContentKind::Submodule, None, Some(commit_id)),
        }
    }

    /// 返回内容对应的稳定展示类别。
    pub fn kind(&self) -> FileContentKind {
        self.parts().0
    }

    /// 返回具有 blob 字节大小语义的内容大小。
    pub fn size(&self) -> Option<usize> {
        self.parts().1
    }

    /// 借用可作为界面文本展示的内容。
    pub fn text(&self) -> Option<&str> {
        self.parts().2
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
/// File Change 相对于 Comparison Base 的变更状态。
pub enum FileChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
/// File Change 内容的稳定展示类别。
pub enum FileContentKind {
    Text,
    Binary,
    Submodule,
    Symlink,
}

/// 将两侧内容类别合并为需要最高安全展示策略的类别。
fn most_restrictive_content_kind(
    kinds: impl IntoIterator<Item = Option<FileContentKind>>,
) -> FileContentKind {
    kinds
        .into_iter()
        .flatten()
        .max_by_key(|kind| match kind {
            FileContentKind::Submodule => 3,
            FileContentKind::Symlink => 2,
            FileContentKind::Binary => 1,
            FileContentKind::Text => 0,
        })
        .unwrap_or(FileContentKind::Text)
}

pub(crate) use comparison::{
    build_comparison, ChangeComparison, FileChange, FileContentRef, ResolvedRevisions,
    SnapshotContentReader,
};

mod comparison;
mod source_tree;

pub(crate) use source_tree::{
    SnapshotSide, SnapshotSourceEntry, SnapshotSourceError, SnapshotSourceRead, SnapshotSourceTree,
};
