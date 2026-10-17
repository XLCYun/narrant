use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs;
#[cfg(unix)]
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const PREFERENCE_VERSION: u32 = 1;

/// 为自动下载设置提供最小持久化边界。
pub(crate) trait PreferenceStore {
    /// 读取自动下载设置；缺失表示采用默认开启。
    fn load_automatic_download(&self) -> Result<Option<bool>, PreferenceError>;

    /// 持久化自动下载设置，并明确返回正式文件所处的提交阶段。
    fn save_automatic_download(&mut self, enabled: bool) -> PreferenceSaveOutcome;
}

/// 描述一次偏好保存后正式文件与崩溃持久性的可信事实。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreferenceSaveOutcome {
    /// 正式文件尚未替换，旧值仍是权威值。
    Rejected { error: PreferenceError },
    /// 正式文件已替换且父目录持久化屏障完成。
    Committed { applied_value: bool },
    /// 正式文件当前已是新值，但父目录屏障失败，崩溃恢复结果未知。
    DurabilityUnknown {
        applied_value: bool,
        error: PreferenceError,
    },
    /// 仓储无法确认正式文件的当前值，调用方必须保守保留旧策略并展示 warning。
    StateUnknown { error: PreferenceError },
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

/// 偏好文件提交测试使用的两个确定性故障点。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreferenceCommitPoint {
    BeforeReplace,
    AfterReplaceBeforeDirectorySync,
}

/// 在真实临时文件、rename 与目录屏障之间注入确定性故障。
pub(crate) trait PreferenceFaultInjector: Send + Sync {
    /// 指定提交点成功返回，或返回稳定、无路径的存储错误。
    fn checkpoint(&self, point: PreferenceCommitPoint) -> Result<(), PreferenceError>;
}

/// 生产环境不注入故障，只执行真实文件系统操作。
struct NoPreferenceFaults;

impl PreferenceFaultInjector for NoPreferenceFaults {
    /// 生产提交点始终继续执行。
    fn checkpoint(&self, _point: PreferenceCommitPoint) -> Result<(), PreferenceError> {
        Ok(())
    }
}

/// 把自动下载偏好原子保存到独立 JSON 文件的生产仓储。
pub(crate) struct FilePreferenceStore {
    path: PathBuf,
    faults: Arc<dyn PreferenceFaultInjector>,
}

impl FilePreferenceStore {
    /// 创建一个仅持有 Application Update 偏好的文件仓储。
    pub(crate) fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            faults: Arc::new(NoPreferenceFaults),
        }
    }

    /// 创建使用真实文件系统、但可在精确提交边界注入故障的测试仓储。
    #[cfg(test)]
    pub(crate) fn with_faults(
        path: impl Into<PathBuf>,
        faults: impl PreferenceFaultInjector + 'static,
    ) -> Self {
        Self {
            path: path.into(),
            faults: Arc::new(faults),
        }
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
    fn save_automatic_download(&mut self, enabled: bool) -> PreferenceSaveOutcome {
        let Some(parent) = self.path.parent() else {
            return PreferenceSaveOutcome::Rejected {
                error: PreferenceError::Storage,
            };
        };
        if fs::create_dir_all(parent).is_err() {
            return PreferenceSaveOutcome::Rejected {
                error: PreferenceError::Storage,
            };
        }
        let Ok(payload) = serde_json::to_vec_pretty(&PreferenceDocument {
            version: PREFERENCE_VERSION,
            automatic_download: enabled,
        }) else {
            return PreferenceSaveOutcome::Rejected {
                error: PreferenceError::InvalidDocument,
            };
        };
        let Ok(mut temporary) = tempfile::NamedTempFile::new_in(parent) else {
            return PreferenceSaveOutcome::Rejected {
                error: PreferenceError::Storage,
            };
        };
        if temporary
            .write_all(&payload)
            .and_then(|_| temporary.as_file().sync_all())
            .is_err()
            || self
                .faults
                .checkpoint(PreferenceCommitPoint::BeforeReplace)
                .is_err()
        {
            return PreferenceSaveOutcome::Rejected {
                error: PreferenceError::Storage,
            };
        }
        if super::staging::atomic_replace_file(temporary.path(), &self.path).is_err() {
            return PreferenceSaveOutcome::Rejected {
                error: PreferenceError::Storage,
            };
        }
        if let Err(error) = self
            .faults
            .checkpoint(PreferenceCommitPoint::AfterReplaceBeforeDirectorySync)
            .and_then(|_| sync_directory(parent))
        {
            return PreferenceSaveOutcome::DurabilityUnknown {
                applied_value: enabled,
                error,
            };
        }
        PreferenceSaveOutcome::Committed {
            applied_value: enabled,
        }
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
