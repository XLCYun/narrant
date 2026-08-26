use super::*;
use rig::completion::{
    AssistantContent, FinishReason, Message, ResponseIdentity, ToolDefinition, Usage,
};
use rig::message::ToolCall;
use secrecy::{ExposeSecret, SecretString};
use std::collections::HashSet;
use std::fmt;

/// Custom provider 在 registry 中使用的稳定 identity。
pub(crate) const PROVIDER_ID: &str = "custom";

/// 已冻结连接的 Custom provider；请求编排委托给共享 runtime。
pub(crate) struct CustomProvider {
    runtime: Arc<CustomRuntime>,
    adapter: Arc<dyn CustomProtocolAdapter>,
    process_generation: String,
}

impl CustomProvider {
    /// 将单一冻结 adapter 接入共享 runtime，不在构造阶段发出网络请求。
    fn new(
        runtime: Arc<CustomRuntime>,
        adapter: Arc<dyn CustomProtocolAdapter>,
        process_generation: String,
    ) -> Self {
        Self {
            runtime,
            adapter,
            process_generation,
        }
    }
}

#[async_trait]
impl BabysitAgentProvider for CustomProvider {
    /// 冻结连接的非 secret identity 参与缓存隔离。
    fn process_generation(&self) -> String {
        self.process_generation.clone()
    }

    /// 通过共享 runtime 生成完整 before/after 结果。
    async fn generate(
        &self,
        request: BabysitProviderRequest,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        self.runtime
            .generate(self.adapter.clone(), request, cancellation)
            .await
    }

    /// 在同一内存会话中执行 validation 指向的 repair。
    async fn repair(
        &self,
        request: BabysitProviderRequest,
        previous: BabysitFileExplanations,
        failures: Vec<ValidationFailure>,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        self.runtime
            .repair(request, previous, failures, cancellation)
            .await
    }

    /// 取消、失败或完成后丢弃该分析在 runtime 中的单内存会话。
    async fn abandon(&self, request: &BabysitProviderRequest) {
        self.runtime.abandon(request.analysis_run_id);
    }

    /// 应用退出时清空 runtime 中尚未结束的 Custom 会话。
    async fn shutdown(&self) {
        self.runtime.shutdown();
    }
}

/// Custom capability bundle 负责读取完整连接并为每次分析冻结 adapter。
pub(crate) struct CustomCapabilityBundle {
    settings: crate::application_settings::ApplicationSettingsApplication,
    runtime: Arc<CustomRuntime>,
}

impl CustomCapabilityBundle {
    /// 创建不发起网络请求的 Custom capability，并共享进程内 runtime。
    pub(crate) fn new(
        settings: crate::application_settings::ApplicationSettingsApplication,
    ) -> Self {
        Self {
            settings,
            runtime: Arc::new(CustomRuntime::new()),
        }
    }
}

#[async_trait]
impl BabysitProviderCapabilities for CustomCapabilityBundle {
    /// 从同一 settings 快照投影配置状态，并冻结模型与 effort。
    async fn prepare_analysis(
        &self,
        workspace_path: &Path,
        preference: Option<&crate::application_settings::BabysitProviderPreference<'_>>,
    ) -> Result<PreparedBabysitAnalysis, BabysitError> {
        let Some(crate::application_settings::BabysitProviderPreference::Custom(connection)) =
            preference
        else {
            return Err(BabysitError::provider_setup(
                PROVIDER_ID,
                BabysitProviderSetupFailure::NotFound,
            ));
        };
        if !connection.is_configured() {
            return Err(BabysitError::provider_setup(
                PROVIDER_ID,
                BabysitProviderSetupFailure::NotFound,
            ));
        }
        let (full_connection, credential_generation) = self
            .settings
            .load_custom_connection_with_generation()
            .map_err(|_| BabysitError::Provider {
                detail: "Custom provider 连接不可用。".to_owned(),
            })?;
        let snapshot = CustomConnectionSnapshot::from_connection(
            full_connection,
            credential_generation.to_string(),
        )
        .map_err(|error| match error {
            CustomAdapterError::UnsupportedProtocol(_) => {
                BabysitError::provider_setup(PROVIDER_ID, BabysitProviderSetupFailure::Incompatible)
            }
            other => BabysitError::Provider {
                detail: other.to_string(),
            },
        })?;
        let process_generation = snapshot.cache_identity();
        let adapter: Arc<dyn CustomProtocolAdapter> = match snapshot.protocol() {
            CustomProtocol::OpenaiChatCompletions => Arc::new(
                OpenAiChatCompletionsAdapter::new(snapshot.clone(), workspace_path).map_err(
                    |error| BabysitError::Provider {
                        detail: error.to_string(),
                    },
                )?,
            ),
            CustomProtocol::AnthropicMessages => Arc::new(
                AnthropicMessagesAdapter::new(snapshot.clone(), workspace_path).map_err(
                    |error| BabysitError::Provider {
                        detail: error.to_string(),
                    },
                )?,
            ),
        };
        let provider = Arc::new(CustomProvider::new(
            self.runtime.clone(),
            adapter,
            process_generation,
        ));
        Ok(PreparedBabysitAnalysis {
            provider,
            resolved: BabysitResolvedModel {
                model: snapshot.model().to_owned(),
                reasoning_effort: snapshot.effort().as_str().to_owned(),
            },
            process_generation: snapshot.cache_identity(),
            _lease: None,
        })
    }

