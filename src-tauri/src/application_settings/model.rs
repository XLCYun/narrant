use super::custom_connection::CustomProviderSettings;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub(super) const SETTINGS_VERSION: u32 = 1;

/// Narrant 的持久化主题偏好。
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ThemePreference {
    /// 跟随操作系统主题。
    #[default]
    System,
    /// 始终使用浅色主题。
    Light,
    /// 始终使用深色主题。
    Dark,
}

/// 设备级 Application Language 的稳定取值。
#[derive(Clone, Copy, Debug, Deserialize, Hash, PartialEq, Eq, Serialize)]
pub(crate) enum ApplicationLanguage {
    /// 使用简体中文界面与后续解释语言。
    #[serde(rename = "zh-CN")]
    SimplifiedChinese,
    /// 使用英文界面与后续解释语言。
    #[serde(rename = "en-US")]
    English,
}

impl ApplicationLanguage {
    /// 返回用于持久化与跨边界传输的稳定 BCP-47 language tag。
    #[allow(dead_code, reason = "供后续 Babysit 语言快照消费稳定 tag")]
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::SimplifiedChinese => "zh-CN",
            Self::English => "en-US",
        }
    }
}

impl Default for ApplicationLanguage {
    /// 为无法访问系统 locale 的静态反序列化场景提供稳定回退值。
    fn default() -> Self {
        Self::English
    }
}

/// Babysit Mode 支持的稳定 provider 身份。
#[derive(Clone, Copy, Debug, Default, Deserialize, Hash, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum BabysitProvider {
    /// 使用用户配置的直接模型服务连接。
    #[default]
    Custom,
    /// 使用用户安装的 Codex App Server。
    Codex,
    /// 使用本机 Claude Code。
    ClaudeCode,
}

impl BabysitProvider {
    /// 返回不会泄露底层 transport 的稳定 provider identity。
    pub(crate) const fn identity(self) -> &'static str {
        match self {
            Self::Codex => "codex",
            Self::ClaudeCode => "claude-code",
            Self::Custom => "custom",
        }
    }
}

/// 单个 provider 独立保留的完整偏好组合。
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BabysitProviderSettings {
    /// 用户显式选择的 executable；空值恢复 provider 自动发现。
    pub executable: Option<PathBuf>,
    /// 用户显式选择的模型；空值使用 provider 默认模型。
    pub model: Option<String>,
    /// 用户显式选择的思考强度；空值使用 provider 默认强度。
    pub reasoning_effort: Option<String>,
}

/// Babysit 的设备级 provider 选择及三套独立配置。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BabysitSettings {
    pub provider: BabysitProvider,
    /// 未来 Babysit 分析是否可以读取 File Change 之外的 Review Workspace 内容。
    #[serde(default)]
    pub repository_exploration: bool,
    /// 是否为全部非空行生成解释；关闭时使用编辑器提供的变更范围。
    #[serde(default)]
    pub all_non_empty_lines: bool,
    pub codex: BabysitProviderSettings,
    pub claude_code: BabysitProviderSettings,
    /// Custom 连接的非 secret 快照。
    pub custom: CustomProviderSettings,
}

impl Default for BabysitSettings {
    /// 首次启动默认选择 Custom，并为三个 provider 建立独立配置。
    fn default() -> Self {
        Self {
            provider: BabysitProvider::Custom,
            repository_exploration: false,
            all_non_empty_lines: false,
            codex: BabysitProviderSettings::default(),
            claude_code: BabysitProviderSettings::default(),
            custom: CustomProviderSettings::default(),
        }
    }
}

/// 一次分析从同一 Application Settings 快照借用的 provider 配置。
pub(crate) enum BabysitProviderPreference<'a> {
    /// Installed provider 的 executable/model/effort 组合。
    Installed(&'a BabysitProviderSettings),
    /// Custom 的协议、地址、模型与 effort 组合。
    Custom(&'a CustomProviderSettings),
}

impl BabysitSettings {
    /// 返回 Custom 独有的非敏感连接配置，供 registry 冻结当前设置。
    pub(crate) fn custom_settings(&self) -> &CustomProviderSettings {
        &self.custom
    }

    /// 按稳定 provider identity 投影同一设置快照，避免应用层出现 provider 分支。
    pub(crate) fn provider_preference(
        &self,
        provider: BabysitProvider,
    ) -> BabysitProviderPreference<'_> {
        match provider {
            BabysitProvider::Codex => BabysitProviderPreference::Installed(&self.codex),
            BabysitProvider::ClaudeCode => BabysitProviderPreference::Installed(&self.claude_code),
            BabysitProvider::Custom => BabysitProviderPreference::Custom(&self.custom),
        }
    }
}

/// 经校验的设备级 Application Settings 值。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ApplicationSettings {
    pub version: u32,
    pub theme: ThemePreference,
    /// 当前设备级语言；旧 settings v1 缺少该字段时由应用边界补出。
    #[serde(default)]
    pub language: ApplicationLanguage,
    pub babysit: BabysitSettings,
}

impl Default for ApplicationSettings {
    /// 构造 settings v1 的设备级默认快照。
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            theme: ThemePreference::System,
            language: ApplicationLanguage::English,
            babysit: BabysitSettings::default(),
        }
    }
}

/// Codex runtime 使用的 automatic/explicit 投影。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub(crate) enum BabysitModelPreference {
    #[default]
    Automatic,
    Explicit {
        #[serde(rename = "modelId")]
        model: String,
        #[serde(rename = "reasoningEffort")]
        reasoning_effort: Option<String>,
    },
}

impl From<&BabysitProviderSettings> for BabysitModelPreference {
    /// 将 nullable model/effort 配置投影为 Codex 目录解析契约。
    fn from(value: &BabysitProviderSettings) -> Self {
        match &value.model {
            Some(model) => Self::Explicit {
                model: model.clone(),
                reasoning_effort: value.reasoning_effort.clone(),
            },
            None => Self::Automatic,
        }
    }
}
