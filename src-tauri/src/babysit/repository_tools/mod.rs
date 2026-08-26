use crate::operation::CancellationSignal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;
use std::fs::{self, File};
use std::io::Read as IoRead;
use std::path::{Path, PathBuf};

mod glob;
mod grep;

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
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct RepositoryToolRegistrationView {
    tools: Vec<RepositoryToolRegistration>,
}

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

/// Glob 工具输入；业务实现由搜索工具分片接管。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobRequest {
    /// Workspace-relative glob pattern。
    pub pattern: String,
}

/// Grep 工具输入；业务实现由搜索工具分片接管。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GrepRequest {
    /// Literal/regex 搜索模式由搜索分片定义。
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

/// 统一工具执行响应；Read 与搜索都可被 Rig 适配器序列化。
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

/// 绑定一个 canonical Review Workspace 的 Narrant-owned 工具集合。
#[derive(Clone, Debug)]
pub struct RepositoryTools {
    pub(super) workspace_root: PathBuf,
}

impl RepositoryTools {
    /// canonicalize 一次 Review Workspace，并拒绝非目录根。
    pub fn new(workspace_root: impl AsRef<Path>) -> Result<Self, RepositoryToolError> {
        let canonical_root = fs::canonicalize(workspace_root.as_ref())
            .map_err(|_| RepositoryToolError::WorkspaceUnavailable)?;
        if !canonical_root.is_dir() {
            return Err(RepositoryToolError::WorkspaceUnavailable);
        }
        Ok(Self {
            workspace_root: canonical_root,
        })
    }

    /// 返回仅含 Read、Glob、Grep 且每项带输入 schema 的注册视图。
    pub fn registration(&self) -> RepositoryToolRegistrationView {
        RepositoryToolRegistrationView {
            tools: self.registrations(),
        }
    }

    /// 返回固定顺序的三项工具注册，供 Rig 适配器直接消费。
    pub fn registrations(&self) -> Vec<RepositoryToolRegistration> {
        vec![
            RepositoryToolRegistration {
                name: READ_TOOL_NAME.to_owned(),
                description: "Read text files or directory entries inside the Review Workspace."
                    .to_owned(),
                input_schema: read_input_schema(),
            },
            RepositoryToolRegistration {
                name: GLOB_TOOL_NAME.to_owned(),
                description: "Find paths inside the Review Workspace.".to_owned(),
                input_schema: glob_input_schema(),
            },
            RepositoryToolRegistration {
                name: GREP_TOOL_NAME.to_owned(),
                description: "Search text inside the Review Workspace.".to_owned(),
                input_schema: grep_input_schema(),
            },
        ]
    }

    /// 执行 typed Read 请求；读取实现随后在同一个 public seam 中提供。
    pub fn read(
        &self,
        input: ReadInput,
        cancellation: &CancellationSignal,
    ) -> Result<ReadResponse, RepositoryToolError> {
        ensure_not_cancelled(cancellation)?;
        let (target, relative_path) = self.resolve_target(&input.path, cancellation)?;
        ensure_not_cancelled(cancellation)?;

        let metadata = fs::metadata(&target).map_err(|_| RepositoryToolError::TargetUnavailable)?;
        if metadata.is_file() {
            read_file(&target, relative_path, &input, cancellation)
        } else if metadata.is_dir() {
            read_directory(&target, relative_path, &input, cancellation)
        } else {
            Err(RepositoryToolError::UnsupportedTarget)
        }
    }

    /// 通过统一 typed seam 执行 Read、Glob 或 Grep 工具。
    pub fn execute(
        &self,
        request: RepositoryToolRequest,
        cancellation: &CancellationSignal,
    ) -> Result<RepositoryToolResponse, RepositoryToolError> {
        match request {
            RepositoryToolRequest::Read(input) => self
                .read(input, cancellation)
                .map(RepositoryToolResponse::Read),
            RepositoryToolRequest::Glob(input) => {
                glob::execute(&self.workspace_root, &input, cancellation)
                    .map(RepositoryToolResponse::Glob)
            }
            RepositoryToolRequest::Grep(input) => {
                grep::execute(&self.workspace_root, &input, cancellation)
                    .map(RepositoryToolResponse::Grep)
            }
        }
    }

