#[cfg(test)]
#[path = "tests/anthropic.rs"]
mod tests;

use super::{
    AssistantContent, CustomAdapterError, CustomConnectionSnapshot, CustomEffort, CustomProtocol,
    CustomProtocolAdapter, CustomRequestPhase, CustomRoundRequest, CustomRoundResult,
    CustomStatusCategory, CustomTransportFailure, FinishReason, ResponseIdentity,
};
use crate::babysit::repository_tools::RepositoryTools;
use crate::operation::CancellationSignal;
use rig::agent::OutputMode;
use rig::client::AgentClientExt;
use rig::completion::{Message, Prompt};
use rig::providers::anthropic;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;
use std::path::{Path, PathBuf};

/// Anthropic Messages adapter 的冻结连接参数。
#[derive(Clone, Debug)]
pub(crate) struct AnthropicAdapterConfig {
    base_url: String,
    api_key: Option<String>,
    model: String,
    effort: CustomEffort,
}

impl AnthropicAdapterConfig {
    /// 创建一次请求使用的 Anthropic 连接快照。
    pub(crate) fn new(
        base_url: impl Into<String>,
        api_key: Option<String>,
        model: impl Into<String>,
        effort: impl Into<CustomEffort>,
    ) -> Self {
        Self {
            base_url: base_url.into(),
            api_key,
            model: model.into(),
            effort: effort.into(),
        }
    }
}

/// Anthropic adapter 一次 completion 所需的协议中立输入。
pub(crate) struct AnthropicAdapterRequest {
    prompt: Message,
    history: Vec<Message>,
    repository_tools: RepositoryTools,
    cancellation: CancellationSignal,
}

impl AnthropicAdapterRequest {
    /// 绑定 prompt、既有对话与请求级 Workspace 工具。
    pub(crate) fn new(
        prompt: impl Into<String>,
        history: Vec<Message>,
        repository_tools: RepositoryTools,
        cancellation: CancellationSignal,
    ) -> Self {
        Self {
            prompt: Message::user(prompt.into()),
            history,
            repository_tools,
            cancellation,
        }
    }
}

/// 将 Anthropic Messages wire adapter 接入共享 Custom runtime 的窄封装。
pub(crate) struct AnthropicMessagesAdapter {
    connection: CustomConnectionSnapshot,
    adapter: AnthropicAdapter,
    workspace: PathBuf,
}

impl AnthropicMessagesAdapter {
    /// 从冻结连接创建 adapter；URL 与认证 header 继续由 Rig 处理。
    pub(crate) fn new(
        connection: CustomConnectionSnapshot,
        workspace: impl AsRef<Path>,
    ) -> Result<Self, CustomAdapterError> {
        if connection.protocol() != CustomProtocol::AnthropicMessages {
            return Err(CustomAdapterError::UnsupportedProtocol(
                connection.protocol(),
            ));
        }
        let api_key = (!connection.api_key().is_empty()).then(|| connection.api_key().to_owned());
        let adapter = AnthropicAdapter::new(AnthropicAdapterConfig::new(
            connection.base_url(),
            api_key,
            connection.model(),
            connection.effort(),
        ))
        .map_err(|_| {
            CustomAdapterError::Transport(CustomTransportFailure::new(
                connection.protocol(),
                CustomRequestPhase::Setup,
                CustomStatusCategory::InvalidEndpoint,
                connection.base_url(),
            ))
        })?;
        Ok(Self {
            connection,
            adapter,
            workspace: workspace.as_ref().to_path_buf(),
        })
    }
}

#[async_trait::async_trait]
impl CustomProtocolAdapter for AnthropicMessagesAdapter {
    /// 返回本 adapter 冻结的连接快照。
    fn connection(&self) -> &CustomConnectionSnapshot {
        &self.connection
    }

