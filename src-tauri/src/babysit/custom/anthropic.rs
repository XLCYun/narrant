#[cfg(test)]
#[path = "tests/anthropic.rs"]
mod tests;

use super::adapter::{
    CustomAdapterError, CustomProtocolAdapter, CustomRequestPhase, CustomStatusCategory,
    CustomTransportFailure,
};
use super::connection::{CustomConnectionSnapshot, CustomEffort, CustomProtocol};
use super::final_tool::FINAL_OUTPUT_TOOL_NAME;
use super::round::{
    reached_output_limit, CustomRoundRequest, CustomRoundResult, MAX_MODEL_TURNS, MAX_OUTPUT_TOKENS,
};
use crate::babysit::repository_tools::RepositoryTools;
use crate::operation::CancellationSignal;
use rig::agent::OutputMode;
use rig::client::AgentClientExt;
use rig::completion::{AssistantContent, Message, Prompt, ResponseIdentity};
use rig::http_client::{self, HttpClientExt, MultipartForm, Request, Response, StreamingResponse};
use rig::providers::anthropic;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::fmt;
use std::future::Future;
use std::path::{Path, PathBuf};

/// Anthropic 请求守卫：处理可选认证，并阻止取消后的任何 HTTP dispatch。
#[derive(Clone)]
struct AnthropicRequestGuardClient {
    inner: http_client::ReqwestClient,
    omit_authentication: bool,
    cancellation: CancellationSignal,
}

impl Default for AnthropicRequestGuardClient {
    /// 满足 Rig 泛型 client 的构造约束，并始终建立显式 rustls backend。
    fn default() -> Self {
        Self::new(false, CancellationSignal::default())
            .expect("Rig rustls HTTP client should be constructible")
    }
}

impl AnthropicRequestGuardClient {
    /// 创建同时执行可选认证与 dispatch-time 取消检查的 Rig backend。
    fn new(
        omit_authentication: bool,
        cancellation: CancellationSignal,
    ) -> Result<Self, AnthropicAdapterError> {
        let inner = http_client::ReqwestClient::builder()
            .use_rustls_tls()
            .build()
            .map_err(|_| AnthropicAdapterError::Client)?;
        Ok(Self {
            inner,
            omit_authentication,
            cancellation,
        })
    }

    /// 仅在用户没有提供凭据时移除 Anthropic 标准认证头。
    fn prepare<T>(&self, mut request: Request<T>) -> Request<T> {
        if self.omit_authentication {
            request.headers_mut().remove("x-api-key");
        }
        request
    }
}

impl HttpClientExt for AnthropicRequestGuardClient {
    /// 普通请求发送前同时检查取消，并在空凭据时省略认证头。
    fn send<T, U>(
        &self,
        request: Request<T>,
    ) -> impl Future<Output = http_client::Result<Response<http_client::LazyBody<U>>>> + Send + 'static
    where
        T: Into<bytes::Bytes> + Send,
        U: From<bytes::Bytes> + Send + 'static,
    {
        let client = self.inner.clone();
        let request = self.prepare(request);
        let cancelled = self.cancellation.is_cancelled();
        let (parts, body) = request.into_parts();
        let request = Request::from_parts(parts, body.into());
        async move {
            if cancelled {
                Err(http_client::Error::NoHeaders)
            } else {
                client.send(request).await
            }
        }
    }

    /// Multipart 请求发送前执行相同的取消与可选认证守卫。
    fn send_multipart<U>(
        &self,
        request: Request<MultipartForm>,
    ) -> impl Future<Output = http_client::Result<Response<http_client::LazyBody<U>>>> + Send + 'static
    where
        U: From<bytes::Bytes> + Send + 'static,
    {
        let client = self.inner.clone();
        let request = self.prepare(request);
        let cancelled = self.cancellation.is_cancelled();
        async move {
            if cancelled {
                Err(http_client::Error::NoHeaders)
            } else {
                client.send_multipart(request).await
            }
        }
    }

    /// 流式请求建立连接前执行相同的取消与可选认证守卫。
    fn send_streaming<T>(
        &self,
        request: Request<T>,
    ) -> impl Future<Output = http_client::Result<StreamingResponse>> + Send
    where
        T: Into<bytes::Bytes> + Send,
    {
        let client = self.inner.clone();
        let request = self.prepare(request);
        let cancelled = self.cancellation.is_cancelled();
        let (parts, body) = request.into_parts();
        let request = Request::from_parts(parts, body.into());
        async move {
            if cancelled {
                Err(http_client::Error::NoHeaders)
            } else {
                client.send_streaming(request).await
            }
        }
    }
}

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

