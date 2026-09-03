use rig::completion::{AssistantContent, Message, ResponseIdentity};
use rig::message::ToolCall;

/// Custom 协议适配器共享的单次模型调用上限。
pub(crate) const MAX_MODEL_TURNS: usize = 16;
/// Custom 协议适配器共享的单次模型输出上限。
pub(crate) const MAX_OUTPUT_TOKENS: u64 = 65_536;

/// 判断 Rig agent run 是否有任一模型请求因固定输出预算结束。
pub(crate) fn reached_output_limit(calls: &[rig::agent::CompletionCall]) -> bool {
    calls.iter().any(|call| {
        call.finish_reason
            .as_ref()
            .is_some_and(|reason| matches!(reason, rig::completion::FinishReason::Length))
    })
}

/// 单次模型请求的 provider-neutral 输入；不含任何协议 wire 类型。
#[derive(Clone, Debug)]
pub(crate) struct CustomRunRequest {
    prompt: Message,
    history: Vec<Message>,
    repository_exploration: bool,
}

impl CustomRunRequest {
    /// 构造由 adapter 自己持有工具集合的单次 Rig agent-loop 请求。
    pub(crate) fn from_prompt(
        prompt: Message,
        history: Vec<Message>,
        repository_exploration: bool,
    ) -> Self {
        Self {
            prompt,
            history,
            repository_exploration,
        }
    }

    /// 返回本轮 prompt 的 provider-neutral 消息。
    pub(super) fn prompt(&self) -> &Message {
        &self.prompt
    }
    /// 返回同一会话的既有消息历史。
    pub(super) fn history(&self) -> &[Message] {
        &self.history
    }
    /// 返回本轮是否注册仓库探索工具。
    pub(super) const fn repository_exploration(&self) -> bool {
        self.repository_exploration
    }
}

/// 一次完整 Rig agent run 的 provider-neutral 结果；协议 wire response 在 adapter 内被抹平。
#[derive(Clone, Debug)]
pub(crate) struct CustomRunResult {
    /// Rig 归一化后的 assistant 内容，工具调用保持可调度信息。
    pub(crate) choice: Vec<AssistantContent>,
    /// provider 为本轮返回的三类可选 identity。
    pub(crate) identity: ResponseIdentity,
    /// Rig Agent 返回的本轮 transcript；跨轮合并由共享 runtime 负责。
    pub(crate) transcript: Option<Vec<Message>>,
}

impl CustomRunResult {
    /// 返回本轮 assistant 发出的工具调用，供协议适配器测试和结果归一化使用。
    pub(crate) fn tool_calls(&self) -> Vec<&ToolCall> {
        self.choice
            .iter()
            .filter_map(|content| match content {
                AssistantContent::ToolCall(call) => Some(call),
                _ => None,
            })
            .collect()
    }
}
