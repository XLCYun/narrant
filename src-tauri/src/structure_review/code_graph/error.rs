use std::fmt;

/// 提取失败，不能被替换为成功的空图。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalysisError {
    pub message: String,
    cancelled: bool,
}

impl AnalysisError {
    /// 保留真实失败，不根据同时到达的取消请求改写错误类别。
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            cancelled: false,
        }
    }

    /// 分析器明确因取消退出时使用，避免与语义分析失败混淆。
    pub fn cancelled(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            cancelled: true,
        }
    }

    /// 只返回错误自身的取消语义，不读取此后改变的运行状态。
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }
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
