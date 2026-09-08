mod content;
mod lifecycle;
mod selection;
pub(crate) mod structure;

pub(crate) use content::{
    build_comparison, ChangeComparison, FileChange, FileContentRef, ResolvedRevisions,
    SnapshotContentReader,
};
pub(crate) use content::{FileChangeContent, FileChangeStatus, FileContent, FileContentKind};
pub(crate) use lifecycle::*;
pub(crate) use selection::*;

#[cfg(test)]
#[path = "tests/local_comparison/mod.rs"]
mod local_comparison_tests;
