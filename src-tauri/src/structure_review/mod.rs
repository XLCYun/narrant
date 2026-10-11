//! 结构化代码审查，提供代码对象及其关系的分析契约。

pub mod code_graph;
pub mod native_analyzer;
pub(crate) mod native_languages;
pub mod python_analyzer;
pub mod rust_analyzer;
pub mod typescript_analyzer;

mod application;
mod source_text;
pub(crate) use application::{StructureComparison, StructureReview, StructureReviewError};

// 旧语言服务与原生公共图共用同一个配置目录测试夹具。
#[cfg(test)]
#[path = "tests/config_dir_fixture.rs"]
mod config_dir_fixture;
