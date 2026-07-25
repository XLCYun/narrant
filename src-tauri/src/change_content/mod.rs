use git2::{Delta, DiffFindOptions, DiffOptions, Repository};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
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
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ContentKind {
    Text,
    Binary,
    Submodule,
    Symlink,
}

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

pub mod github;
pub mod local;
