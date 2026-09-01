use super::adapter::{
    CustomAdapterError, CustomProtocolAdapter, CustomRequestPhase, CustomStatusCategory,
    CustomTransportFailure,
};
use super::connection::{CustomConnectionSnapshot, CustomProtocol};
use super::final_tool::{line_explanation_schema, FINAL_OUTPUT_TOOL_NAME};
use super::round::{CustomRoundRequest, CustomRoundResult, MAX_MODEL_TURNS};
use crate::babysit::RepositoryTools;
use crate::operation::CancellationSignal;
use async_trait::async_trait;
use rig::client::AgentClientExt;
use rig::completion::Prompt;
use rig::completion::{AssistantContent, ResponseIdentity};
use rig::http_client::{HttpClientExt, LazyBody, MultipartForm, StreamingResponse};
use rig::providers::openai::CompletionsClient;
use rig::wasm_compat::WasmCompatSend;
use std::future::Future;
use std::future::IntoFuture;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 一次 Rig prompt 的内部失败，保留取消、构造与 provider 错误边界。
enum OpenAiPromptAttemptError {
    /// 请求在模型返回前被取消。
    Cancelled,
    /// agent 或 Workspace 工具无法构造。
    Adapter(CustomAdapterError),
    /// provider 或 Rig prompt loop 返回失败。
    Prompt(rig::completion::PromptError),
}

/// OpenAI Chat Completions 兼容协议的 Rig adapter。
#[derive(Clone)]
pub(crate) struct OpenAiChatCompletionsAdapter {
    connection: CustomConnectionSnapshot,
    client: CompletionsClient<OpenAiRequestGuardClient>,
    http_client: OpenAiRequestGuardClient,
    workspace: PathBuf,
    #[cfg(test)]
    tool_boundary_cancellation: Option<crate::operation::TestCancellation>,
}

/// OpenAI 请求守卫：处理可选认证，并阻止取消后的任何 HTTP dispatch。
#[derive(Clone, Debug)]
struct OpenAiRequestGuardClient {
    inner: rig::http_client::ReqwestClient,
    omit_authorization: bool,
    cancellation: Arc<Mutex<Option<CancellationSignal>>>,
}

impl Default for OpenAiRequestGuardClient {
    /// 满足 Rig 泛型 client 的构造约束，并始终建立显式 rustls backend。
    fn default() -> Self {
        Self::new(false).expect("Rig rustls HTTP client should be constructible")
    }
}

impl OpenAiRequestGuardClient {
    /// 创建同时执行可选认证与 dispatch-time 取消检查的 Rig backend。
    fn new(omit_authorization: bool) -> Result<Self, ()> {
        let inner = rig::http_client::ReqwestClient::builder()
            .use_rustls_tls()
            .build()
            .map_err(|_| ())?;
        Ok(Self {
            inner,
            omit_authorization,
            cancellation: Arc::new(Mutex::new(None)),
        })
    }

    /// 将当前 provider round 的共享取消信号绑定到每一次后续 HTTP dispatch。
    fn bind_cancellation(&self, cancellation: &CancellationSignal) {
        if let Ok(mut current) = self.cancellation.lock() {
            *current = Some(cancellation.clone());
        }
    }

    /// 工具或 repair 边界取消后，Rig 不得再发下一次 HTTP 请求。
    fn dispatch_cancelled(&self) -> bool {
        self.cancellation
            .lock()
            .ok()
            .and_then(|current| current.clone())
            .is_some_and(|cancellation| cancellation.is_cancelled())
    }

    /// 空 key 不能变成 `Authorization: Bearer `；非空 header 保持 Rig 原样。
    fn strip_empty_authorization<T>(&self, request: &mut http::Request<T>) {
        if self.omit_authorization {
            request.headers_mut().remove(http::header::AUTHORIZATION);
        }
    }
}

