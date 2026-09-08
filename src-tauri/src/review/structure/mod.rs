//! Review 结构分析的内部解析边界；语法提供方不进入前端契约。

mod parser;
mod parser_model;

pub(crate) use parser::parse_source;
pub(crate) use parser_model::*;

#[cfg(test)]
#[path = "tests/parser.rs"]
mod parser_tests;
