use super::*;
use crate::operation::CancellationSignal;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// 进程内 repository tool 调用序号，只用于关联开发日志。
static TOOL_CALL_SEQUENCE: AtomicU64 = AtomicU64::new(1);

const READ_TOOL_DESCRIPTION: &str =
    "Read text files or directory entries inside the Review Workspace.";
const GLOB_TOOL_DESCRIPTION: &str = "Find paths inside the Review Workspace.";
const GREP_TOOL_DESCRIPTION: &str = "Search text inside the Review Workspace.";

/// 一次 Built-in provider 分析可直接注册的三个 Rig 工具。
pub struct RepositoryRigTools {
    /// Rig Read 工具。
    pub read: RigReadTool,
    /// Rig Glob 工具。
    pub glob: RigGlobTool,
    /// Rig Grep 工具。
    pub grep: RigGrepTool,
}
/// 绑定 Workspace 与本次 Babysit 取消信号的 Rig Read 工具。
#[derive(Clone)]
pub struct RigReadTool {
    tools: RepositoryTools,
    cancellation: CancellationSignal,
}
/// 绑定 Workspace 与本次 Babysit 取消信号的 Rig Glob 工具。
#[derive(Clone)]
pub struct RigGlobTool {
    tools: RepositoryTools,
    cancellation: CancellationSignal,
}
/// 绑定 Workspace 与本次 Babysit 取消信号的 Rig Grep 工具。
#[derive(Clone)]
pub struct RigGrepTool {
    tools: RepositoryTools,
    cancellation: CancellationSignal,
}

/// 为一次可取消分析构造可直接交给 Rig agent builder 的工具。
pub(super) fn build(
    tools: &RepositoryTools,
    cancellation: &CancellationSignal,
) -> RepositoryRigTools {
    RepositoryRigTools {
        read: RigReadTool {
            tools: tools.clone(),
            cancellation: cancellation.clone(),
        },
        glob: RigGlobTool {
            tools: tools.clone(),
            cancellation: cancellation.clone(),
        },
        grep: RigGrepTool {
            tools: tools.clone(),
            cancellation: cancellation.clone(),
        },
    }
}

/// 返回固定顺序的三项工具注册。
#[cfg(test)]
pub(super) fn registrations() -> Vec<RepositoryToolRegistration> {
    vec![
        registration(READ_TOOL_NAME, READ_TOOL_DESCRIPTION, read_input_schema()),
        registration(GLOB_TOOL_NAME, GLOB_TOOL_DESCRIPTION, glob_input_schema()),
        registration(GREP_TOOL_NAME, GREP_TOOL_DESCRIPTION, grep_input_schema()),
    ]
}

/// 构造一项工具注册描述。
#[cfg(test)]
fn registration(name: &str, description: &str, input_schema: Value) -> RepositoryToolRegistration {
    RepositoryToolRegistration {
        name: name.to_owned(),
        description: description.to_owned(),
        input_schema,
    }
}

impl rig::tool::Tool for RigReadTool {
    type Args = ReadInput;
    type Output = ReadResponse;
    type Error = RepositoryToolError;
    const NAME: &'static str = READ_TOOL_NAME;
    /// 返回 Rig 暴露给模型的 Read 描述。
    fn description(&self) -> String {
        READ_TOOL_DESCRIPTION.to_owned()
    }
    /// 返回 Read 的有界输入 schema。
    fn parameters(&self) -> Value {
        read_input_schema()
    }
    /// 通过 Narrant seam 执行 Rig Read 调用。
    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let sequence = TOOL_CALL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let input_bytes = serialized_size(&args);
        let started_at = Instant::now();
        tracing::debug!(target: "narrant::babysit::tool", tool = READ_TOOL_NAME, sequence, input_bytes, phase = "started");
        #[cfg(test)]
        self.tools.trigger_execute_cancellation();
        let result = self.tools.read(args, &self.cancellation);
        log_tool_completion(READ_TOOL_NAME, sequence, input_bytes, started_at, &result);
        result
    }
}

