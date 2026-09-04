use super::credential::{default_custom_credential_adapter, CustomCredentialAdapter};
use super::custom_connection::{CustomProviderConnection, CustomProviderSettings};
#[cfg(test)]
use super::model::BabysitModelPreference;
use super::model::{
    ApplicationLanguage, ApplicationSettings, BabysitProvider, BabysitProviderSettings,
    ThemePreference, SETTINGS_VERSION,
};
use std::{
    fmt,
    io::Write,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

/// 为旧 settings v1 缺少语言字段时提供系统 locale 的可注入边界。
pub(crate) trait SystemLocaleResolver: Send + Sync {
    /// 返回系统当前 locale；无法识别的值由应用边界映射为英文。
    fn resolve(&self) -> String;
}

impl<F> SystemLocaleResolver for F
where
    F: Fn() -> String + Send + Sync,
{
    /// 允许契约测试直接注入闭包而不暴露文件或平台实现。
    fn resolve(&self) -> String {
        self()
    }
}

impl<T> SystemLocaleResolver for Arc<T>
where
    T: SystemLocaleResolver + ?Sized,
{
    /// 保持已擦除 resolver 的共享所有权，同时保留注入式 seam。
    fn resolve(&self) -> String {
        (**self).resolve()
    }
}

/// 生产环境通过跨平台 OS API 取得当前语言提示。
struct OsSystemLocaleResolver;

impl SystemLocaleResolver for OsSystemLocaleResolver {
    /// 读取 macOS、Windows、Linux 等平台的首选 BCP-47 locale。
    fn resolve(&self) -> String {
        sys_locale::get_locale().unwrap_or_else(|| "en-US".to_owned())
    }
}

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
    locale_resolver: Arc<dyn SystemLocaleResolver>,
    update_lock: Arc<Mutex<()>>,
}

impl ApplicationSettingsApplication {
    /// 为首版统一设置文件建立应用边界，不读取或迁移旧文件。
    pub(crate) fn new(settings_path: PathBuf) -> Self {
        Self::with_system_locale_resolver(settings_path)
    }

    /// 使用跨平台 OS locale resolver 建立生产设置应用。
    #[allow(dead_code, reason = "供生产启动与 OS locale 集成契约使用")]
    pub(crate) fn with_system_locale_resolver(settings_path: PathBuf) -> Self {
        let credential_adapter = default_custom_credential_adapter(&settings_path);
        Self::with_dependencies(
            settings_path,
            credential_adapter,
            Arc::new(OsSystemLocaleResolver),
        )
    }

    /// 使用注入的凭据适配器建立设置应用，供 E2E 与契约测试隔离系统钥匙串。
    #[allow(dead_code, reason = "供契约测试注入凭据边界")]
    pub(crate) fn with_credential_adapter(
        settings_path: PathBuf,
        credential_adapter: Arc<dyn CustomCredentialAdapter>,
    ) -> Self {
        Self::with_dependencies(
            settings_path,
            credential_adapter,
            Arc::new(OsSystemLocaleResolver),
        )
    }

    /// 使用注入的系统 locale resolver 建立设置应用，供公开契约测试固定默认语言。
    #[allow(dead_code, reason = "供契约测试注入系统 locale 边界")]
    pub(crate) fn with_locale_resolver<R>(settings_path: PathBuf, resolver: R) -> Self
    where
        R: SystemLocaleResolver + 'static,
    {
        let credential_adapter = default_custom_credential_adapter(&settings_path);
        Self::with_dependencies(settings_path, credential_adapter, Arc::new(resolver))
    }

    /// 组装设置边界依赖，确保所有构造入口共享同一 resolver 生命周期。
    fn with_dependencies(
        settings_path: PathBuf,
        credential_adapter: Arc<dyn CustomCredentialAdapter>,
        locale_resolver: Arc<dyn SystemLocaleResolver>,
    ) -> Self {
        Self {
            settings_path,
            credential_adapter,
            locale_resolver,
            update_lock: Arc::new(Mutex::new(())),
        }
    }

