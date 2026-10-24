//! 结构化代码审查，提供代码对象及其关系的分析契约。

pub mod code_graph;
pub mod codegraph_client;

mod application;
pub(crate) use application::{StructureComparison, StructureReview, StructureReviewError};
