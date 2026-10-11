//! 结构化代码审查，提供代码对象及其关系的分析契约。

pub mod code_graph;
pub mod native_analyzer;
pub(crate) mod native_languages;

mod application;
mod source_text;
pub(crate) use application::{StructureComparison, StructureReview, StructureReviewError};

// 原生 TS 图复用历史配置目录场景，夹具独立于语言实现。
#[cfg(test)]
#[path = "tests/config_dir_fixture.rs"]
mod config_dir_fixture;
