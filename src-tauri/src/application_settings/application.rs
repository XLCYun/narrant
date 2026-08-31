use super::credential::{default_custom_credential_adapter, CustomCredentialAdapter};
use super::custom_connection::{CustomProviderConnection, CustomProviderSettings};
#[cfg(test)]
use super::model::BabysitModelPreference;
use super::model::{
    ApplicationSettings, BabysitProvider, BabysitProviderSettings, ThemePreference,
    SETTINGS_VERSION,
};
use std::{fmt, io::Write, path::PathBuf, sync::Arc};

/// Application Settings 读取或持久化失败。
pub(crate) enum ApplicationSettingsError {
    Io(std::io::Error),
    Malformed(serde_json::Error),
    UnsupportedVersion(u32),
    InvalidProviderPreference,
    InvalidCustomConnection,
    Credential(String),
}

impl fmt::Debug for ApplicationSettingsError {
    /// 调试输出不回显凭据适配器可能携带的敏感详情。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => formatter.debug_tuple("Io").field(error).finish(),
            Self::Malformed(error) => formatter.debug_tuple("Malformed").field(error).finish(),
            Self::UnsupportedVersion(version) => formatter
                .debug_tuple("UnsupportedVersion")
                .field(version)
                .finish(),
            Self::InvalidProviderPreference => formatter.write_str("InvalidProviderPreference"),
            Self::InvalidCustomConnection => formatter.write_str("InvalidCustomConnection"),
            Self::Credential(_) => formatter.write_str("Credential(<redacted>)"),
        }
    }
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
            Self::InvalidCustomConnection => {
                formatter.write_str("Custom 模型服务连接缺少有效的 base URL 或模型")
            }
            Self::Credential(detail) => {
                // 适配器详情可能包含敏感内容，只读取以保持错误值可调试而不回显。
                let _ = detail;
                formatter.write_str("Custom API key 保存失败：无法访问安全凭据存储")
            }
        }
    }
}

impl std::error::Error for ApplicationSettingsError {}

/// 设备级 Application Settings 的应用契约。
#[derive(Clone)]
pub(crate) struct ApplicationSettingsApplication {
    settings_path: PathBuf,
    credential_adapter: Arc<dyn CustomCredentialAdapter>,
}

impl ApplicationSettingsApplication {
    /// 为首版统一设置文件建立应用边界，不读取或迁移旧文件。
    pub(crate) fn new(settings_path: PathBuf) -> Self {
        let credential_adapter = default_custom_credential_adapter(&settings_path);
        Self::with_credential_adapter(settings_path, credential_adapter)
    }

    /// 使用注入的凭据适配器建立设置应用，供 E2E 与契约测试隔离系统钥匙串。
    pub(crate) fn with_credential_adapter(
        settings_path: PathBuf,
        credential_adapter: Arc<dyn CustomCredentialAdapter>,
    ) -> Self {
        Self {
            settings_path,
            credential_adapter,
        }
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

    /// 原子切换 future analyses 使用的 provider，保留三套 provider 配置。
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
        if provider == BabysitProvider::Custom {
            return Err(ApplicationSettingsError::InvalidProviderPreference);
        }
        validate_provider_settings(provider, &preference)?;
        self.update(|settings| match provider {
            BabysitProvider::Codex => settings.babysit.codex = preference,
            BabysitProvider::ClaudeCode => settings.babysit.claude_code = preference,
            BabysitProvider::Custom => unreachable!("Custom provider uses a distinct connection"),
        })
    }

    /// 读取完整 Custom 连接，并仅在返回值中临时携带当前 API key。
    pub(crate) fn load_custom_connection(
        &self,
    ) -> Result<CustomProviderConnection, ApplicationSettingsError> {
        self.load_custom_connection_with_generation()
            .map(|(connection, _)| connection)
    }

    /// 读取同一非敏感设置快照及其凭据 generation，供运行开始时冻结连接 identity。
    pub(crate) fn load_custom_connection_with_generation(
        &self,
    ) -> Result<(CustomProviderConnection, u64), ApplicationSettingsError> {
        let settings = self.load()?;
        let api_key = self
            .credential_adapter
            .load()
            .map_err(ApplicationSettingsError::Credential)?;
        let mut connection = CustomProviderConnection::from(settings.babysit.custom_settings());
        connection.api_key = api_key.unwrap_or_default();
        Ok((connection, settings.babysit.custom.credential_generation))
    }

    /// 校验并原子替换完整 Custom 连接，先写非敏感设置再更新系统凭据。
    pub(crate) fn update_custom_connection(
        &self,
        connection: CustomProviderConnection,
    ) -> Result<CustomProviderConnection, ApplicationSettingsError> {
        validate_custom_connection(&connection)?;
        let preceding = self.load()?;
        let mut next = preceding.clone();
        next.babysit.custom = CustomProviderSettings::from(&connection);
        next.babysit.custom.credential_generation = preceding
            .babysit
            .custom
            .credential_generation
            .saturating_add(1);
        self.persist(&next)?;

        let key = (!connection.api_key.is_empty()).then(|| connection.api_key.clone());
        let mutation = match key.as_deref() {
            Some(value) => self.credential_adapter.save(value),
            None => self.credential_adapter.delete(),
        };
        if let Err(detail) = mutation {
            // 凭据更新失败时恢复此前完整的非敏感 settings 快照，避免两端描述不同连接。
            let restore = self.persist(&preceding);
            if let Err(restore_error) = restore {
                return Err(ApplicationSettingsError::Credential(format!(
                    "{detail}（恢复原设置失败：{restore_error}）"
                )));
            }
            return Err(ApplicationSettingsError::Credential(detail));
        }

        let mut saved = connection;
        saved.api_key = key.unwrap_or_default();
        Ok(saved)
    }

    /// 保存完整 Custom 连接后返回新的非敏感 Application Settings 快照。
    pub(crate) fn save_custom_connection(
        &self,
        connection: CustomProviderConnection,
    ) -> Result<ApplicationSettings, ApplicationSettingsError> {
        self.update_custom_connection(connection)?;
        self.load()
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
    if provider != BabysitProvider::ClaudeCode {
        return Ok(());
    }
    if preference
        .model
        .as_deref()
        .is_some_and(|model| model.trim().is_empty())
    {
        return Err(ApplicationSettingsError::InvalidProviderPreference);
    }
    let effort_valid = preference
        .reasoning_effort
        .as_deref()
        .is_none_or(|effort| matches!(effort, "low" | "medium" | "high" | "xhigh" | "max"));
    if !effort_valid
        || (preference.model.as_deref() == Some("haiku") && preference.reasoning_effort.is_some())
    {
        return Err(ApplicationSettingsError::InvalidProviderPreference);
    }
    Ok(())
}

/// 校验 Custom 连接的本地字段，不探测网络或模型服务能力。
fn validate_custom_connection(
    connection: &CustomProviderConnection,
) -> Result<(), ApplicationSettingsError> {
    if !connection.is_valid() {
        return Err(ApplicationSettingsError::InvalidCustomConnection);
    }
    Ok(())
}