    /// Custom 不依赖模型目录；设置页应使用完整连接编辑器。
    async fn model_catalog(&self) -> Result<BabysitProviderModelCatalog, BabysitError> {
        Err(BabysitError::Provider {
            detail: "Custom provider 不支持模型目录。".to_owned(),
        })
    }

    /// Custom 不接受 executable 路径，避免误把 Installed 设置写入 Custom。
    async fn validate_executable(&self, _path: &Path) -> Result<std::path::PathBuf, BabysitError> {
        Err(BabysitError::provider_setup(
            PROVIDER_ID,
            BabysitProviderSetupFailure::NotFound,
        ))
    }

    /// 清空共享 runtime，不持有网络线程或子进程。
    async fn shutdown(&self) {
        self.runtime.shutdown();
    }
}

impl crate::application_settings::CustomProviderEffort {
    /// 将设置中的 effort 投影为 provider-neutral cache identity 文本。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

mod anthropic;
mod final_tool;
mod openai;
mod runtime;

pub(crate) use anthropic::AnthropicMessagesAdapter;
#[cfg(test)]
pub(crate) use anthropic::{
    AnthropicAdapter, AnthropicAdapterConfig, AnthropicAdapterError, AnthropicAdapterRequest,
};
pub(crate) use final_tool::{
    custom_tool_definitions, line_explanation_schema, FINAL_OUTPUT_TOOL_NAME,
};
pub(crate) use openai::OpenAiChatCompletionsAdapter;
pub(crate) use runtime::CustomRuntime;

/// Custom 协议适配器共享的单次模型调用上限。
pub(crate) const MAX_MODEL_TURNS: usize = 16;
/// Custom 运行允许的最大修复轮数；共享 runtime 负责具体 repair 编排。
pub(crate) const MAX_REPAIR_ATTEMPTS: u8 = 3;

/// Custom 连接显式选择的模型服务协议；不从 URL 推断协议。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CustomProtocol {
    /// OpenAI Chat Completions 兼容协议。
    OpenaiChatCompletions,
    /// Anthropic Messages 兼容协议的预留枚举值。
    AnthropicMessages,
}

/// Custom 请求使用的 provider-neutral effort 值。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CustomEffort {
    /// 低工作量。
    Low,
    /// 中等工作量。
    Medium,
    /// 高工作量。
    High,
}

impl CustomEffort {
    /// 返回各协议共享的稳定 effort 文本。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

impl From<crate::application_settings::CustomProviderProtocol> for CustomProtocol {
    /// 将 settings 协议投影到不携带 provider wire shape 的适配器 seam。
    fn from(value: crate::application_settings::CustomProviderProtocol) -> Self {
        match value {
            crate::application_settings::CustomProviderProtocol::OpenaiChatCompletions => {
                Self::OpenaiChatCompletions
            }
            crate::application_settings::CustomProviderProtocol::AnthropicMessages => {
                Self::AnthropicMessages
            }
        }
    }
}

impl From<crate::application_settings::CustomProviderEffort> for CustomEffort {
    /// 将 settings effort 投影为协议中立值。
    fn from(value: crate::application_settings::CustomProviderEffort) -> Self {
        match value {
            crate::application_settings::CustomProviderEffort::Low => Self::Low,
            crate::application_settings::CustomProviderEffort::Medium => Self::Medium,
            crate::application_settings::CustomProviderEffort::High => Self::High,
        }
    }
}

/// 一次分析开始时冻结的完整 Custom 连接；secret 只存在于内存。
#[derive(Clone)]
pub(crate) struct CustomConnectionSnapshot {
    protocol: CustomProtocol,
    base_url: String,
    api_key: SecretString,
    model: String,
    effort: CustomEffort,
    credential_generation: String,
}

impl fmt::Debug for CustomConnectionSnapshot {
    /// 调试输出只显示凭据存在性，不显示 URL 中可能携带的敏感内容或 key。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CustomConnectionSnapshot")
            .field("protocol", &self.protocol)
            .field("origin_hash", &endpoint_origin_hash(&self.base_url))
            .field(
                "api_key",
                &(!self.api_key.expose_secret().is_empty()).then_some("<redacted>"),
            )
            .field("model", &self.model)
            .field("effort", &self.effort)
            .field("credential_generation", &self.credential_generation)
            .finish()
    }
}

