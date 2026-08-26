use super::*;
use crate::operation::CancellationSignal;
use rig::client::AgentClientExt;
use rig::completion::Prompt;
use rig::completion::{AssistantContent, FinishReason, ResponseIdentity};
use rig::http_client::{HttpClientExt, LazyBody, MultipartForm, StreamingResponse};
use rig::providers::openai::CompletionsClient;
use rig::wasm_compat::WasmCompatSend;
use std::future::Future;
use std::future::IntoFuture;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// OpenAI Chat Completions 兼容协议的 Rig adapter。
#[derive(Clone)]
pub(crate) struct OpenAiChatCompletionsAdapter {
    connection: CustomConnectionSnapshot,
    client: CompletionsClient<OptionalAuthClient>,
    workspace: PathBuf,
}

/// 复用 Rig 默认 HTTP backend，并在未配置 credential 时移除空 Bearer header。
#[derive(Clone, Debug, Default)]
struct OptionalAuthClient {
    inner: rig::http_client::ReqwestClient,
    omit_authorization: bool,
}

impl OptionalAuthClient {
    /// 创建只处理“无 credential”语义的透明 backend。
    fn new(omit_authorization: bool) -> Self {
        Self {
            inner: rig::http_client::ReqwestClient::new(),
            omit_authorization,
        }
    }

    /// 空 key 不能变成 `Authorization: Bearer `；非空 header 保持 Rig 原样。
    fn strip_empty_authorization<T>(&self, request: &mut http::Request<T>) {
        if self.omit_authorization {
            request.headers_mut().remove(http::header::AUTHORIZATION);
        }
    }
}

impl HttpClientExt for OptionalAuthClient {
    fn send<T, U>(
        &self,
        mut request: http::Request<T>,
    ) -> impl Future<Output = rig::http_client::Result<http::Response<LazyBody<U>>>>
           + WasmCompatSend
           + 'static
    where
        T: Into<bytes::Bytes> + WasmCompatSend,
        U: From<bytes::Bytes> + WasmCompatSend + 'static,
    {
        self.strip_empty_authorization(&mut request);
        self.inner.send(request)
    }

    fn send_multipart<U>(
        &self,
        mut request: http::Request<MultipartForm>,
    ) -> impl Future<Output = rig::http_client::Result<http::Response<LazyBody<U>>>>
           + WasmCompatSend
           + 'static
    where
        U: From<bytes::Bytes> + WasmCompatSend + 'static,
    {
        self.strip_empty_authorization(&mut request);
        self.inner.send_multipart(request)
    }

    fn send_streaming<T>(
        &self,
        mut request: http::Request<T>,
    ) -> impl Future<Output = rig::http_client::Result<StreamingResponse>> + WasmCompatSend
    where
        T: Into<bytes::Bytes> + WasmCompatSend,
    {
        self.strip_empty_authorization(&mut request);
        self.inner.send_streaming(request)
    }
}

impl OpenAiChatCompletionsAdapter {
    /// 从冻结连接创建 Rig client；endpoint、URL 拼接和认证均由 Rig 负责。
    pub(crate) fn new(
        connection: CustomConnectionSnapshot,
        workspace: impl Into<PathBuf>,
    ) -> Result<Self, CustomAdapterError> {
        if connection.protocol() != CustomProtocol::OpenaiChatCompletions {
            return Err(CustomAdapterError::UnsupportedProtocol(
                connection.protocol(),
            ));
        }
        let base_url = connection.base_url().to_owned();
        let empty_key = connection.api_key().is_empty();
        let client = CompletionsClient::builder()
            .base_url(&base_url)
            .api_key(connection.api_key().to_owned())
            .http_client(OptionalAuthClient::new(empty_key))
            .build()
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
            client,
            workspace: workspace.into(),
        })
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
}

#[async_trait]
impl CustomProtocolAdapter for OpenAiChatCompletionsAdapter {
    /// 返回本 adapter 冻结的连接快照。
    fn connection(&self) -> &CustomConnectionSnapshot {
        &self.connection
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
        let agent = self.agent_for_workspace(
            &self.workspace,
            cancellation,
            "Only use the registered read-only repository tools. Submit the final answer through the schema-bearing final_result tool.",
        )?;
        let future = agent
            .prompt(request.prompt().clone())
            .history(request.history().to_vec())
            .max_turns(MAX_MODEL_TURNS)
            .extended_details()
            .into_future();
        tokio::pin!(future);
        loop {
            if cancellation.is_cancelled() {
                return Err(CustomAdapterError::Cancelled);
            }
            tokio::select! {
                result = &mut future => {
                    let response = result.map_err(|_| {
                        CustomAdapterError::Transport(
                            CustomTransportFailure::new(
                                self.connection.protocol(),
                                CustomRequestPhase::Model,
                                CustomStatusCategory::Unknown,
                                self.connection.base_url(),
                            ),
                        )
                    })?;
                    if cancellation.is_cancelled() {
                        return Err(CustomAdapterError::Cancelled);
                    }
                    let arguments = serde_json::from_str(&response.output)
                        .unwrap_or(serde_json::Value::Null);
                    return Ok(CustomRoundResult {
                        choice: vec![AssistantContent::tool_call(
                            "final-result",
                            FINAL_OUTPUT_TOOL_NAME,
                            arguments,
                        )],
                        usage: response.usage,
                        identity: ResponseIdentity {
                            message_id: None,
                            response_id: None,
                            provider_request_id: None,
                        },
                        finish_reason: Some(FinishReason::ToolCalls),
                        transcript: response.messages,
                    });
                }
                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }
    }
}
