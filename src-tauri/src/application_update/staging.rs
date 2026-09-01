use semver::Version;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const STAGING_DIRECTORY: &str = "application-update";
const METADATA_FILE: &str = "metadata.json";
const METADATA_TEMP_FILE: &str = ".metadata.json.tmp";
const PACKAGE_FILE: &str = "package.bin";
const PACKAGE_TEMP_FILE: &str = ".package.bin.tmp";
const METADATA_SCHEMA_VERSION: u32 = 1;
const MAX_METADATA_BYTES: u64 = 64 * 1024;

/// 选择受信平台安装实现的持久化分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum InstallKind {
    /// macOS Developer ID、公证后重新打包的应用归档。
    MacosAppArchive,
    /// Windows current-user NSIS updater 可执行文件。
    WindowsNsis,
}

/// 更新包 Tauri minisign 签名验证的系统边界。
pub(crate) trait MinisignVerifier: Send + Sync {
    /// 使用编译进应用的公钥验证磁盘包及 manifest 签名字面量。
    fn verify(&self, package: &Path, signature: &str) -> Result<(), String>;
}

/// 当前进程用于恢复和安装复验的目标平台上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StagingContext {
    current_version: Version,
    target: String,
    install_kind: InstallKind,
}

impl StagingContext {
    /// 创建绑定当前运行版本、manifest target 与平台安装类型的上下文。
    pub(crate) fn new(
        current_version: &str,
        target: impl Into<String>,
        install_kind: InstallKind,
    ) -> Result<Self, StagingError> {
        let current_version =
            Version::parse(current_version).map_err(|_| StagingError::InvalidCandidate)?;
        let target = target.into();
        if target.trim().is_empty() {
            return Err(StagingError::InvalidCandidate);
        }
        Ok(Self {
            current_version,
            target,
            install_kind,
        })
    }
}

/// 准备进入暂存区的可信 Stable 候选元数据。
#[derive(Clone)]
pub(crate) struct StageCandidate {
    version: Version,
    signature: String,
    target: String,
    install_kind: InstallKind,
}

impl StageCandidate {
    /// 校验并创建绑定版本、签名、目标与安装类型的候选。
    pub(crate) fn new(
        version: &str,
        signature: impl Into<String>,
        target: impl Into<String>,
        install_kind: InstallKind,
    ) -> Result<Self, StagingError> {
        let version = Version::parse(version).map_err(|_| StagingError::InvalidCandidate)?;
        let signature = signature.into();
        let target = target.into();
        if !version.pre.is_empty() || signature.trim().is_empty() || target.trim().is_empty() {
            return Err(StagingError::InvalidCandidate);
        }
        Ok(Self {
            version,
            signature,
            target,
            install_kind,
        })
    }
}

impl fmt::Debug for StageCandidate {
    /// 输出候选身份并固定脱敏签名。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StageCandidate")
            .field("version", &self.version)
            .field("signature", &"[redacted]")
            .field("target", &self.target)
            .field("install_kind", &self.install_kind)
            .finish()
    }
}

/// 可安全公开给状态快照和日志的暂存更新摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StagedUpdate {
    version: String,
}

impl StagedUpdate {
    /// 返回暂存更新版本，不暴露签名或本机路径。
    pub(crate) fn version(&self) -> &str {
        &self.version
    }
}

/// 通过安装前复验、可交给受信安装适配器的包句柄。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct VerifiedStagedPackage {
    version: String,
    path: PathBuf,
    target: String,
    install_kind: InstallKind,
}

impl VerifiedStagedPackage {
    /// 返回已复验包的版本。
    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    /// 返回仅供受信安装适配器使用的本地包路径。
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// 返回下载时绑定且已再次匹配当前进程的 manifest target。
    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    /// 返回下载时绑定且已再次匹配当前进程的平台安装类型。
    pub(crate) fn install_kind(&self) -> InstallKind {
        self.install_kind
    }
}

impl fmt::Debug for VerifiedStagedPackage {
    /// 输出安装句柄身份并隐藏本机包路径。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("VerifiedStagedPackage")
            .field("version", &self.version)
            .field("path", &"[redacted]")
            .field("target", &self.target)
            .field("install_kind", &self.install_kind)
            .finish()
    }
}

