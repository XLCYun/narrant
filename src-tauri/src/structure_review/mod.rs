//! 结构化代码审查，提供代码对象及其关系的分析契约。

pub mod code_graph;
pub mod python_analyzer;
pub mod rust_analyzer;
pub mod typescript_analyzer;

mod application;
mod source_text;
pub(crate) use application::{StructureComparison, StructureReview, StructureReviewError};