impl rig::tool::Tool for RigGlobTool {
    type Args = GlobRequest;
    type Output = GlobResponse;
    type Error = RepositoryToolError;
    const NAME: &'static str = GLOB_TOOL_NAME;
    /// 返回 Rig 暴露给模型的 Glob 描述。
    fn description(&self) -> String {
        GLOB_TOOL_DESCRIPTION.to_owned()
    }
    /// 返回 Glob 的输入 schema。
    fn parameters(&self) -> Value {
        glob_input_schema()
    }
    /// 通过 Narrant seam 执行 Rig Glob 调用。
    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let sequence = TOOL_CALL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let input_bytes = serialized_size(&args);
        let started_at = Instant::now();
        tracing::debug!(target: "narrant::babysit::tool", tool = GLOB_TOOL_NAME, sequence, input_bytes, phase = "started");
        #[cfg(test)]
        self.tools.trigger_execute_cancellation();
        let result = glob::execute(&self.tools.workspace_root, &args, &self.cancellation);
        log_tool_completion(GLOB_TOOL_NAME, sequence, input_bytes, started_at, &result);
        result
    }
}

impl rig::tool::Tool for RigGrepTool {
    type Args = GrepRequest;
    type Output = GrepResponse;
    type Error = RepositoryToolError;
    const NAME: &'static str = GREP_TOOL_NAME;
    /// 返回 Rig 暴露给模型的 Grep 描述。
    fn description(&self) -> String {
        GREP_TOOL_DESCRIPTION.to_owned()
    }
    /// 返回 Grep 的输入 schema。
    fn parameters(&self) -> Value {
        grep_input_schema()
    }
    /// 通过 Narrant seam 执行 Rig Grep 调用。
    async fn call(
        &self,
        _context: &mut rig::tool::ToolContext,
        args: Self::Args,
    ) -> Result<Self::Output, Self::Error> {
        let sequence = TOOL_CALL_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let input_bytes = serialized_size(&args);
        let started_at = Instant::now();
        tracing::debug!(target: "narrant::babysit::tool", tool = GREP_TOOL_NAME, sequence, input_bytes, phase = "started");
        #[cfg(test)]
        self.tools.trigger_execute_cancellation();
        let result = grep::execute(&self.tools.workspace_root, &args, &self.cancellation);
        log_tool_completion(GREP_TOOL_NAME, sequence, input_bytes, started_at, &result);
        result
    }
}

/// 计算结构化值的序列化字节数，不把值本身写入日志。
fn serialized_size(value: &impl serde::Serialize) -> usize {
    serde_json::to_vec(value).map_or(0, |bytes| bytes.len())
}

/// 记录 repository tool 的安全完成元数据。
fn log_tool_completion<T: serde::Serialize + ToolOutputMetadata>(
    tool: &'static str,
    sequence: u64,
    input_bytes: usize,
    started_at: Instant,
    result: &Result<T, RepositoryToolError>,
) {
    tracing::debug!(
        target: "narrant::babysit::tool",
        tool,
        sequence,
        input_bytes,
        output_bytes = result.as_ref().map_or(0, |output| serialized_size(output)),
        truncated = result.as_ref().is_ok_and(ToolOutputMetadata::truncated),
        status = if result.is_ok() { "ok" } else { "error" },
        elapsed_ms = started_at.elapsed().as_millis() as u64,
        phase = "completed",
    );
}

/// 提取不同 repository tool 响应共享的截断标记。
trait ToolOutputMetadata {
    /// 返回响应是否因预算而截断。
    fn truncated(&self) -> bool;
}

impl ToolOutputMetadata for ReadResponse {
    fn truncated(&self) -> bool {
        self.truncated
    }
}

impl ToolOutputMetadata for GlobResponse {
    fn truncated(&self) -> bool {
        self.truncated
    }
}

impl ToolOutputMetadata for GrepResponse {
    fn truncated(&self) -> bool {
        self.truncated
    }
}

/// 构造 Read 输入的严格 JSON Schema。
fn read_input_schema() -> Value {
    json!({"type":"object","properties":{"path":{"type":"string"},"offset":{"type":"integer","minimum":0},"limit":{"type":"integer","minimum":0}},"required":["path"],"additionalProperties":false})
}
/// 构造 Glob 输入的严格 JSON Schema。
fn glob_input_schema() -> Value {
    json!({"type":"object","properties":{"pattern":{"type":"string"}},"required":["pattern"],"additionalProperties":false})
}
/// 构造 Grep 输入的严格 JSON Schema。
fn grep_input_schema() -> Value {
    json!({"type":"object","properties":{"pattern":{"type":"string"},"path":{"type":"string"}},"required":["pattern","path"],"additionalProperties":false})
}
