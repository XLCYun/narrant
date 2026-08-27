use super::adapter::CustomAdapterError;
use super::final_tool::{line_explanation_schema, FINAL_OUTPUT_TOOL_NAME};
use rig::completion::{
    AssistantContent, FinishReason, Message, ResponseIdentity, ToolDefinition, Usage,
};
use rig::message::ToolCall;
use std::collections::HashSet;

/// Custom 协议适配器共享的单次模型调用上限。
pub(crate) const MAX_MODEL_TURNS: usize = 16;

/// 单次模型请求的 provider-neutral 输入；不含任何协议 wire 类型。
#[derive(Clone, Debug)]
pub(crate) struct CustomRoundRequest {
    prompt: Message,
    history: Vec<Message>,
    turn: usize,
    tools: Vec<ToolDefinition>,
}

impl CustomRoundRequest {
    /// 创建一轮请求，并拒绝超过共享模型调用预算或越权工具集合的输入。
    pub(crate) fn new(
        prompt: Message,
        history: Vec<Message>,
        turn: usize,
        tools: Vec<ToolDefinition>,
    ) -> Result<Self, CustomAdapterError> {
        if turn == 0 || turn > MAX_MODEL_TURNS {
            return Err(CustomAdapterError::TurnLimitExceeded {
                limit: MAX_MODEL_TURNS,
            });
        }
        let names = tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<HashSet<_>>();
        if names.len() != 4
            || !["read", "glob", "grep", FINAL_OUTPUT_TOOL_NAME]
                .iter()
                .all(|name| names.contains(name))
            || tools
                .iter()
                .find(|tool| tool.name == FINAL_OUTPUT_TOOL_NAME)
                .is_none_or(|tool| tool.parameters != line_explanation_schema().to_value())
        {
            return Err(CustomAdapterError::InvalidToolSet);
        }
        Ok(Self {
            prompt,
            history,
            turn,
            tools,
        })
    }
    /// 返回本轮一基 turn 编号。
    pub(crate) const fn turn(&self) -> usize {
        self.turn
    }
    /// 返回本轮 prompt 的 provider-neutral 消息。
    pub(super) fn prompt(&self) -> &Message {
        &self.prompt
    }
    /// 返回同一会话的既有消息历史。
    pub(super) fn history(&self) -> &[Message] {
        &self.history
    }
    /// 返回仅含三项 repository 工具和一个 final tool 的定义。
    pub(crate) fn tools(&self) -> &[ToolDefinition] {
        &self.tools
    }
}

/// 一次模型轮次的 provider-neutral 结果；协议 wire response 在 adapter 内被抹平。
#[derive(Clone, Debug)]
pub(crate) struct CustomRoundResult {
    /// Rig 归一化后的 assistant 内容，工具调用保持可调度信息。
    pub(crate) choice: Vec<AssistantContent>,
    /// provider 报告的本轮用量。
    pub(crate) usage: Usage,
    /// provider 为本轮返回的三类可选 identity。
    pub(crate) identity: ResponseIdentity,
    /// provider 归一化后的结束原因。
    pub(crate) finish_reason: Option<FinishReason>,
    /// Rig Agent 在本轮内部工具循环后返回的完整会话记录。
    pub(crate) transcript: Option<Vec<Message>>,
}

impl CustomRoundResult {
    /// 从 Rig 公共响应投影 provider-neutral round result。
    pub(super) fn from_response(response: rig::completion::CompletionResponse) -> Self {
        let finish_reason = response.finish_reason();
        Self {
            choice: response.choice,
            usage: response.usage,
            identity: ResponseIdentity {
                message_id: response.message_id,
                response_id: response.response_id,
                provider_request_id: response.provider_request_id,
            },
            finish_reason,
            transcript: None,
        }
    }
    /// 返回本轮 assistant 发出的工具调用，供共享 runtime 决定是否 dispatch。
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
