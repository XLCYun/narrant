use super::connection::{endpoint_origin_hash, CustomProtocol};
use super::round::{CustomRoundRequest, CustomRoundResult};
use crate::operation::CancellationSignal;
use std::fmt;

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

impl CustomStatusCategory {
    /// 返回不携带 provider 正文的稳定状态分类。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Network => "network",
            Self::InvalidEndpoint => "invalid_endpoint",
            Self::Unauthorized => "unauthorized",
            Self::Client => "client",
            Self::Server => "server",
            Self::RateLimited => "rate_limited",
            Self::InvalidResponse => "invalid_response",
            Self::Unknown => "unknown",
        }
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

impl CustomRequestPhase {
    /// 返回可跨 IPC 传递的稳定请求阶段。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Setup => "setup",
            Self::Model => "model",
            Self::Response => "response",
        }
    }
}

/// 从 provider 错误信封归一化出的安全原因；绝不保存原始正文。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CustomFailureReason {
    /// provider 报告模型不存在或当前端点不可用。
    ModelNotFound,
    /// Chat Completions 不接受工具与当前 reasoning effort 的组合。
    ReasoningToolsIncompatible,
    /// 当前模型或端点不支持函数工具。
    ToolsUnsupported,
    /// provider 不认识请求中的某个参数。
    ParameterUnsupported,
    /// 2xx 响应实际承载了 provider 错误信封。
    ErrorEnvelope,
    /// 模型在固定调用预算内没有提交最终结果。
    ModelTurnLimit,
    /// 模型调用了当前回合没有注册或不允许的工具。
    UnknownToolCall,
    /// 错误正文没有命中任何白名单原因。
    Unknown,
}

impl CustomFailureReason {
    /// 返回可跨 IPC 传递的稳定原因代码。
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ModelNotFound => "model_not_found",
            Self::ReasoningToolsIncompatible => "reasoning_tools_incompatible",
            Self::ToolsUnsupported => "tools_unsupported",
            Self::ParameterUnsupported => "parameter_unsupported",
            Self::ErrorEnvelope => "error_envelope",
            Self::ModelTurnLimit => "model_turn_limit",
            Self::UnknownToolCall => "unknown_tool_call",
            Self::Unknown => "unknown",
        }
    }
}

/// 只包含协议、阶段、状态、白名单原因与 endpoint origin hash 的脱敏传输失败。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CustomTransportFailure {
    protocol: CustomProtocol,
    phase: CustomRequestPhase,
    status: CustomStatusCategory,
    reason: CustomFailureReason,
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
            reason: CustomFailureReason::Unknown,
            origin_hash: endpoint_origin_hash(base_url),
        }
    }
    /// 将 Rig agent 错误降级为状态类别，丢弃 provider body 与原始错误文本。
    pub(super) fn from_prompt_error(
        protocol: CustomProtocol,
        base_url: &str,
        error: rig::completion::PromptError,
    ) -> Self {
        let response_status = error.provider_response_status();
        let reason = classify_failure_reason(&error, response_status.map(|status| status.as_u16()));
        let status = response_status
            .map(|status| classify_http_status(status.as_u16()))
            .unwrap_or_else(|| match error {
                rig::completion::PromptError::CompletionError(error) => match error {
                    rig::completion::CompletionError::JsonError(_)
                    | rig::completion::CompletionError::ResponseError(_) => {
                        CustomStatusCategory::InvalidResponse
                    }
                    rig::completion::CompletionError::UrlError(_) => {
                        CustomStatusCategory::InvalidEndpoint
                    }
                    rig::completion::CompletionError::HttpError(_) => CustomStatusCategory::Network,
                    _ => CustomStatusCategory::Unknown,
                },
                rig::completion::PromptError::UnknownToolCall { .. } => {
                    CustomStatusCategory::InvalidResponse
                }
                rig::completion::PromptError::MaxTurnsError { .. } => {
                    CustomStatusCategory::InvalidResponse
                }
                _ => CustomStatusCategory::Unknown,
            });
        Self {
            reason,
            ..Self::new(protocol, CustomRequestPhase::Model, status, base_url)
        }
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
    /// 返回从 provider 错误信封归一化出的白名单原因。
    pub(crate) const fn reason(&self) -> CustomFailureReason {
        self.reason
    }
    /// 返回不可逆 endpoint origin hash。
    pub(crate) fn origin_hash(&self) -> &str {
        &self.origin_hash
    }
}