impl CustomConnectionSnapshot {
    /// 校验并冻结 settings、secret 与 credential generation，保留用户输入的原值。
    pub(crate) fn new(
        protocol: CustomProtocol,
        base_url: impl Into<String>,
        api_key: impl Into<String>,
        model: impl Into<String>,
        effort: CustomEffort,
        credential_generation: impl Into<String>,
    ) -> Result<Self, CustomAdapterError> {
        let base_url = base_url.into();
        let model = model.into();
        if base_url.trim().is_empty() || model.trim().is_empty() {
            return Err(CustomAdapterError::InvalidConnection);
        }
        Ok(Self {
            protocol,
            base_url,
            api_key: SecretString::from(api_key.into()),
            model,
            effort,
            credential_generation: credential_generation.into(),
        })
    }

    /// 从完整 settings connection 构造一次不可变 adapter 输入快照。
    pub(crate) fn from_connection(
        connection: crate::application_settings::CustomProviderConnection,
        credential_generation: impl Into<String>,
    ) -> Result<Self, CustomAdapterError> {
        Self::new(
            connection.protocol.into(),
            connection.base_url,
            connection.api_key,
            connection.model,
            connection.effort.into(),
            credential_generation,
        )
    }

    /// 返回显式选择的协议。
    pub(crate) const fn protocol(&self) -> CustomProtocol {
        self.protocol
    }

    /// 返回保持原样的 base URL，供 Rig 负责 endpoint 拼接。
    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    /// 返回保持原样的模型标识。
    pub(crate) fn model(&self) -> &str {
        &self.model
    }

    /// 返回本轮映射使用的 effort。
    pub(crate) const fn effort(&self) -> CustomEffort {
        self.effort
    }

    /// 返回不含 secret 的凭据 generation，用于冻结运行和缓存隔离。
    pub(crate) fn credential_generation(&self) -> &str {
        &self.credential_generation
    }

    /// 返回只由非 secret 连接字段组成的稳定缓存 identity。
    pub(crate) fn cache_identity(&self) -> String {
        use std::hash::{Hash, Hasher};

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.protocol.hash(&mut hasher);
        self.base_url.hash(&mut hasher);
        self.model.hash(&mut hasher);
        self.effort.hash(&mut hasher);
        self.credential_generation.hash(&mut hasher);
        format!("custom-config-{:016x}", hasher.finish())
    }

    /// 仅供 adapter 构造 Rig client 时读取 secret。
    pub(super) fn api_key(&self) -> &str {
        self.api_key.expose_secret()
    }
}

/// 单次模型请求的 provider-neutral 输入；不含任何 OpenAI/Anthropic wire 类型。
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

/// 适配器 transport failure 可携带的脱敏状态类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CustomStatusCategory {
    /// 无 HTTP 响应的网络或连接失败。
    Network,
    /// 端点地址无法构造。
    InvalidEndpoint,
    /// 认证失败。
    Unauthorized,
    /// 其他 4xx 客户端失败。
    Client,
    /// 服务端失败。
    Server,
    /// 限流。
    RateLimited,
    /// 响应无法解析。
    InvalidResponse,
    /// 无法进一步分类的 provider failure。
    Unknown,
}

/// 只包含协议、阶段、状态类别与 endpoint origin hash 的脱敏传输失败。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CustomTransportFailure {
    protocol: CustomProtocol,
    phase: CustomRequestPhase,
    status: CustomStatusCategory,
    origin_hash: String,
}

impl CustomTransportFailure {
    /// 构造仅包含允许出现在诊断中的字段的失败值。
    pub(super) fn new(
        protocol: CustomProtocol,
        phase: CustomRequestPhase,
        status: CustomStatusCategory,
        base_url: &str,
    ) -> Self {
        Self {
            protocol,
            phase,
            status,
            origin_hash: endpoint_origin_hash(base_url),
        }
    }

