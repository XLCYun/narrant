use std::fmt;

/// 提取失败，不能被替换为成功的空图。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalysisError {
    pub message: String,
}

impl fmt::Display for AnalysisError {
    /// 返回语言实现提供的分析失败说明。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for AnalysisError {}

/// 节点身份重复、包含关系非法或图内引用目标不存在等构图错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvalidGraph {
    pub message: String,
}

impl fmt::Display for InvalidGraph {
    /// 返回包含相关节点身份的构图错误说明。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for InvalidGraph {}

/// 两侧比较约定或必要信息不兼容。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompareError {
    pub message: String,
}

impl fmt::Display for CompareError {
    /// 返回比较无法完成的原因。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for CompareError {}