/// 只检查有限长度的错误字段并投影到固定枚举，原始正文不会离开本函数。
fn classify_failure_reason(
    error: &rig::completion::PromptError,
    response_status: Option<u16>,
) -> CustomFailureReason {
    match error {
        rig::completion::PromptError::MaxTurnsError { .. } => {
            return CustomFailureReason::ModelTurnLimit;
        }
        rig::completion::PromptError::UnknownToolCall { .. } => {
            return CustomFailureReason::UnknownToolCall;
        }
        _ => {}
    }
    let response = error
        .provider_response_json()
        .ok()
        .flatten()
        .map(|json| {
            let fields = [
                json.pointer("/error/code")
                    .and_then(serde_json::Value::as_str),
                json.pointer("/error/type")
                    .and_then(serde_json::Value::as_str),
                json.pointer("/error/message")
                    .and_then(serde_json::Value::as_str),
                json.get("message").and_then(serde_json::Value::as_str),
            ];
            fields.into_iter().flatten().collect::<Vec<_>>().join(" ")
        })
        .or_else(|| error.provider_response_body().map(str::to_owned))
        .unwrap_or_default()
        .chars()
        .take(4096)
        .collect::<String>()
        .to_ascii_lowercase();

    if response.contains("model_not_found")
        || response.contains("model not found")
        || response.contains("model does not exist")
        || response.contains("model doesn't exist")
    {
        return CustomFailureReason::ModelNotFound;
    }
    let rejects = response.contains("not supported")
        || response.contains("unsupported")
        || response.contains("cannot")
        || response.contains("does not work");
    if response.contains("reasoning_effort")
        && (response.contains("tool") || response.contains("function"))
        && rejects
    {
        return CustomFailureReason::ReasoningToolsIncompatible;
    }
    if (response.contains("tool") || response.contains("function")) && rejects {
        return CustomFailureReason::ToolsUnsupported;
    }
    if response.contains("unknown parameter")
        || response.contains("unsupported parameter")
        || response.contains("unrecognized request argument")
    {
        return CustomFailureReason::ParameterUnsupported;
    }
    if response_status.is_some_and(|status| (200..=299).contains(&status)) {
        return CustomFailureReason::ErrorEnvelope;
    }
    CustomFailureReason::Unknown
}

/// 将 HTTP 状态映射为不携带状态正文的稳定类别。
fn classify_http_status(status: u16) -> CustomStatusCategory {
    match status {
        // 部分 OpenAI-compatible 网关会用 2xx 包装 `{ "error": ... }`；Rig 已把它识别为
        // provider failure，因此这里不能再按成功处理，也不能降级成含混的 Unknown。
        200..=299 => CustomStatusCategory::InvalidResponse,
        401 => CustomStatusCategory::Unauthorized,
        429 => CustomStatusCategory::RateLimited,
        400..=499 => CustomStatusCategory::Client,
        500..=599 => CustomStatusCategory::Server,
        _ => CustomStatusCategory::Unknown,
    }
}

/// 适配器公开给共享 runtime 的稳定、脱敏错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CustomAdapterError {
    /// 连接缺少 URL 或模型。
    InvalidConnection,
    /// 调用了与 adapter 协议不匹配的 implementation。
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
            Self::UnsupportedProtocol(protocol) => write!(
                formatter,
                "Custom 协议不受当前 adapter 支持：{protocol:?}。"
            ),
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

/// OpenAI 与 Anthropic 共用的单轮协议适配器 seam。
#[async_trait::async_trait]
pub(crate) trait CustomProtocolAdapter: Send + Sync {
    /// 发送一轮 provider-neutral 模型请求并返回归一化结果。
    async fn complete(
        &self,
        request: CustomRoundRequest,
        cancellation: &CancellationSignal,
    ) -> Result<CustomRoundResult, CustomAdapterError>;
}