    /// 读取并校验当前设置；缺失文件返回默认值且不写磁盘。
    pub(crate) fn load(&self) -> Result<ApplicationSettings, ApplicationSettingsError> {
        match std::fs::read_to_string(&self.settings_path) {
            Ok(contents) => {
                let document: serde_json::Value =
                    serde_json::from_str(&contents).map_err(ApplicationSettingsError::Malformed)?;
                let mut settings: ApplicationSettings = serde_json::from_value(document.clone())
                    .map_err(ApplicationSettingsError::Malformed)?;
                if document.get("language").is_none() {
                    settings.language = self.resolve_system_language();
                }
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
                Ok(self.default_settings())
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

    /// 原子保存 Application Language，并仅在落盘成功后返回新权威快照。
    pub(crate) fn update_language(
        &self,
        language: ApplicationLanguage,
    ) -> Result<ApplicationSettings, ApplicationSettingsError> {
        self.update(|settings| settings.language = language)
    }

    /// 原子切换 future analyses 使用的 provider，保留三套 provider 配置。
    pub(crate) fn update_provider(
        &self,
        provider: BabysitProvider,
    ) -> Result<ApplicationSettings, ApplicationSettingsError> {
        self.update(|settings| settings.babysit.provider = provider)
    }

    /// 原子保存未来分析的解释范围，不清理既有结果。
    pub(crate) fn update_all_non_empty_lines(
        &self,
        enabled: bool,
    ) -> Result<ApplicationSettings, ApplicationSettingsError> {
        self.update(|settings| settings.babysit.all_non_empty_lines = enabled)
    }

    /// 原子保存未来 Babysit 分析是否允许探索 Review Workspace。
    pub(crate) fn update_repository_exploration(
        &self,
        enabled: bool,
    ) -> Result<ApplicationSettings, ApplicationSettingsError> {
        self.update(|settings| settings.babysit.repository_exploration = enabled)
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
    #[allow(dead_code, reason = "供契约测试直接验证完整 Custom 更新边界")]
    pub(crate) fn update_custom_connection(
        &self,
        connection: CustomProviderConnection,
    ) -> Result<CustomProviderConnection, ApplicationSettingsError> {
        self.with_update_lock(|application| {
            application.update_custom_connection_unlocked(connection)
        })
    }

    /// 在调用方已持有更新锁时替换 Custom 连接，保证设置与凭据变更属于同一串行事务。
    fn update_custom_connection_unlocked(
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
        self.with_update_lock(|application| {
            application.update_custom_connection_unlocked(connection)?;
            application.load()
        })
    }

    /// 将 Codex runtime 的模型投影写回完整 provider 配置，同时保留 executable。
    #[cfg(test)]
    pub(crate) fn update_babysit_model(
        &self,
        preference: BabysitModelPreference,
    ) -> Result<ApplicationSettings, ApplicationSettingsError> {
        self.update(|settings| match preference {
            BabysitModelPreference::Automatic => {
                settings.babysit.codex.model = None;
                settings.babysit.codex.reasoning_effort = None;
            }
            BabysitModelPreference::Explicit {
                model,
                reasoning_effort,
            } => {
                settings.babysit.codex.model = Some(model);
                settings.babysit.codex.reasoning_effort = reasoning_effort;
            }
        })
    }

    /// 在内存快照完成一次变更，并仅在原子替换成功后返回新权威值。
    fn update(
        &self,
        change: impl FnOnce(&mut ApplicationSettings),
    ) -> Result<ApplicationSettings, ApplicationSettingsError> {
        self.with_update_lock(|application| {
            let mut settings = application.load()?;
            change(&mut settings);
            application.persist(&settings)?;
            Ok(settings)
        })
    }

    /// 串行化同一应用实例的所有读取、修改、持久化更新事务，并把锁毒化转为稳定错误。
    fn with_update_lock<T>(
        &self,
        operation: impl FnOnce(&Self) -> Result<T, ApplicationSettingsError>,
    ) -> Result<T, ApplicationSettingsError> {
        let _update_guard = self.lock_updates()?;
        operation(self)
    }

    /// 获取共享更新锁，避免同一应用实例的并发 update 丢失字段。
    fn lock_updates(&self) -> Result<MutexGuard<'_, ()>, ApplicationSettingsError> {
        self.update_lock.lock().map_err(|_| {
            ApplicationSettingsError::Io(std::io::Error::other("Application Settings 更新锁已中毒"))
        })
    }

    /// 以当前系统 locale 解析无持久化语言时的完整设备级默认快照。
    fn default_settings(&self) -> ApplicationSettings {
        ApplicationSettings {
            language: self.resolve_system_language(),
            ..ApplicationSettings::default()
        }
    }

    /// 将 locale family 映射为 Application Language 白名单中的一个稳定值。
    fn resolve_system_language(&self) -> ApplicationLanguage {
        let locale = self.locale_resolver.resolve();
        let family = locale
            .trim()
            .split(['-', '_', '.'])
            .next()
            .unwrap_or_default();
        if family.eq_ignore_ascii_case("zh") {
            ApplicationLanguage::SimplifiedChinese
        } else {
            ApplicationLanguage::English
        }
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
