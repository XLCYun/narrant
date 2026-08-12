#[derive(Clone, Debug, PartialEq)]
/// 从固定 Review Snapshot 读取的 File Change 内容。
pub struct LoadedFileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub status: ChangeStatus,
    pub content_kind: ContentKind,
    pub before_size: Option<usize>,
    pub after_size: Option<usize>,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
/// File Change 相对于 Comparison Base 的变更状态。
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
/// File Change 内容的稳定展示类别。
pub enum ContentKind {
    Text,
    Binary,
    Submodule,
    Symlink,
}

/// 将两侧内容类别合并为需要最高安全展示策略的类别。
fn resolved_content_kind(kinds: impl IntoIterator<Item = Option<ContentKind>>) -> ContentKind {
    kinds
        .into_iter()
        .flatten()
        .max_by_key(|kind| match kind {
            ContentKind::Submodule => 3,
            ContentKind::Symlink => 2,
            ContentKind::Binary => 1,
            ContentKind::Text => 0,
        })
        .unwrap_or(ContentKind::Text)
}

pub(super) mod local;
