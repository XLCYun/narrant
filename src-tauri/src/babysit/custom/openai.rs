use super::*;
use crate::operation::CancellationSignal;
use rig::client::{AgentClientExt, CompletionClient};
use rig::completion::CompletionModel;
use rig::providers::openai::CompletionsClient;
use std::path::Path;
use std::time::Duration;

/// OpenAI Chat Completions 兼容协议的 Rig adapter。
#[derive(Clone)]
pub(crate) struct OpenAiChatCompletionsAdapter {
    connection: CustomConnectionSnapshot,
    client: CompletionsClient,
}

impl OpenAiChatCompletionsAdapter {
    /// 从冻结连接创建 Rig client；endpoint、URL 拼接和认证均由 Rig 负责。
    pub(crate) fn new(connection: CustomConnectionSnapshot) -> Result<Self, CustomAdapterError> {
        if connection.protocol() != CustomProtocol::OpenaiChatCompletions {
            return Err(CustomAdapterError::UnsupportedProtocol(
                connection.protocol(),
            ));
        }
        let base_url = connection.base_url().to_owned();
        let client: CompletionsClient = CompletionsClient::builder()
            .base_url(&base_url)
            .api_key(connection.api_key().to_owned())
            .build()
            .map_err(|_| {
                CustomAdapterError::Transport(CustomTransportFailure::new(
                    connection.protocol(),
                    CustomRequestPhase::Setup,
                    CustomStatusCategory::InvalidEndpoint,
                    connection.base_url(),
                ))
            })?;
        Ok(Self { connection, client })
    }

    /// 构造使用 Rig OutputMode::Tool 的 agent，供共享 runtime 保持会话与工具循环。
    pub(crate) fn agent_for_workspace(
        &self,
        workspace: impl AsRef<Path>,
        cancellation: &CancellationSignal,
        preamble: impl Into<String>,
    ) -> Result<rig::Agent, CustomAdapterError> {
        let tools = RepositoryTools::new(workspace.as_ref()).map_err(|_| {
            CustomAdapterError::Transport(CustomTransportFailure::new(
                self.connection.protocol(),
                CustomRequestPhase::Setup,
                CustomStatusCategory::InvalidEndpoint,
                self.connection.base_url(),
            ))
        })?;
        let rig_tools = tools.rig_tools(cancellation);
        let preamble = preamble.into();
        Ok(self
            .client
            .agent(self.connection.model().to_owned())
            .preamble(&preamble)
            .additional_params(serde_json::json!({
                "reasoning_effort": self.connection.effort().as_str()
            }))
            .default_max_turns(MAX_MODEL_TURNS)
            .tool(rig_tools.read)
            .tool(rig_tools.glob)
            .tool(rig_tools.grep)
            .output_schema_raw(line_explanation_schema())
            .output_mode(rig::agent::OutputMode::Tool)
            .build())
    }

    /// 返回 adapter 绑定的冻结模型服务连接。
    pub(crate) fn connection_snapshot(&self) -> &CustomConnectionSnapshot {
        &self.connection
    }
}

#[async_trait]
impl CustomProtocolAdapter for OpenAiChatCompletionsAdapter {
    /// 返回本 adapter 冻结的连接快照。
    fn connection(&self) -> &CustomConnectionSnapshot {
        self.connection_snapshot()
    }

    /// 发送一轮 Chat Completions 请求，并在取消时丢弃活动 HTTP future。
    async fn complete(
        &self,
        request: CustomRoundRequest,
        cancellation: &CancellationSignal,
    ) -> Result<CustomRoundResult, CustomAdapterError> {
        if cancellation.is_cancelled() {
            return Err(CustomAdapterError::Cancelled);
        }
        let model = self
            .client
            .completion_model(self.connection.model().to_owned());
        let future = model
            .completion_request(request.prompt().clone())
            .messages(request.history().iter().cloned())
            .tools(request.tools().to_vec())
            .additional_params(serde_json::json!({
                "reasoning_effort": self.connection.effort().as_str()
            }))
            .send();
        tokio::pin!(future);
        loop {
            if cancellation.is_cancelled() {
                return Err(CustomAdapterError::Cancelled);
            }
            tokio::select! {
                result = &mut future => {
                    let response = result.map_err(|error| {
                        CustomAdapterError::Transport(
                            CustomTransportFailure::from_completion_error(
                                self.connection.protocol(),
                                self.connection.base_url(),
                                error,
                            ),
                        )
                    })?;
                    if cancellation.is_cancelled() {
                        return Err(CustomAdapterError::Cancelled);
                    }
                    return Ok(CustomRoundResult::from_response(response));
                }
                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }
    }
}
