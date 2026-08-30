use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

/// 单次 Read 请求的最大输出字节数，防止模型上下文被一个文件耗尽。
pub const READ_OUTPUT_LIMIT: usize = 64 * 1024;

/// Narrant 拥有的只读 Repository 工具的稳定名称。
pub const READ_TOOL_NAME: &str = "read";
/// Narrant 拥有的只读 Repository 工具的稳定名称。
pub const GLOB_TOOL_NAME: &str = "glob";
/// Narrant 拥有的只读 Repository 工具的稳定名称。
pub const GREP_TOOL_NAME: &str = "grep";

/// 一项带输入 schema 的 Repository 工具注册描述。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryToolRegistration {
    /// Rig 使用的稳定工具名。
    pub name: String,
    /// 面向模型的短工具说明。
    pub description: String,
    /// 工具输入的 JSON Schema。
    pub input_schema: Value,
}

/// Repository 工具注册视图；只暴露 Read、Glob、Grep 三项工具。
#[cfg(test)]
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct RepositoryToolRegistrationView {
    pub(super) tools: Vec<RepositoryToolRegistration>,
}

#[cfg(test)]
impl RepositoryToolRegistrationView {
    /// 返回按稳定顺序排列的工具描述。
    pub fn tools(&self) -> &[RepositoryToolRegistration] {
        &self.tools
    }
    /// 返回按注册顺序排列的工具名，便于能力探针和契约测试复用。
    pub fn names(&self) -> Vec<String> {
        self.tools.iter().map(|tool| tool.name.clone()).collect()
    }
    /// 按稳定工具名查找注册描述。
    pub fn tool(&self, name: &str) -> Option<&RepositoryToolRegistration> {
        self.tools.iter().find(|tool| tool.name == name)
    }
}

/// Read 工具的 typed 输入；offset 与 limit 都以 UTF-8 字节计数。
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadInput {
    /// Workspace 内的相对路径，也允许最终仍在 Workspace 内的绝对路径。
    pub path: String,
    /// 从内容开头跳过的字节数；省略时从零开始。
    #[serde(default)]
    pub offset: usize,
    /// 本次最多返回的字节数；省略时使用 64 KiB 上限。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[cfg(test)]
impl ReadInput {
    /// 创建从内容开头读取的请求。
    pub fn new(path: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            offset: 0,
            limit: None,
        }
    }
    /// 为请求设置字节 offset。
    pub fn with_offset(mut self, offset: usize) -> Self {
        self.offset = offset;
        self
    }
    /// 为请求设置字节 limit。
    pub fn with_limit(mut self, limit: usize) -> Self {
        self.limit = Some(limit);
        self
    }
}

/// Glob 工具输入。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobRequest {
    /// Workspace-relative glob pattern。
    pub pattern: String,
}

/// Grep 工具输入。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrepRequest {
    /// Literal/regex 搜索模式。
    pub pattern: String,
    /// Workspace-relative 搜索起点。
    pub path: String,
}

/// 统一工具执行请求；Read 与搜索都通过此 typed seam 调度。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "tool")]
pub enum RepositoryToolRequest {
    /// 读取文件或目录。
    Read(ReadInput),
    /// 匹配 Workspace 内路径。
    Glob(GlobRequest),
    /// 搜索 Workspace 内文本。
    Grep(GrepRequest),
}

/// Glob 结果的共享响应形状。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobResponse {
    /// Workspace-relative 匹配路径。
    pub paths: Vec<String>,
    /// 是否超过本次结果预算。
    pub truncated: bool,
}

/// Grep 单条匹配结果的共享响应形状。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrepMatch {
    /// Workspace-relative 文件路径。
    pub path: String,
    /// 一基行号。
    pub line_number: usize,
    /// 匹配所在完整行。
    pub line: String,
}

/// Grep 结果的共享响应形状。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrepResponse {
    /// 有界匹配行。
    pub matches: Vec<GrepMatch>,
    /// 是否超过本次结果预算。
    pub truncated: bool,
}

/// 统一工具执行响应。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "tool")]
pub enum RepositoryToolResponse {
    /// Read 输出。
    Read(ReadResponse),
    /// Glob 输出。
    Glob(GlobResponse),
    /// Grep 输出。
    Grep(GrepResponse),
}

/// Read 输出继续读取时使用的下一个字节 offset。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadContinuation {
    /// 下一次 Read 请求应使用的 offset。
    pub offset: usize,
}

/// Read 工具的 bounded、可序列化结果。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReadResponse {
    /// 解析后得到的 Workspace-relative canonical identity。
    pub path: String,
    /// 文件文本或目录条目文本。
    pub content: String,
    /// 仍有内容时的下一次读取位置。
    pub continuation: Option<ReadContinuation>,
    /// 内容是否因本次预算或调用方 limit 被截断。
    pub truncated: bool,
}

/// Repository 工具执行失败的稳定类别；不携带绝对路径或系统原文。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryToolError {
    /// Review Workspace 无法 canonicalize。
    WorkspaceUnavailable,
    /// 请求目标不存在或无法 canonicalize。
    TargetUnavailable,
    /// 请求目标 canonical identity 在 Workspace 外。
    OutsideWorkspace,
    /// 请求目标不是普通文件或目录。
    UnsupportedTarget,
    /// 请求目标包含二进制或非 UTF-8 内容。
    NonTextContent,
    /// 请求参数不满足工具契约。
    InvalidInput,
    /// 搜索模式不符合搜索工具契约。
    InvalidPattern,
    /// 文件系统访问失败，但不向调用方泄漏系统诊断。
    Io,
    /// 请求在读取过程中被取消。
    Cancelled,
}

impl fmt::Display for RepositoryToolError {
    /// 仅输出稳定、脱敏的工具诊断，不包含 Workspace 或目标路径。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::WorkspaceUnavailable => "Repository Workspace 不可用。",
            Self::TargetUnavailable => "Repository 工具目标不可用。",
            Self::OutsideWorkspace => "Repository 工具目标超出 Workspace。",
            Self::UnsupportedTarget => "Repository 工具目标类型不受支持。",
            Self::NonTextContent => "Repository 工具仅支持文本内容。",
            Self::InvalidInput => "Repository 工具输入无效。",
            Self::InvalidPattern => "Repository 工具模式无效。",
            Self::Io => "Repository 工具无法读取 Workspace。",
            Self::Cancelled => "Repository 工具操作已取消。",
        })
    }
}

impl std::error::Error for RepositoryToolError {}
