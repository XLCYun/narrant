use super::adapter::CustomAdapterError;
use secrecy::{ExposeSecret, SecretString};
use std::fmt;

/// Custom 连接显式选择的模型服务协议；不从 URL 推断协议。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CustomProtocol {
    /// OpenAI Chat Completions 兼容协议。
    OpenaiChatCompletions,
    /// Anthropic Messages 兼容协议。
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

/// 从 URL 中只取 scheme 与 authority，再计算不可逆 origin hash。
pub(super) fn endpoint_origin_hash(base_url: &str) -> String {
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
