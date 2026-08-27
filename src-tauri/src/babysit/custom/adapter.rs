use super::connection::{endpoint_origin_hash, CustomConnectionSnapshot, CustomProtocol};
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
    /// 返回本 adapter 冻结的连接快照。
    fn connection(&self) -> &CustomConnectionSnapshot;
    /// 发送一轮 provider-neutral 模型请求并返回归一化结果。
    async fn complete(
        &self,
        request: CustomRoundRequest,
        cancellation: &CancellationSignal,
    ) -> Result<CustomRoundResult, CustomAdapterError>;
}