/// 暂存仓储的稳定错误分类，不包含本机路径或签名。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagingError {
    InvalidCandidate,
    TargetMismatch,
    NotNewer,
    VerificationFailed,
    StorageFailed,
    NoStagedUpdate,
}

/// 磁盘上的私有暂存元数据。
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredMetadata {
    schema_version: u32,
    version: String,
    signature: String,
    target: String,
    install_kind: InstallKind,
}

/// 负责单个已验证更新包持久化、恢复、复验和清理的仓储。
pub(crate) struct StagingRepository<V> {
    root: PathBuf,
    verifier: V,
    context: StagingContext,
    staged: Option<StoredMetadata>,
    recovery_failure: Option<StagingError>,
}

impl<V: MinisignVerifier> StagingRepository<V> {
    /// 在注入的缓存根下打开仓储并恢复与当前进程匹配的已验证包。
    pub(crate) fn open(
        cache_root: impl AsRef<Path>,
        context: StagingContext,
        verifier: V,
    ) -> Result<Self, StagingError> {
        let root = cache_root.as_ref().join(STAGING_DIRECTORY);
        fs::create_dir_all(&root).map_err(|_| StagingError::StorageFailed)?;
        remove_if_exists(&root.join(PACKAGE_TEMP_FILE))?;
        remove_if_exists(&root.join(METADATA_TEMP_FILE))?;

        let recovery = recover_metadata(&root, &context, &verifier)?;
        Ok(Self {
            root,
            verifier,
            context,
            staged: recovery.metadata,
            recovery_failure: recovery.discarded,
        })
    }

    /// 返回不含文件路径与签名的当前暂存快照。
    pub(crate) fn snapshot(&self) -> Option<StagedUpdate> {
        self.staged.as_ref().map(|metadata| StagedUpdate {
            version: metadata.version.clone(),
        })
    }

    /// 返回恢复时已安全丢弃的损坏或不匹配原因，供上层发布诊断。
    pub(crate) fn recovery_failure(&self) -> Option<StagingError> {
        self.recovery_failure
    }

    /// 将输入流复制到临时文件，fsync 并验签后原子晋级为唯一暂存包与元数据。
    pub(crate) fn stage(
        &mut self,
        candidate: StageCandidate,
        mut package: impl Read,
    ) -> Result<StagedUpdate, StagingError> {
        self.validate_candidate(&candidate)?;
        let package_temp = self.root.join(PACKAGE_TEMP_FILE);
        let metadata_temp = self.root.join(METADATA_TEMP_FILE);
        remove_if_exists(&package_temp)?;
        remove_if_exists(&metadata_temp)?;

        let result = (|| {
            let mut output =
                File::create(&package_temp).map_err(|_| StagingError::StorageFailed)?;
            io::copy(&mut package, &mut output).map_err(|_| StagingError::StorageFailed)?;
            output.sync_all().map_err(|_| StagingError::StorageFailed)?;
            self.verifier
                .verify(&package_temp, &candidate.signature)
                .map_err(|_| StagingError::VerificationFailed)?;

            let metadata = StoredMetadata {
                schema_version: METADATA_SCHEMA_VERSION,
                version: candidate.version.to_string(),
                signature: candidate.signature,
                target: candidate.target,
                install_kind: candidate.install_kind,
            };
            write_metadata_temp(&metadata_temp, &metadata)?;
            atomic_replace_file(&package_temp, &self.root.join(PACKAGE_FILE))
                .map_err(|_| StagingError::StorageFailed)?;
            atomic_replace_file(&metadata_temp, &self.root.join(METADATA_FILE))
                .map_err(|_| StagingError::StorageFailed)?;
            sync_directory(&self.root)?;
            Ok(metadata)
        })();

        if let Err(error) = result {
            let _ = remove_if_exists(&package_temp);
            let _ = remove_if_exists(&metadata_temp);
            let recovered = recover_metadata(&self.root, &self.context, &self.verifier)?;
            self.staged = recovered.metadata;
            self.recovery_failure = recovered.discarded;
            return Err(error);
        }

        let metadata = result.expect("successful staging result must contain metadata");
        let staged = StagedUpdate {
            version: metadata.version.clone(),
        };
        self.staged = Some(metadata);
        self.recovery_failure = None;
        Ok(staged)
    }

