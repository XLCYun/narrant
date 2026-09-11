//! 多语言代码图及其差异，通过本模块统一构建、查询和比较。
//!
//! 语言实现负责源码提取、身份生成和签名规范化；本模块不读取仓库或调用 LSP。
//! 当前只比较声明和关系，尚不包含函数体或内容指纹。

mod analyzer;
mod diff;
mod error;
mod graph;
mod node;
mod validation;

pub use analyzer::{AnalysisInput, LanguageAnalyzer};
pub use diff::{ChangeKind, CodeGraphDiff, NodeDiff, SetChange};
pub use error::{AnalysisError, CompareError, InvalidGraph};
pub use graph::{CodeGraph, CodeRoot};
pub use node::{
    CodeCategory, CodeNode, CodeNodeData, CodeReference, ExtensionFields, LanguageId, NodeId,
    ReferenceTarget,
};

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
