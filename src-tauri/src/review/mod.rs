mod content;
mod lifecycle;
mod selection;

pub(crate) use content::local::compare_exact;
pub(crate) use content::local::{
    ExactComparison, LocalEntry, LocalFileChange, LocalSnapshot, LocalSnapshotReader,
};
pub(crate) use content::{ChangeStatus, ContentKind, LoadedFileChange};
pub(crate) use lifecycle::*;
pub(crate) use selection::*;

#[cfg(test)]
#[path = "tests/local_comparison/mod.rs"]
mod local_comparison_tests;