    /// 在安装前重验版本、目标、安装类型与签名，失败时关闭信任并清除暂存。
    pub(crate) fn verify_for_install(&mut self) -> Result<VerifiedStagedPackage, StagingError> {
        let Some(metadata) = &self.staged else {
            return Err(StagingError::NoStagedUpdate);
        };
        let metadata_path = self.root.join(METADATA_FILE);
        let disk_metadata = read_metadata(&metadata_path);
        let validation = disk_metadata.and_then(|disk_metadata| {
            if &disk_metadata != metadata {
                return Err(StagingError::VerificationFailed);
            }
            validate_metadata(&disk_metadata, &self.context)?;
            Ok(disk_metadata)
        });
        if let Err(error) = validation {
            self.clear()?;
            return Err(error);
        }
        let metadata = validation.expect("validated metadata must be available");
        let package = self.root.join(PACKAGE_FILE);
        if self.verifier.verify(&package, &metadata.signature).is_err() {
            self.clear()?;
            return Err(StagingError::VerificationFailed);
        }
        Ok(VerifiedStagedPackage {
            version: metadata.version,
            path: package,
            target: metadata.target,
            install_kind: metadata.install_kind,
        })
    }

    /// 在受信安装适配器确认原路径替换后清除与句柄对应的暂存更新。
    pub(crate) fn complete_install(
        &mut self,
        installed: &VerifiedStagedPackage,
    ) -> Result<(), StagingError> {
        let Some(metadata) = &self.staged else {
            return Err(StagingError::NoStagedUpdate);
        };
        if installed.version != metadata.version
            || installed.path != self.root.join(PACKAGE_FILE)
            || installed.target != metadata.target
            || installed.install_kind != metadata.install_kind
        {
            return Err(StagingError::InvalidCandidate);
        }
        self.clear()
    }

    /// 在新进程版本达到暂存版本后确认 installer handoff 成功并清理。
    pub(crate) fn reconcile_current_version(
        &mut self,
        current_version: &str,
    ) -> Result<bool, StagingError> {
        let Some(metadata) = &self.staged else {
            return Ok(false);
        };
        let current =
            Version::parse(current_version).map_err(|_| StagingError::InvalidCandidate)?;
        let staged =
            Version::parse(&metadata.version).map_err(|_| StagingError::VerificationFailed)?;
        self.context.current_version = current.clone();
        if current < staged {
            return Ok(false);
        }
        self.clear()?;
        Ok(true)
    }

    /// 发现更高 Stable 时清除旧 staging，为唯一候选腾出可信槽位。
    pub(crate) fn discard_if_older_than(
        &mut self,
        replacement_version: &str,
    ) -> Result<bool, StagingError> {
        let Some(metadata) = &self.staged else {
            return Ok(false);
        };
        let replacement =
            Version::parse(replacement_version).map_err(|_| StagingError::InvalidCandidate)?;
        let staged =
            Version::parse(&metadata.version).map_err(|_| StagingError::VerificationFailed)?;
        if replacement <= staged {
            return Ok(false);
        }
        self.clear()?;
        Ok(true)
    }

    /// 验证候选严格升级且绑定当前目标与平台安装类型。
    fn validate_candidate(&self, candidate: &StageCandidate) -> Result<(), StagingError> {
        if candidate.target != self.context.target
            || candidate.install_kind != self.context.install_kind
        {
            return Err(StagingError::TargetMismatch);
        }
        if candidate.version <= self.context.current_version {
            return Err(StagingError::NotNewer);
        }
        if let Some(current) = &self.staged {
            let current =
                Version::parse(&current.version).map_err(|_| StagingError::VerificationFailed)?;
            if candidate.version <= current {
                return Err(StagingError::NotNewer);
            }
        }
        Ok(())
    }

    /// 删除包、元数据与临时文件并同步公开快照。
    fn clear(&mut self) -> Result<(), StagingError> {
        remove_if_exists(&self.root.join(METADATA_FILE))?;
        remove_if_exists(&self.root.join(PACKAGE_FILE))?;
        remove_if_exists(&self.root.join(METADATA_TEMP_FILE))?;
        remove_if_exists(&self.root.join(PACKAGE_TEMP_FILE))?;
        sync_directory(&self.root)?;
        self.staged = None;
        Ok(())
    }
}

/// 汇总一次恢复得到的可信元数据和可报告丢弃原因。
struct RecoveryResult {
    metadata: Option<StoredMetadata>,
    discarded: Option<StagingError>,
}