    /// 将用户路径解析为 canonical target，并以组件边界证明其位于 Workspace 内。
    fn resolve_target(
        &self,
        requested_path: &str,
        cancellation: &CancellationSignal,
    ) -> Result<(PathBuf, String), RepositoryToolError> {
        ensure_not_cancelled(cancellation)?;
        let requested = Path::new(requested_path);
        let candidate = if requested.is_absolute() {
            requested.to_owned()
        } else {
            self.workspace_root.join(requested)
        };
        let canonical =
            fs::canonicalize(candidate).map_err(|_| RepositoryToolError::TargetUnavailable)?;
        ensure_not_cancelled(cancellation)?;
        if canonical.strip_prefix(&self.workspace_root).is_err() {
            return Err(RepositoryToolError::OutsideWorkspace);
        }
        Ok((
            canonical.clone(),
            workspace_relative_path(&self.workspace_root, &canonical),
        ))
    }
}

/// 检查共享取消 signal，并将取消统一映射为稳定工具错误。
fn ensure_not_cancelled(cancellation: &CancellationSignal) -> Result<(), RepositoryToolError> {
    if cancellation.is_cancelled() {
        Err(RepositoryToolError::Cancelled)
    } else {
        Ok(())
    }
}

/// 读取普通文本文件，在扫描过程中维持 UTF-8 校验和 bounded 输出缓冲。
fn read_file(
    target: &Path,
    relative_path: String,
    input: &ReadInput,
    cancellation: &CancellationSignal,
) -> Result<ReadResponse, RepositoryToolError> {
    let mut file = File::open(target).map_err(|_| RepositoryToolError::TargetUnavailable)?;
    let output_limit = input
        .limit
        .unwrap_or(READ_OUTPUT_LIMIT)
        .min(READ_OUTPUT_LIMIT);
    let mut output = Vec::with_capacity(output_limit);
    let mut utf8_tail = Vec::new();
    let mut offset = 0usize;
    let mut buffer = [0u8; 8 * 1024];

    loop {
        ensure_not_cancelled(cancellation)?;
        let read = file
            .read(&mut buffer)
            .map_err(|_| RepositoryToolError::TargetUnavailable)?;
        if read == 0 {
            break;
        }
        let bytes = &buffer[..read];
        if bytes.contains(&0) {
            return Err(RepositoryToolError::NonTextContent);
        }
        validate_utf8_chunk(bytes, &mut utf8_tail)?;

        let chunk_end = offset.saturating_add(read);
        let requested_end = input.offset.saturating_add(output_limit);
        let capture_start = input.offset.max(offset);
        let capture_end = requested_end.min(chunk_end);
        if capture_start < capture_end {
            let start = capture_start - offset;
            let end = capture_end - offset;
            output.extend_from_slice(&bytes[start..end]);
        }
        offset = chunk_end;
    }
    ensure_not_cancelled(cancellation)?;
    if !utf8_tail.is_empty() {
        return Err(RepositoryToolError::NonTextContent);
    }

    let text = text_from_captured_bytes(output)?;
    let next_offset = input.offset.saturating_add(text.len());
    Ok(ReadResponse {
        path: relative_path,
        content: text,
        continuation: (next_offset < offset).then_some(ReadContinuation {
            offset: next_offset,
        }),
        truncated: next_offset < offset,
    })
}

/// 读取目录条目并按名称排序，再沿用 Read 的统一字节预算和 continuation 语义。
fn read_directory(
    target: &Path,
    relative_path: String,
    input: &ReadInput,
    cancellation: &CancellationSignal,
) -> Result<ReadResponse, RepositoryToolError> {
    let mut entries = Vec::new();
    let directory = fs::read_dir(target).map_err(|_| RepositoryToolError::TargetUnavailable)?;
    for entry in directory {
        ensure_not_cancelled(cancellation)?;
        let entry = entry.map_err(|_| RepositoryToolError::TargetUnavailable)?;
        entries.push(entry.file_name().to_string_lossy().into_owned());
    }
    ensure_not_cancelled(cancellation)?;
    entries.sort();
    let listing = entries.join("\n");
    let (content, next_offset, truncated) = slice_text(&listing, input)?;
    ensure_not_cancelled(cancellation)?;
    Ok(ReadResponse {
        path: relative_path,
        content,
        continuation: truncated.then_some(ReadContinuation {
            offset: next_offset,
        }),
        truncated,
    })
}