    /// 将共享 round 请求转换为 Anthropic Messages 对话并归一化 final tool 结果。
    async fn complete(
        &self,
        request: CustomRoundRequest,
        cancellation: &CancellationSignal,
    ) -> Result<CustomRoundResult, CustomAdapterError> {
        let repository_tools = RepositoryTools::new(&self.workspace).map_err(|_| {
            CustomAdapterError::Transport(CustomTransportFailure::new(
                self.connection.protocol(),
                CustomRequestPhase::Setup,
                CustomStatusCategory::InvalidEndpoint,
                self.connection.base_url(),
            ))
        })?;
        let response = self
            .adapter
            .complete(AnthropicAdapterRequest {
                prompt: request.prompt().clone(),
                history: request.history().to_vec(),
                repository_tools,
                cancellation: cancellation.clone(),
            })
            .await
            .map_err(|error| match error {
                AnthropicAdapterError::Cancelled => CustomAdapterError::Cancelled,
                AnthropicAdapterError::Client => {
                    CustomAdapterError::Transport(CustomTransportFailure::new(
                        self.connection.protocol(),
                        CustomRequestPhase::Model,
                        CustomStatusCategory::Network,
                        self.connection.base_url(),
                    ))
                }
                AnthropicAdapterError::Protocol => {
                    CustomAdapterError::Transport(CustomTransportFailure::new(
                        self.connection.protocol(),
                        CustomRequestPhase::Response,
                        CustomStatusCategory::InvalidResponse,
                        self.connection.base_url(),
                    ))
                }
            })?;
        Ok(CustomRoundResult {
            choice: vec![AssistantContent::tool_call(
                "final-result",
                super::FINAL_OUTPUT_TOOL_NAME,
                response.output,
            )],
            usage: Default::default(),
            identity: ResponseIdentity {
                message_id: None,
                response_id: None,
                provider_request_id: None,
            },
            finish_reason: Some(FinishReason::ToolCalls),
            transcript: Some(response.conversation),
        })
    }
}

/// Anthropic final-output tool 返回的未验证 JSON。
#[derive(Debug)]
pub(crate) struct AnthropicAdapterResponse {
    pub(crate) output: Value,
    /// 包含工具调用与结果的完整对话，可直接用于下一次 repair。
    pub(crate) conversation: Vec<Message>,
}

/// 不携带 endpoint、credential 或模型内容的 Anthropic adapter 错误。
#[derive(Debug)]
pub(crate) enum AnthropicAdapterError {
    Cancelled,
    Client,
    Protocol,
}

impl fmt::Display for AnthropicAdapterError {
    /// 输出稳定的脱敏错误分类。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Cancelled => "Anthropic 请求已取消。",
            Self::Client => "Anthropic 客户端不可用。",
            Self::Protocol => "Anthropic 响应无效。",
        })
    }
}

impl std::error::Error for AnthropicAdapterError {}

/// 仅负责 Anthropic Messages wire shape 的 Custom 协议适配器。
pub(crate) struct AnthropicAdapter {
    config: AnthropicAdapterConfig,
}

impl AnthropicAdapter {
    /// 保存不可变连接参数；endpoint 解析仍由 Rig 负责。
    pub(crate) fn new(config: AnthropicAdapterConfig) -> Result<Self, AnthropicAdapterError> {
        Ok(Self { config })
    }

    /// 执行一次允许仓库只读工具与 final-output tool 的 Messages 对话。
    pub(crate) async fn complete(
        &self,
        request: AnthropicAdapterRequest,
    ) -> Result<AnthropicAdapterResponse, AnthropicAdapterError> {
        if request.cancellation.is_cancelled() {
            return Err(AnthropicAdapterError::Cancelled);
        }
        let client = anthropic::Client::builder()
            .api_key(self.config.api_key.clone().unwrap_or_default())
            .base_url(&self.config.base_url)
            .build()
            .map_err(|_| AnthropicAdapterError::Client)?;
        let tools = request.repository_tools.rig_tools(&request.cancellation);
        let cancellation = request.cancellation.clone();
        let prompt = request.prompt;
        let history = request.history;
        let agent = client
            .agent(&self.config.model)
            .additional_params(json!({
                "thinking": {"type": "adaptive"},
                "output_config": {"effort": self.config.effort.as_str()},
            }))
            .max_tokens(8_192)
            .default_max_turns(16)
            .output_schema::<LineExplanationOutput>()
            .output_mode(OutputMode::Tool)
            .tool(tools.read)
            .tool(tools.glob)
            .tool(tools.grep)
            .build();
        let completion = async move {
            agent
                .prompt(prompt)
                .history(history)
                .extended_details()
                .await
        };
        tokio::pin!(completion);
        let response = loop {
            tokio::select! {
                response = &mut completion => {
                    break response.map_err(|_| AnthropicAdapterError::Protocol)?;
                }
                _ = tokio::time::sleep(std::time::Duration::from_millis(10)) => {
                    if cancellation.is_cancelled() {
                        return Err(AnthropicAdapterError::Cancelled);
                    }
                }
            }
        };
        if cancellation.is_cancelled() {
            return Err(AnthropicAdapterError::Cancelled);
        }
        let output =
            serde_json::from_str(&response.output).map_err(|_| AnthropicAdapterError::Protocol)?;
        Ok(AnthropicAdapterResponse {
            output,
            conversation: response.messages.unwrap_or_default(),
        })
    }
}

/// final_result 工具的 schema；严格业务校验仍由共享 validation seam 完成。
#[derive(Debug, Deserialize, Serialize, rig::schemars::JsonSchema)]
struct LineExplanationOutput {
    before: std::collections::BTreeMap<String, String>,
    after: std::collections::BTreeMap<String, String>,
}
