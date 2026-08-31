use serde::{Deserialize, Serialize};
use std::fmt;

/// Custom provider 支持的模型服务协议。
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CustomProviderProtocol {
    /// OpenAI Chat Completions 兼容协议。
    #[default]
    OpenaiChatCompletions,
    /// Anthropic Messages 兼容协议。
    #[serde(rename = "anthropic_messages")]
    AnthropicMessages,
}

/// Custom provider 请求使用的推理参数策略。
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum CustomProviderEffort {
    /// 不覆盖模型服务自己的推理默认值。
    #[default]
    ProviderDefault,
    /// 尽快完成请求。
    Low,
    /// 在质量和成本之间使用平衡强度。
    Medium,
    /// 优先请求更高质量的推理。
    High,
}

/// 写入 settings.json 的 Custom 非敏感连接配置。
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CustomProviderSettings {
    /// 明确选择的模型服务协议，不根据 URL 推断。
    pub protocol: CustomProviderProtocol,
    /// 模型服务 base URL；具体 endpoint 路径由 provider 适配器负责。
    pub base_url: String,
    /// 用户输入的模型标识。
    pub model: String,
    /// 用户选择 Provider 默认值或显式低、中、高工作量级别。
    pub effort: CustomProviderEffort,
    /// 凭据变更 generation；参与进程内缓存 identity，不携带 secret。
    pub credential_generation: u64,
}

impl CustomProviderSettings {
    /// 判断非敏感快照是否足以启用 Custom provider，不执行任何网络探测。
    pub(crate) fn is_configured(&self) -> bool {
        !self.base_url.trim().is_empty() && !self.model.trim().is_empty()
    }
}

/// Settings 编辑流程使用的完整 Custom 连接；API key 只在内存和 IPC 中存在。
#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CustomProviderConnection {
    /// 明确选择的模型服务协议。
    pub protocol: CustomProviderProtocol,
    /// 模型服务 base URL。
    pub base_url: String,
    /// 可选 API key；空 key 表示删除设备凭据。
    pub api_key: String,
    /// 用户输入的模型标识。
    pub model: String,
    /// 用户选择 Provider 默认值或显式低、中、高工作量级别。
    pub effort: CustomProviderEffort,
}

impl Default for CustomProviderConnection {
    /// 构造未配置但可直接填充的 Custom 编辑连接。
    fn default() -> Self {
        Self {
            protocol: CustomProviderProtocol::default(),
            base_url: String::new(),
            api_key: String::new(),
            model: String::new(),
            effort: CustomProviderEffort::default(),
        }
    }
}

impl fmt::Debug for CustomProviderConnection {
    /// 调试时仅显示 API key 是否存在，避免日志意外暴露 secret。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CustomProviderConnection")
            .field("protocol", &self.protocol)
            .field("base_url", &self.base_url)
            .field(
                "api_key",
                &(!self.api_key.is_empty()).then_some("<redacted>"),
            )
            .field("model", &self.model)
            .field("effort", &self.effort)
            .finish()
    }
}

impl CustomProviderConnection {
    /// 判断连接是否具有本地运行所需的 base URL 与模型标识。
    pub(super) fn is_valid(&self) -> bool {
        !self.base_url.trim().is_empty() && !self.model.trim().is_empty()
    }
}

impl From<&CustomProviderSettings> for CustomProviderConnection {
    /// 将磁盘中的非敏感设置与待加载 key 组合成完整连接。
    fn from(value: &CustomProviderSettings) -> Self {
        Self {
            protocol: value.protocol,
            base_url: value.base_url.clone(),
            api_key: String::new(),
            model: value.model.clone(),
            effort: value.effort,
        }
    }
}

impl From<&CustomProviderConnection> for CustomProviderSettings {
    /// 丢弃 API key，仅投影可安全落盘的 Custom 配置。
    fn from(value: &CustomProviderConnection) -> Self {
        Self {
            protocol: value.protocol,
            base_url: value.base_url.clone(),
            model: value.model.clone(),
            effort: value.effort,
            credential_generation: 0,
        }
    }
}
