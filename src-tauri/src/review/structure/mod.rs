//! Review 结构分析的内部解析边界；语法提供方不进入前端契约。

mod engine;
mod extraction;
mod extraction_adapter;
mod extraction_bindings;
mod extraction_model;
mod extraction_rust;
mod extraction_typescript;
mod matching;
mod matching_model;
mod model;
mod parser;
mod parser_model;
mod relationships;
mod repository_index;
mod sources;

pub(crate) use engine::analyze_structure;
pub(crate) use extraction::{extract_source, fingerprint_tokens};
pub(crate) use extraction_model::*;
pub(crate) use matching::compare_declarations;
pub(crate) use model::*;
pub(crate) use parser::parse_source;
pub(crate) use parser_model::*;
pub(crate) use relationships::resolve_relationships;
pub(crate) use sources::read_structure_source;

#[cfg(test)]
#[path = "tests/parser.rs"]
mod parser_tests;

#[cfg(test)]
#[path = "tests/extraction.rs"]
mod extraction_tests;

#[cfg(test)]
#[path = "tests/extraction_typescript.rs"]
mod extraction_typescript_tests;

#[cfg(test)]
#[path = "tests/extraction_rust.rs"]
mod extraction_rust_tests;

#[cfg(test)]
#[path = "tests/extraction_bindings.rs"]
mod extraction_binding_tests;
