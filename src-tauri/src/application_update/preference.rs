use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

const PREFERENCE_VERSION: u32 = 1;

/// 为自动下载设置提供最小持久化边界。
pub(crate) trait PreferenceStore {
    /// 读取自动下载设置；缺失表示采用默认开启。
    fn load_automatic_download(&self) -> Result<Option<bool>, PreferenceError>;

    /// 原子持久化自动下载设置。
    fn save_automatic_download(&mut self, enabled: bool) -> Result<(), PreferenceError>;
}

/// 描述偏好文件失败的稳定错误，不暴露本机路径。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreferenceError {
    /// 偏好文件不能读取或原子写入。
    Storage,
    /// 偏好文件格式、版本或字段不受支持。
    InvalidDocument,
}

impl fmt::Display for PreferenceError {
    /// 输出可安全用于诊断的稳定错误文本。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage => formatter.write_str("应用更新偏好无法读写"),
            Self::InvalidDocument => formatter.write_str("应用更新偏好格式无效"),
        }
    }
}

/// Application Update 独占的版本化偏好文档。
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PreferenceDocument {
    version: u32,
    automatic_download: bool,
}

/// 把自动下载偏好原子保存到独立 JSON 文件的生产仓储。
pub(crate) struct FilePreferenceStore {
    path: PathBuf,
}

impl FilePreferenceStore {
    /// 创建一个仅持有 Application Update 偏好的文件仓储。
    pub(crate) fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }
}

impl PreferenceStore for FilePreferenceStore {
    /// 读取并验证偏好文档；不存在时保留首次默认策略。
    fn load_automatic_download(&self) -> Result<Option<bool>, PreferenceError> {
        let bytes = match fs::read(&self.path) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(PreferenceError::Storage),
        };
        let document: PreferenceDocument =
            serde_json::from_slice(&bytes).map_err(|_| PreferenceError::InvalidDocument)?;
        if document.version != PREFERENCE_VERSION {
            return Err(PreferenceError::InvalidDocument);
        }
        Ok(Some(document.automatic_download))
    }

    /// 同步临时文件后在同目录原子替换正式偏好文件。
    fn save_automatic_download(&mut self, enabled: bool) -> Result<(), PreferenceError> {
        let parent = self.path.parent().ok_or(PreferenceError::Storage)?;
        fs::create_dir_all(parent).map_err(|_| PreferenceError::Storage)?;
        let payload = serde_json::to_vec_pretty(&PreferenceDocument {
            version: PREFERENCE_VERSION,
            automatic_download: enabled,
        })
        .map_err(|_| PreferenceError::InvalidDocument)?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| PreferenceError::Storage)?;
        temporary
            .write_all(&payload)
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|_| PreferenceError::Storage)?;
        super::staging::atomic_replace_file(temporary.path(), &self.path)
            .map_err(|_| PreferenceError::Storage)?;
        sync_directory(parent)?;
        Ok(())
    }
}

/// 同步偏好文件所在目录，确保原子替换的目录项持久化。
#[cfg(unix)]
fn sync_directory(directory: &Path) -> Result<(), PreferenceError> {
    File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|_| PreferenceError::Storage)
}

/// Windows 的覆盖移动使用 WRITE_THROUGH，无需额外打开目录句柄。
#[cfg(windows)]
fn sync_directory(_directory: &Path) -> Result<(), PreferenceError> {
    Ok(())
}
