//! TypeScript / JavaScript 适配器：语法文档与后续语义查询共享稳定接口。

mod identity;
pub(crate) mod syntax;
mod test_code;
pub(crate) mod text;

use super::code_graph::AnalysisError;

/// 适配器内部统一返回公共分析错误，解析失败不能伪装为空图。
pub(crate) type Result<T> = std::result::Result<T, AnalysisError>;

/// 为各层保留统一的可读错误入口。
pub(crate) fn failure(message: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::new(message.to_string())
}