/// 从磁盘恢复元数据，并重新校验版本、目标与完整包签名。
fn recover_metadata<V: MinisignVerifier>(
    root: &Path,
    context: &StagingContext,
    verifier: &V,
) -> Result<RecoveryResult, StagingError> {
    let metadata_path = root.join(METADATA_FILE);
    let package_path = root.join(PACKAGE_FILE);
    if !metadata_path.is_file() || !package_path.is_file() {
        let incomplete = metadata_path.exists() || package_path.exists();
        remove_if_exists(&metadata_path)?;
        remove_if_exists(&package_path)?;
        return Ok(RecoveryResult {
            metadata: None,
            discarded: incomplete.then_some(StagingError::StorageFailed),
        });
    }

    let recovered = read_metadata(&metadata_path).and_then(|metadata| {
        validate_metadata(&metadata, context)?;
        verifier
            .verify(&package_path, &metadata.signature)
            .map_err(|_| StagingError::VerificationFailed)?;
        Ok(metadata)
    });
    match recovered {
        Ok(metadata) => Ok(RecoveryResult {
            metadata: Some(metadata),
            discarded: None,
        }),
        Err(error) => {
            remove_if_exists(&metadata_path)?;
            remove_if_exists(&package_path)?;
            sync_directory(root)?;
            Ok(RecoveryResult {
                metadata: None,
                // 运行版本已经达到 staging 是一次成功的 installer reconciliation，不是损坏。
                discarded: (error != StagingError::NotNewer).then_some(error),
            })
        }
    }
}

/// 有界读取并反序列化版本化元数据，避免恶意文件造成无界内存分配。
fn read_metadata(path: &Path) -> Result<StoredMetadata, StagingError> {
    let file = File::open(path).map_err(|_| StagingError::StorageFailed)?;
    let mut bytes = Vec::new();
    file.take(MAX_METADATA_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| StagingError::StorageFailed)?;
    if bytes.len() as u64 > MAX_METADATA_BYTES {
        return Err(StagingError::VerificationFailed);
    }
    serde_json::from_slice(&bytes).map_err(|_| StagingError::VerificationFailed)
}

/// 校验恢复或安装前元数据仍属于当前运行版本和唯一目标。
fn validate_metadata(
    metadata: &StoredMetadata,
    context: &StagingContext,
) -> Result<(), StagingError> {
    if metadata.schema_version != METADATA_SCHEMA_VERSION {
        return Err(StagingError::VerificationFailed);
    }
    if metadata.target != context.target || metadata.install_kind != context.install_kind {
        return Err(StagingError::TargetMismatch);
    }
    let version =
        Version::parse(&metadata.version).map_err(|_| StagingError::VerificationFailed)?;
    if version <= context.current_version {
        return Err(StagingError::NotNewer);
    }
    if metadata.signature.trim().is_empty() {
        return Err(StagingError::VerificationFailed);
    }
    Ok(())
}

/// 把元数据完整写入并同步临时文件，等待与包一起晋级。
fn write_metadata_temp(path: &Path, metadata: &StoredMetadata) -> Result<(), StagingError> {
    let bytes = serde_json::to_vec(metadata).map_err(|_| StagingError::StorageFailed)?;
    let mut file = File::create(path).map_err(|_| StagingError::StorageFailed)?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| StagingError::StorageFailed)
}

/// 在支持的平台上以覆盖语义原子替换同一目录中的目标文件。
#[cfg(not(windows))]
pub(super) fn atomic_replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    fs::rename(source, destination)
}

/// 在 Windows 上使用系统覆盖语义和 WRITE_THROUGH 原子替换同卷文件。
#[cfg(windows)]
pub(super) fn atomic_replace_file(source: &Path, destination: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let moved = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if moved == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// 在 Unix 上同步目录项，确保原子晋级在崩溃恢复后仍可见。
#[cfg(unix)]
fn sync_directory(directory: &Path) -> Result<(), StagingError> {
    File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|_| StagingError::StorageFailed)
}

/// Windows 文件替换已经请求 WRITE_THROUGH，无需额外目录句柄。
#[cfg(windows)]
fn sync_directory(_directory: &Path) -> Result<(), StagingError> {
    Ok(())
}

/// 删除存在的仓储文件，同时把不存在视为成功。
fn remove_if_exists(path: &Path) -> Result<(), StagingError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(StagingError::StorageFailed),
    }
}
