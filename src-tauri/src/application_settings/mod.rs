use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::Write;
use std::path::PathBuf;

/// 当前 Application Settings 文件格式版本。
const SETTINGS_VERSION: u32 = 1;

/// Narrant 的持久化主题偏好。
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ThemePreference {
    /// 跟随操作系统当前主题及后续变化。
    #[default]
    System,
    /// 始终使用浅色主题。
    Light,
    /// 始终使用深色主题。
    Dark,
}

/// Babysit Mode 对未来分析使用的模型偏好。
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "mode")]
pub(crate) enum BabysitModelPreference {
    /// 每次分析采用 Codex App Server 的默认模型与思考强度。
    #[default]
    Automatic,
    /// 固定使用指定模型；空思考强度表示采用该模型的服务端默认值。
    Explicit {
        model: String,
        reasoning_effort: Option<String>,
    },
}

/// 经校验的设备级 Application Settings 快照。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
#[serde(deny_unknown_fields)]
pub(crate) struct ApplicationSettings {
    pub version: u32,
    pub theme: ThemePreference,
    pub codex_executable: Option<PathBuf>,
    pub babysit_model: BabysitModelPreference,
}

/// 旧 Babysit 文件中唯一可迁移的非敏感偏好。
#[derive(Deserialize)]
struct LegacyBabysitSettings {
    executable: Option<PathBuf>,
}

impl Default for ApplicationSettings {
    /// 构造首次启动采用的安全默认设置。
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            theme: ThemePreference::System,
            codex_executable: None,
            babysit_model: BabysitModelPreference::Automatic,
        }
    }
}

/// Application Settings 读取或持久化失败。
#[derive(Debug)]
pub(crate) enum ApplicationSettingsError {
    Io(std::io::Error),
    Malformed(serde_json::Error),
    UnsupportedVersion(u32),
    InvalidCodexExecutable,
}

impl fmt::Display for ApplicationSettingsError {
    /// 将设置失败转换为不泄露文件内容的稳定诊断。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "Application Settings I/O 失败：{error}"),
            Self::Malformed(error) => write!(formatter, "settings.json 格式无效：{error}"),
            Self::UnsupportedVersion(version) => {
                write!(formatter, "不支持 settings.json 版本 {version}")
            }
            Self::InvalidCodexExecutable => formatter.write_str("Codex executable 必须是绝对路径"),
        }
    }
}

impl std::error::Error for ApplicationSettingsError {}

/// 设备级 Application Settings 的应用契约。
pub(crate) struct ApplicationSettingsApplication {
    settings_path: PathBuf,
    legacy_babysit_path: PathBuf,
}

impl ApplicationSettingsApplication {
    /// 为统一设置文件与一次性旧 Babysit 设置来源建立应用边界。
    pub(crate) fn new(settings_path: PathBuf, legacy_babysit_path: PathBuf) -> Self {
        Self {
            settings_path,
            legacy_babysit_path,
        }
    }

    /// 读取并校验当前设置；缺失文件返回默认值且不产生磁盘写入。
    pub(crate) fn load(&self) -> Result<ApplicationSettings, ApplicationSettingsError> {
        match std::fs::read_to_string(&self.settings_path) {
            Ok(contents) => {
                let settings = serde_json::from_str::<ApplicationSettings>(&contents)
                    .map_err(ApplicationSettingsError::Malformed)?;
                if settings.version != SETTINGS_VERSION {
                    return Err(ApplicationSettingsError::UnsupportedVersion(
                        settings.version,
                    ));
                }
                if settings
                    .codex_executable
                    .as_ref()
                    .is_some_and(|path| !path.is_absolute())
                {
                    return Err(ApplicationSettingsError::InvalidCodexExecutable);
                }
                Ok(settings)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => self.migrate_legacy(),
            Err(error) => Err(ApplicationSettingsError::Io(error)),
        }
    }

    /// 缺少统一文件时迁移一次旧 executable；无有效旧值仍只返回默认值。
    fn migrate_legacy(&self) -> Result<ApplicationSettings, ApplicationSettingsError> {
        let contents = match std::fs::read_to_string(&self.legacy_babysit_path) {
            Ok(contents) => contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ApplicationSettings::default());
            }
            Err(error) => return Err(ApplicationSettingsError::Io(error)),
        };
        let legacy = serde_json::from_str::<LegacyBabysitSettings>(&contents)
            .map_err(ApplicationSettingsError::Malformed)?;
        let Some(executable) = legacy.executable else {
            return Ok(ApplicationSettings::default());
        };
        if !executable.is_absolute() {
            return Err(ApplicationSettingsError::InvalidCodexExecutable);
        }
        let settings = ApplicationSettings {
            codex_executable: Some(executable),
            ..ApplicationSettings::default()
        };
        self.persist(&settings)?;
        Ok(settings)
    }

    /// 原子保存主题并返回新的权威快照；失败时旧文件保持不变。
    pub(crate) fn update_theme(
        &self,
        theme: ThemePreference,
    ) -> Result<ApplicationSettings, ApplicationSettingsError> {
        let mut settings = self.load()?;
        settings.theme = theme;
        self.persist(&settings)?;
        Ok(settings)
    }

    /// 原子更新 Codex Executable Preference，并保留同一快照中的其他设置。
    pub(crate) fn update_codex_executable(
        &self,
        executable: Option<PathBuf>,
    ) -> Result<ApplicationSettings, ApplicationSettingsError> {
        if executable.as_ref().is_some_and(|path| !path.is_absolute()) {
            return Err(ApplicationSettingsError::InvalidCodexExecutable);
        }
        let mut settings = self.load()?;
        settings.codex_executable = executable;
        self.persist(&settings)?;
        Ok(settings)
    }

    /// 在目标目录写入临时文件并原子替换正式设置文件。
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

#[cfg(test)]
#[path = "tests/contract.rs"]
mod tests;