#[cfg(test)]
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
    #[cfg(test)]
    tool_boundary_cancellation: Option<crate::operation::TestCancellation>,
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
            ))
        })?;
        Ok(Self {
            connection,
            adapter,
            workspace: workspace.as_ref().to_path_buf(),
            #[cfg(test)]
            tool_boundary_cancellation: None,
        })
    }

    /// 测试专用：在 Rig 调用真实 repository tool 时触发指定取消信号。
    #[cfg(test)]
    pub(crate) fn with_tool_boundary_cancellation(
        mut self,
        cancellation: crate::operation::TestCancellation,
    ) -> Self {
        self.tool_boundary_cancellation = Some(cancellation);
        self
    }
}

#[async_trait::async_trait]
impl CustomProtocolAdapter for AnthropicMessagesAdapter {
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
            ))
        })?;
        #[cfg(test)]
        let repository_tools = match self.tool_boundary_cancellation.clone() {
            Some(cancellation) => repository_tools.with_execute_cancellation(cancellation),
            None => repository_tools,
        };
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
                    ))
                }
                AnthropicAdapterError::OutputLimit { limit } => {
                    CustomAdapterError::OutputLimit { limit }
                }
                AnthropicAdapterError::Transport(failure) => CustomAdapterError::Transport(failure),
                AnthropicAdapterError::Protocol => {
                    CustomAdapterError::Transport(CustomTransportFailure::new(
                        self.connection.protocol(),
                        CustomRequestPhase::Response,
                        CustomStatusCategory::InvalidResponse,
                    ))
                }
            })?;
        Ok(CustomRoundResult {
            choice: vec![AssistantContent::tool_call(
                "final-result",
                FINAL_OUTPUT_TOOL_NAME,
                response.output,
            )],
            identity: ResponseIdentity {
                message_id: None,
                response_id: None,
                provider_request_id: None,
            },
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
    /// provider 已经在固定输出预算处截断了回复，不应进入 malformed repair。
    OutputLimit {
        limit: u64,
    },
    Cancelled,
    Client,
    /// Rig prompt 失败已投影为共享的脱敏 transport failure。
    Transport(CustomTransportFailure),
    Protocol,
}

impl fmt::Display for AnthropicAdapterError {
    /// 输出稳定的脱敏错误分类。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutputLimit { limit } => {
                write!(formatter, "Anthropic 输出已达到固定上限 {limit}。")
            }
            Self::Cancelled => formatter.write_str("Anthropic 请求已取消。"),
            Self::Client => formatter.write_str("Anthropic 客户端不可用。"),
            Self::Transport(_) => formatter.write_str("Anthropic 请求失败。"),
            Self::Protocol => formatter.write_str("Anthropic 响应无效。"),
        }
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
        let api_key = self.config.api_key.clone().unwrap_or_default();
        // 显式构造 rustls client，确保 Anthropic-compatible HTTPS endpoint 可用。
        let http_client =
            AnthropicRequestGuardClient::new(api_key.is_empty(), request.cancellation.clone())?;
        let client = anthropic::Client::builder()
            .api_key(api_key.clone())
            .base_url(&self.config.base_url)
            .http_client(http_client)
            .build()
            .map_err(|_| AnthropicAdapterError::Client)?;
        let tools = request.repository_tools.rig_tools(&request.cancellation);
        let cancellation = request.cancellation.clone();
        let prompt = request.prompt;
        let history = request.history;
        let builder = client
            .agent(&self.config.model)
            .max_tokens(MAX_OUTPUT_TOKENS);
        let builder = match self.config.effort.wire_value() {
            Some(effort) => builder.additional_params(json!({
                "thinking": {"type": "adaptive"},
                "output_config": {"effort": effort},
            })),
            None => builder,
        };
        let agent = builder
            .default_max_turns(MAX_MODEL_TURNS)
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
                    if cancellation.is_cancelled() {
                        return Err(AnthropicAdapterError::Cancelled);
                    }
                    break response.map_err(|error| {
                        AnthropicAdapterError::Transport(
                            CustomTransportFailure::from_prompt_error(
                                CustomProtocol::AnthropicMessages,
                                error,
                            ),
                        )
                    })?;
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
        if reached_output_limit(response.completion_calls()) {
            return Err(AnthropicAdapterError::OutputLimit {
                limit: MAX_OUTPUT_TOKENS,
            });
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