/// 将目录 listing 或文件截取结果限制在合法 UTF-8 字符边界内。
fn slice_text(text: &str, input: &ReadInput) -> Result<(String, usize, bool), RepositoryToolError> {
    let bytes = text.as_bytes();
    let start = input.offset.min(bytes.len());
    if !text.is_char_boundary(start) {
        return Err(RepositoryToolError::InvalidInput);
    }
    let output_limit = input
        .limit
        .unwrap_or(READ_OUTPUT_LIMIT)
        .min(READ_OUTPUT_LIMIT);
    let requested_end = start.saturating_add(output_limit).min(bytes.len());
    let mut end = requested_end;
    while end > start && !text.is_char_boundary(end) {
        end -= 1;
    }
    let content = text[start..end].to_owned();
    let next_offset = input.offset.saturating_add(content.len());
    let truncated = end < bytes.len();
    Ok((content, next_offset, truncated))
}

/// 将文件 bounded 字节缓冲解码为文本，并区分起点不合法和终点被预算截断。
fn text_from_captured_bytes(mut bytes: Vec<u8>) -> Result<String, RepositoryToolError> {
    let had_bytes = !bytes.is_empty();
    match String::from_utf8(bytes) {
        Ok(text) => Ok(text),
        Err(error) => {
            let utf8_error = error.utf8_error();
            if utf8_error.error_len().is_some() {
                return Err(RepositoryToolError::InvalidInput);
            }
            let valid = utf8_error.valid_up_to();
            bytes = error.into_bytes();
            bytes.truncate(valid);
            if valid == 0 && had_bytes {
                return Err(RepositoryToolError::InvalidInput);
            }
            String::from_utf8(bytes).map_err(|_| RepositoryToolError::InvalidInput)
        }
    }
}

/// 增量校验文件字节，允许 chunk 边界落在 UTF-8 多字节序列中。
fn validate_utf8_chunk(bytes: &[u8], pending: &mut Vec<u8>) -> Result<(), RepositoryToolError> {
    let previous = std::mem::take(pending);
    let mut combined = Vec::with_capacity(previous.len() + bytes.len());
    combined.extend_from_slice(&previous);
    combined.extend_from_slice(bytes);
    match std::str::from_utf8(&combined) {
        Ok(_) => Ok(()),
        Err(error) if error.error_len().is_none() => {
            pending.extend_from_slice(&combined[error.valid_up_to()..]);
            if pending.len() <= 3 {
                Ok(())
            } else {
                Err(RepositoryToolError::NonTextContent)
            }
        }
        Err(_) => Err(RepositoryToolError::NonTextContent),
    }
}

/// 把 canonical target 投影为稳定的 workspace-relative、跨平台路径身份。
fn workspace_relative_path(workspace_root: &Path, target: &Path) -> String {
    let relative = target
        .strip_prefix(workspace_root)
        .expect("canonical target containment is checked before identity projection");
    let components = relative
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if components.is_empty() {
        ".".to_owned()
    } else {
        components.join("/")
    }
}

/// 构造 Read 输入的严格 JSON Schema，供 Rig 注册和契约测试共用。
fn read_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "path": {"type": "string"},
            "offset": {"type": "integer", "minimum": 0},
            "limit": {"type": "integer", "minimum": 0}
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

/// 构造 Glob 输入 schema；搜索执行实现由另一分片接管。
fn glob_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {"pattern": {"type": "string"}},
        "required": ["pattern"],
        "additionalProperties": false
    })
}

/// 构造 Grep 输入 schema；搜索执行实现由另一分片接管。
fn grep_input_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "pattern": {"type": "string"},
            "path": {"type": "string"}
        },
        "required": ["pattern", "path"],
        "additionalProperties": false
    })
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