impl HttpClientExt for OpenAiRequestGuardClient {
    /// 普通请求发送前同时检查取消，并在无 credential 时移除空认证头。
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
        let client = self.inner.clone();
        let cancelled = self.dispatch_cancelled();
        let (parts, body) = request.into_parts();
        let request = http::Request::from_parts(parts, body.into());
        async move {
            if cancelled {
                Err(rig::http_client::Error::NoHeaders)
            } else {
                client.send(request).await
            }
        }
    }

    /// Multipart 请求发送前执行相同的取消与可选认证守卫。
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
        let client = self.inner.clone();
        let cancelled = self.dispatch_cancelled();
        async move {
            if cancelled {
                Err(rig::http_client::Error::NoHeaders)
            } else {
                client.send_multipart(request).await
            }
        }
    }

    /// 流式请求建立连接前执行相同的取消与可选认证守卫。
    fn send_streaming<T>(
        &self,
        mut request: http::Request<T>,
    ) -> impl Future<Output = rig::http_client::Result<StreamingResponse>> + WasmCompatSend
    where
        T: Into<bytes::Bytes> + WasmCompatSend,
    {
        self.strip_empty_authorization(&mut request);
        let client = self.inner.clone();
        let cancelled = self.dispatch_cancelled();
        let (parts, body) = request.into_parts();
        let request = http::Request::from_parts(parts, body.into());
        async move {
            if cancelled {
                Err(rig::http_client::Error::NoHeaders)
            } else {
                client.send_streaming(request).await
            }
        }
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
        // 显式构造 rustls client，让 Custom 的 HTTPS 能力不依赖 Rig 的隐式默认 feature。
        let http_client = OpenAiRequestGuardClient::new(empty_key).map_err(|_| {
            CustomAdapterError::Transport(CustomTransportFailure::new(
                connection.protocol(),
                CustomRequestPhase::Setup,
                CustomStatusCategory::Network,
            ))
        })?;
        let client = CompletionsClient::builder()
            .base_url(&base_url)
            .api_key(connection.api_key().to_owned())
            .http_client(http_client.clone())
            .build()
            .map_err(|_| {
                CustomAdapterError::Transport(CustomTransportFailure::new(
                    connection.protocol(),
                    CustomRequestPhase::Setup,
                    CustomStatusCategory::InvalidEndpoint,
                ))
            })?;
        Ok(Self {
            connection,
            client,
            http_client,
            workspace: workspace.into(),
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

    /// 构造使用 Rig OutputMode::Tool 的 agent，供共享 runtime 保持会话与工具循环。
    fn agent_for_workspace(
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
            ))
        })?;
        #[cfg(test)]
        let tools = match self.tool_boundary_cancellation.clone() {
            Some(cancellation) => tools.with_execute_cancellation(cancellation),
            None => tools,
        };
        let rig_tools = tools.rig_tools(cancellation);
        let preamble = preamble.into();
        let builder = self
            .client
            .agent(self.connection.model().to_owned())
            .preamble(&preamble);
        let builder = match self.connection.effort().wire_value() {
            Some(effort) => builder.additional_params(serde_json::json!({
                "reasoning_effort": effort
            })),
            None => builder,
        };
        Ok(builder
            .default_max_turns(MAX_MODEL_TURNS)
            .tool(rig_tools.read)
            .tool(rig_tools.glob)
            .tool(rig_tools.grep)
            .output_schema_raw(line_explanation_schema())
            .output_mode(rig::agent::OutputMode::Tool)
            .build())
    }

    /// 运行一次完整 Rig agent loop，并把取消与 provider 错误保留为可判定的内部结果。
    async fn run_prompt_attempt(
        &self,
        request: &CustomRoundRequest,
        cancellation: &CancellationSignal,
    ) -> Result<rig::agent::PromptResponse, OpenAiPromptAttemptError> {
        let agent = self
            .agent_for_workspace(
                &self.workspace,
                cancellation,
                "Only use the registered read-only repository tools. Submit the final answer through the schema-bearing final_result tool.",
            )
            .map_err(OpenAiPromptAttemptError::Adapter)?;
        let future = agent
            .prompt(request.prompt().clone())
            .history(request.history().to_vec())
            .max_turns(MAX_MODEL_TURNS)
            .extended_details()
            .into_future();
        tokio::pin!(future);
        loop {
            if cancellation.is_cancelled() {
                return Err(OpenAiPromptAttemptError::Cancelled);
            }
            tokio::select! {
                result = &mut future => {
                    if cancellation.is_cancelled() {
                        return Err(OpenAiPromptAttemptError::Cancelled);
                    }
                    return result.map_err(OpenAiPromptAttemptError::Prompt);
                }
                _ = tokio::time::sleep(Duration::from_millis(10)) => {}
            }
        }
    }

    /// 将内部尝试错误投影成 adapter 的稳定脱敏错误。
    fn map_attempt_error(&self, error: OpenAiPromptAttemptError) -> CustomAdapterError {
        match error {
            OpenAiPromptAttemptError::Cancelled => CustomAdapterError::Cancelled,
            OpenAiPromptAttemptError::Adapter(error) => error,
            OpenAiPromptAttemptError::Prompt(error) => CustomAdapterError::Transport(
                CustomTransportFailure::from_prompt_error(self.connection.protocol(), error),
            ),
        }
    }
}

#[async_trait]
impl CustomProtocolAdapter for OpenAiChatCompletionsAdapter {
    /// 发送一轮 Chat Completions 请求，并在取消时丢弃活动 HTTP future。
    async fn complete(
        &self,
        request: CustomRoundRequest,
        cancellation: &CancellationSignal,
    ) -> Result<CustomRoundResult, CustomAdapterError> {
        if cancellation.is_cancelled() {
            return Err(CustomAdapterError::Cancelled);
        }
        self.http_client.bind_cancellation(cancellation);
        let response = self
            .run_prompt_attempt(&request, cancellation)
            .await
            .map_err(|error| self.map_attempt_error(error))?;
        if cancellation.is_cancelled() {
            return Err(CustomAdapterError::Cancelled);
        }
        let arguments = serde_json::from_str(&response.output).unwrap_or(serde_json::Value::Null);
        Ok(CustomRoundResult {
            choice: vec![AssistantContent::tool_call(
                "final-result",
                FINAL_OUTPUT_TOOL_NAME,
                arguments,
            )],
            identity: ResponseIdentity {
                message_id: None,
                response_id: None,
                provider_request_id: None,
            },
            transcript: response.messages,
        })
    }
}
