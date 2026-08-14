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
    Symlink { size: usize, target: Option<String> },
    Submodule { commit_id: String },
}

impl FileContent {
    /// 返回内容对应的稳定展示类别。
    pub fn kind(&self) -> FileContentKind {
        match self {
            Self::Text { .. } => FileContentKind::Text,
            Self::Binary { .. } => FileContentKind::Binary,
            Self::Symlink { .. } => FileContentKind::Symlink,
            Self::Submodule { .. } => FileContentKind::Submodule,
        }
    }

    /// 返回具有 blob 字节大小语义的内容大小。
    pub fn size(&self) -> Option<usize> {
        match self {
            Self::Text { size, .. } | Self::Binary { size } | Self::Symlink { size, .. } => {
                Some(*size)
            }
            Self::Submodule { .. } => None,
        }
    }

    /// 借用可作为界面文本展示的内容。
    pub fn text(&self) -> Option<&str> {
        match self {
            Self::Text { text, .. } => Some(text),
            Self::Symlink { target, .. } => target.as_deref(),
            Self::Submodule { commit_id } => Some(commit_id),
            Self::Binary { .. } => None,
        }
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

pub(super) mod comparison;