    /// 将 Rig 错误降级为状态类别，丢弃 provider body 与原始错误文本。
    pub(super) fn from_completion_error(
        protocol: CustomProtocol,
        base_url: &str,
        error: rig::completion::CompletionError,
    ) -> Self {
        let status = error
            .provider_response_status()
            .map(|status| classify_http_status(status.as_u16()))
            .unwrap_or_else(|| match error {
                rig::completion::CompletionError::JsonError(_)
                | rig::completion::CompletionError::ResponseError(_) => {
                    CustomStatusCategory::InvalidResponse
                }
                rig::completion::CompletionError::UrlError(_) => {
                    CustomStatusCategory::InvalidEndpoint
                }
                _ => CustomStatusCategory::Network,
            });
        Self::new(protocol, CustomRequestPhase::Model, status, base_url)
    }

    /// 返回脱敏协议字段。
    pub(crate) const fn protocol(&self) -> CustomProtocol {
        self.protocol
    }

    /// 返回脱敏请求阶段。
    pub(crate) const fn phase(&self) -> CustomRequestPhase {
        self.phase
    }

    /// 返回脱敏 HTTP/网络状态类别。
    pub(crate) const fn status(&self) -> CustomStatusCategory {
        self.status
    }

    /// 返回不可逆 endpoint origin hash。
    pub(crate) fn origin_hash(&self) -> &str {
        &self.origin_hash
    }
}

/// 将 HTTP 状态映射为不携带状态正文的稳定类别。
fn classify_http_status(status: u16) -> CustomStatusCategory {
    match status {
        401 => CustomStatusCategory::Unauthorized,
        429 => CustomStatusCategory::RateLimited,
        400..=499 => CustomStatusCategory::Client,
        500..=599 => CustomStatusCategory::Server,
        _ => CustomStatusCategory::Unknown,
    }
}

/// Custom 请求失败发生的稳定阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CustomRequestPhase {
    /// 构造 client 或 URL 阶段。
    Setup,
    /// 发出模型请求阶段。
    Model,
    /// 归一化 provider response 阶段。
    Response,
}

/// 适配器公开给共享 runtime 的稳定、脱敏错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CustomAdapterError {
    /// 连接缺少 URL 或模型。
    InvalidConnection,
    /// 调用了与 adapter 协议不匹配的实现。
    UnsupportedProtocol(CustomProtocol),
    /// 工具集合不是严格的三项 repository 工具加 final tool。
    InvalidToolSet,
    /// 请求超出共享模型轮次预算。
    TurnLimitExceeded { limit: usize },
    /// 调用方在 transport 返回前取消了请求。
    Cancelled,
    /// provider/transport 失败的脱敏投影。
    Transport(CustomTransportFailure),
}

impl fmt::Display for CustomAdapterError {
    /// 将适配器错误转换为不含 URL、key、body、prompt 或模型输出的稳定文本。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConnection => formatter.write_str("Custom 连接配置无效。"),
            Self::UnsupportedProtocol(protocol) => {
                write!(
                    formatter,
                    "Custom 协议不受当前 adapter 支持：{protocol:?}。"
                )
            }
            Self::InvalidToolSet => formatter.write_str("Custom 工具集合不符合固定契约。"),
            Self::TurnLimitExceeded { limit } => {
                write!(formatter, "Custom 模型轮次已达到上限 {limit}。")
            }
            Self::Cancelled => formatter.write_str("Custom 请求已取消。"),
            Self::Transport(failure) => write!(
                formatter,
                "Custom {:?} 请求在 {:?} 阶段失败（状态 {:?}，origin {}）。",
                failure.protocol, failure.phase, failure.status, failure.origin_hash
            ),
        }
    }
}

impl std::error::Error for CustomAdapterError {}

/// OpenAI 与 Anthropic 共用的单轮协议适配器边界。
#[async_trait]
pub(crate) trait CustomProtocolAdapter: Send + Sync {
    /// 返回本 adapter 冻结的连接快照。
    fn connection(&self) -> &CustomConnectionSnapshot;

    /// 发送一轮 provider-neutral 模型请求并返回归一化结果。
    async fn complete(
        &self,
        request: CustomRoundRequest,
        cancellation: &CancellationSignal,
    ) -> Result<CustomRoundResult, CustomAdapterError>;
}

/// 从 URL 中只取 scheme 与 authority，再计算不可逆 origin hash。
fn endpoint_origin_hash(base_url: &str) -> String {
    use std::hash::{Hash, Hasher};
    let origin = base_url
        .split_once("://")
        .map(|(_, remainder)| {
            let authority_end = remainder.find(['/', '?', '#']).unwrap_or(remainder.len());
            &base_url[..base_url.len() - remainder.len() + authority_end]
        })
        .unwrap_or(base_url);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    origin.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
