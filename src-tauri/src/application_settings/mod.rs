use serde::{Deserialize, Serialize};
use std::{fmt, io::Write, path::PathBuf};

const SETTINGS_VERSION: u32 = 1;

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

/// Babysit Mode 支持的稳定 provider 身份。
#[derive(Clone, Copy, Debug, Default, Deserialize, Hash, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum BabysitProvider {
    /// 使用 Codex App Server。
    #[default]
    Codex,
    /// 使用本机 Claude Code。
    ClaudeCode,
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

/// Babysit 的设备级 provider 选择及两套独立配置。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct BabysitSettings {
    pub provider: BabysitProvider,
    pub codex: BabysitProviderSettings,
    pub claude_code: BabysitProviderSettings,
}

impl Default for BabysitSettings {
    /// 首次启动默认选择 Codex，并为两个 provider 建立空配置。
    fn default() -> Self {
        Self {
            provider: BabysitProvider::Codex,
            codex: BabysitProviderSettings::default(),
            claude_code: BabysitProviderSettings::default(),
        }
    }
}

/// 经校验的设备级 Application Settings 快照。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ApplicationSettings {
    pub version: u32,
    pub theme: ThemePreference,
    pub babysit: BabysitSettings,
}

impl Default for ApplicationSettings {
    /// 构造 settings v1 的设备级默认快照。
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            theme: ThemePreference::System,
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

/// Application Settings 读取或持久化失败。
#[derive(Debug)]
pub(crate) enum ApplicationSettingsError {
    Io(std::io::Error),
    Malformed(serde_json::Error),
    UnsupportedVersion(u32),
    InvalidProviderPreference,
}

impl fmt::Display for ApplicationSettingsError {
    /// 将设置错误投影为稳定且不泄露设置内容的用户可读诊断。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Application Settings I/O 失败：{error}"),
            Self::Malformed(error) => write!(formatter, "settings.json 格式无效：{error}"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "不支持 settings.json 版本 {version}")
            }
            Self::InvalidProviderPreference => formatter.write_str("Babysit provider 偏好组合无效"),
        }
    }
}
impl std::error::Error for ApplicationSettingsError {}

/// 设备级 Application Settings 的应用契约。
#[derive(Clone)]
pub(crate) struct ApplicationSettingsApplication {
    settings_path: PathBuf,
}

impl ApplicationSettingsApplication {
    /// 为首版统一设置文件建立应用边界，不读取或迁移旧文件。
    pub(crate) fn new(settings_path: PathBuf) -> Self {
        Self { settings_path }
    }

    /// 读取并校验当前设置；缺失文件返回默认值且不写磁盘。
    pub(crate) fn load(&self) -> Result<ApplicationSettings, ApplicationSettingsError> {
        match std::fs::read_to_string(&self.settings_path) {
            Ok(contents) => {
                let settings: ApplicationSettings =
                    serde_json::from_str(&contents).map_err(ApplicationSettingsError::Malformed)?;
                if settings.version != SETTINGS_VERSION {
                    return Err(ApplicationSettingsError::UnsupportedVersion(
                        settings.version,
                    ));
                }
                validate_provider_settings(BabysitProvider::Codex, &settings.babysit.codex)?;
                validate_provider_settings(
                    BabysitProvider::ClaudeCode,
                    &settings.babysit.claude_code,
                )?;
                Ok(settings)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(ApplicationSettings::default())
            }
            Err(error) => Err(ApplicationSettingsError::Io(error)),
        }
    }

    /// 原子保存主题并返回新快照。
    pub(crate) fn update_theme(
        &self,
        theme: ThemePreference,
    ) -> Result<ApplicationSettings, ApplicationSettingsError> {
        self.update(|settings| settings.theme = theme)
    }

    /// 原子切换 future analyses 使用的 provider，保留两套 provider 配置。
    pub(crate) fn update_provider(
        &self,
        provider: BabysitProvider,
    ) -> Result<ApplicationSettings, ApplicationSettingsError> {
        self.update(|settings| settings.babysit.provider = provider)
    }

    /// 原子替换一个 provider 的完整 executable/model/effort 组合。
    pub(crate) fn update_provider_settings(
        &self,
        provider: BabysitProvider,
        preference: BabysitProviderSettings,
    ) -> Result<ApplicationSettings, ApplicationSettingsError> {
        validate_provider_settings(provider, &preference)?;
        self.update(|settings| match provider {
            BabysitProvider::Codex => settings.babysit.codex = preference,
            BabysitProvider::ClaudeCode => settings.babysit.claude_code = preference,
        })
    }

    /// 将 Codex runtime 的模型投影写回完整 provider 配置，同时保留 executable。
    #[cfg(test)]
    pub(crate) fn update_babysit_model(
        &self,
        preference: BabysitModelPreference,
    ) -> Result<ApplicationSettings, ApplicationSettingsError> {
        let mut provider = self.load()?.babysit.codex;
        match preference {
            BabysitModelPreference::Automatic => {
                provider.model = None;
                provider.reasoning_effort = None;
            }
            BabysitModelPreference::Explicit {
                model,
                reasoning_effort,
            } => {
                provider.model = Some(model);
                provider.reasoning_effort = reasoning_effort;
            }
        }
        self.update_provider_settings(BabysitProvider::Codex, provider)
    }

    /// 在内存快照完成一次变更，并仅在原子替换成功后返回新权威值。
    fn update(
        &self,
        change: impl FnOnce(&mut ApplicationSettings),
    ) -> Result<ApplicationSettings, ApplicationSettingsError> {
        let mut settings = self.load()?;
        change(&mut settings);
        self.persist(&settings)?;
        Ok(settings)
    }

    /// 通过同目录临时文件同步落盘并原子替换正式设置文件。
    fn persist(&self, settings: &ApplicationSettings) -> Result<(), ApplicationSettingsError> {
        let parent = self.settings_path.parent().ok_or_else(|| {
            ApplicationSettingsError::Io(std::io::Error::other("设置路径缺少父目录"))
        })?;
        std::fs::create_dir_all(parent).map_err(ApplicationSettingsError::Io)?;
        let payload =
            serde_json::to_vec_pretty(settings).map_err(ApplicationSettingsError::Malformed)?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(ApplicationSettingsError::Io)?;
        temporary
            .write_all(&payload)
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(ApplicationSettingsError::Io)?;
        temporary
            .persist(&self.settings_path)
            .map_err(|error| ApplicationSettingsError::Io(error.error))?;
        Ok(())
    }
}

/// 校验 provider 的 nullable model/effort 是否构成后端可接受的完整组合。
fn validate_provider_settings(
    provider: BabysitProvider,
    preference: &BabysitProviderSettings,
) -> Result<(), ApplicationSettingsError> {
    if preference.model.is_none() && preference.reasoning_effort.is_some() {
        return Err(ApplicationSettingsError::InvalidProviderPreference);
    }
    if provider != BabysitProvider::ClaudeCode {
        return Ok(());
    }
    let Some(model) = preference.model.as_deref() else {
        return Ok(());
    };
    if model.trim().is_empty() {
        return Err(ApplicationSettingsError::InvalidProviderPreference);
    }
    let effort_valid = preference
        .reasoning_effort
        .as_deref()
        .is_none_or(|effort| matches!(effort, "low" | "medium" | "high" | "xhigh" | "max"));
    if !effort_valid || (model == "haiku" && preference.reasoning_effort.is_some()) {
        return Err(ApplicationSettingsError::InvalidProviderPreference);
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/contract.rs"]
mod tests;
