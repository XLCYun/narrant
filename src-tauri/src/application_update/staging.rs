use fs2::FileExt;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

const STAGING_DIRECTORY: &str = "application-update";
const LOCK_FILE: &str = ".lock";
const COMMIT_POINTER: &str = "current";
const GENERATION_PREFIX: &str = "generation-";
const POINTER_TEMP_PREFIX: &str = ".current-";
const METADATA_FILE: &str = "metadata.json";
const PACKAGE_FILE: &str = "package.bin";
const METADATA_SCHEMA_VERSION: u32 = 2;
const MAX_METADATA_BYTES: u64 = 64 * 1024;
const MAX_POINTER_BYTES: u64 = 256;

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
    release_notes: String,
}

impl StageCandidate {
    /// 校验并创建绑定版本、签名、目标、安装类型与有界 Release Notes 的候选。
    pub(crate) fn new(
        version: &str,
        signature: impl Into<String>,
        target: impl Into<String>,
        install_kind: InstallKind,
        release_notes: impl AsRef<str>,
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
            release_notes: super::bounded_release_notes(release_notes.as_ref()),
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
            .field("release_notes_bytes", &self.release_notes.len())
            .finish()
    }
}

/// 可安全公开给状态快照和日志的暂存更新摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StagedUpdate {
    version: String,
    release_notes: String,
}

impl StagedUpdate {
    /// 返回暂存更新版本，不暴露签名或本机路径。
    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    /// 返回与暂存候选一起持久化的有界 Release Notes。
    pub(crate) fn release_notes(&self) -> &str {
        &self.release_notes
    }
}

/// 通过安装前复验、可交给受信安装适配器的包句柄。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct VerifiedStagedPackage {
    generation: String,
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
    /// 输出安装句柄身份并隐藏 generation 与本机包路径。
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

/// generation 提交协议中可注入故障的持久化边界。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagingCommitPoint {
    AfterPackageSync,
    AfterVerification,
    AfterMetadataSync,
    AfterGenerationSync,
    BeforePointerCommit,
    AfterPointerCommit,
    AfterPointerSync,
}

/// 为真实文件系统测试提供确定性崩溃/失败点，不参与业务决策。
pub(crate) trait StagingFaultInjector: Send + Sync {
    /// 在指定提交点返回存储失败以模拟进程中断或系统调用失败。
    fn checkpoint(&self, point: StagingCommitPoint) -> Result<(), StagingError>;
}

/// 生产 staging 使用的不注入故障实现。
pub(crate) struct NoStagingFaults;

impl StagingFaultInjector for NoStagingFaults {
    /// 生产路径在所有提交点继续执行。
    fn checkpoint(&self, _point: StagingCommitPoint) -> Result<(), StagingError> {
        Ok(())
    }
}

/// 磁盘上的私有暂存元数据。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredMetadata {
    schema_version: u32,
    version: String,
    signature: String,
    target: String,
    install_kind: InstallKind,
    #[serde(default)]
    release_notes: String,
}

/// 一个由 commit pointer 原子选中的完整 generation。
#[derive(Clone)]
struct CommittedGeneration {
    name: String,
    metadata: StoredMetadata,
}

/// 负责单个已验证更新 generation 持久化、恢复、复验和清理的仓储。
pub(crate) struct StagingRepository<V, F = NoStagingFaults> {
    root: PathBuf,
    verifier: V,
    faults: F,
    context: StagingContext,
    staged: Option<CommittedGeneration>,
    recovery_failure: Option<StagingError>,
}

impl<V: MinisignVerifier> StagingRepository<V, NoStagingFaults> {
    /// 使用生产无故障协议打开仓储。
    pub(crate) fn open(
        cache_root: impl AsRef<Path>,
        context: StagingContext,
        verifier: V,
    ) -> Result<Self, StagingError> {
        Self::open_inner(cache_root, context, verifier, NoStagingFaults)
    }
}

impl<V: MinisignVerifier, F: StagingFaultInjector> StagingRepository<V, F> {
    /// 使用注入故障点打开仓储，供真实文件系统事务测试复现崩溃窗口。
    #[cfg(test)]
    pub(crate) fn open_with_faults(
        cache_root: impl AsRef<Path>,
        context: StagingContext,
        verifier: V,
        faults: F,
    ) -> Result<Self, StagingError> {
        Self::open_inner(cache_root, context, verifier, faults)
    }

    /// 建立仓储并在文件锁内完成首次恢复，生产与故障测试共享同一实现。
    fn open_inner(
        cache_root: impl AsRef<Path>,
        context: StagingContext,
        verifier: V,
        faults: F,
    ) -> Result<Self, StagingError> {
        let root = cache_root.as_ref().join(STAGING_DIRECTORY);
        fs::create_dir_all(&root).map_err(|_| StagingError::StorageFailed)?;
        let mut repository = Self {
            root,
            verifier,
            faults,
            context,
            staged: None,
            recovery_failure: None,
        };
        let _lock = lock_root(&repository.root)?;
        repository.refresh_locked()?;
        Ok(repository)
    }

    /// 返回不含文件路径与签名的当前进程暂存快照。
    pub(crate) fn snapshot(&self) -> Option<StagedUpdate> {
        self.staged.as_ref().map(|generation| StagedUpdate {
            version: generation.metadata.version.clone(),
            release_notes: generation.metadata.release_notes.clone(),
        })
    }

    /// 返回恢复时已安全丢弃的损坏或不匹配原因，供上层发布诊断。
    pub(crate) fn recovery_failure(&self) -> Option<StagingError> {
        self.recovery_failure
    }

    /// 在跨进程锁内流式写入唯一 generation，并以单一 pointer 原子提交更高候选。
    pub(crate) fn stage(
        &mut self,
        candidate: StageCandidate,
        mut package: impl Read,
    ) -> Result<StagedUpdate, StagingError> {
        let _lock = lock_root(&self.root)?;
        self.refresh_locked()?;
        self.validate_candidate(&candidate)?;
        let result = self.commit_generation_locked(candidate, &mut package);
        if result.is_err() {
            // 无论失败发生在 pointer 前后，都按唯一 pointer 恢复；旧或新 generation 至少一个可用。
            self.refresh_locked()?;
        }
        result
    }

    /// 在同一锁内原子回填同版本 Release Notes，不修改已验证包或 commit pointer。
    pub(crate) fn update_release_notes(
        &mut self,
        version: &str,
        release_notes: &str,
    ) -> Result<bool, StagingError> {
        let _lock = lock_root(&self.root)?;
        self.refresh_locked()?;
        let Some(committed) = &mut self.staged else {
            return Ok(false);
        };
        if committed.metadata.version != version {
            return Ok(false);
        }
        let notes = super::bounded_release_notes(release_notes);
        if committed.metadata.release_notes == notes {
            return Ok(false);
        }
        let mut metadata = committed.metadata.clone();
        metadata.release_notes = notes;
        let generation_path = self.root.join(&committed.name);
        write_metadata_atomic(&generation_path, &metadata)?;
        committed.metadata = metadata;
        Ok(true)
    }

    /// 在安装前锁内重读 pointer，并重验版本、目标、安装类型与包签名。
    pub(crate) fn verify_for_install(&mut self) -> Result<VerifiedStagedPackage, StagingError> {
        let _lock = lock_root(&self.root)?;
        self.refresh_locked()?;
        let Some(committed) = &self.staged else {
            return Err(self
                .recovery_failure
                .unwrap_or(StagingError::NoStagedUpdate));
        };
        let package = self.root.join(&committed.name).join(PACKAGE_FILE);
        self.verifier
            .verify(&package, &committed.metadata.signature)
            .map_err(|_| StagingError::VerificationFailed)?;
        Ok(VerifiedStagedPackage {
            generation: committed.name.clone(),
            version: committed.metadata.version.clone(),
            path: package,
            target: committed.metadata.target.clone(),
            install_kind: committed.metadata.install_kind,
        })
    }

    /// 在受信安装适配器确认原路径替换后，仅清除句柄对应的 committed generation。
    pub(crate) fn complete_install(
        &mut self,
        installed: &VerifiedStagedPackage,
    ) -> Result<(), StagingError> {
        let _lock = lock_root(&self.root)?;
        self.refresh_locked()?;
        let Some(committed) = &self.staged else {
            return Err(StagingError::NoStagedUpdate);
        };
        if installed.generation != committed.name
            || installed.version != committed.metadata.version
            || installed.path != self.root.join(&committed.name).join(PACKAGE_FILE)
            || installed.target != committed.metadata.target
            || installed.install_kind != committed.metadata.install_kind
        {
            return Err(StagingError::InvalidCandidate);
        }
        self.clear_committed_locked()
    }

    /// 在新进程版本达到暂存版本后确认 installer handoff 成功并清理。
    pub(crate) fn reconcile_current_version(
        &mut self,
        current_version: &str,
    ) -> Result<bool, StagingError> {
        let current =
            Version::parse(current_version).map_err(|_| StagingError::InvalidCandidate)?;
        let _lock = lock_root(&self.root)?;
        let had_staging = read_pointer(&self.root)?.is_some();
        self.context.current_version = current;
        self.refresh_locked()?;
        Ok(had_staging && self.staged.is_none())
    }

    /// 将候选写入独立 generation，并在最后一步原子切换 commit pointer。
    fn commit_generation_locked(
        &mut self,
        candidate: StageCandidate,
        package: &mut impl Read,
    ) -> Result<StagedUpdate, StagingError> {
        let generation = tempfile::Builder::new()
            .prefix(GENERATION_PREFIX)
            .tempdir_in(&self.root)
            .map_err(|_| StagingError::StorageFailed)?;
        let package_path = generation.path().join(PACKAGE_FILE);
        let mut output = File::create(&package_path).map_err(|_| StagingError::StorageFailed)?;
        io::copy(package, &mut output).map_err(|_| StagingError::StorageFailed)?;
        output.sync_all().map_err(|_| StagingError::StorageFailed)?;
        self.faults
            .checkpoint(StagingCommitPoint::AfterPackageSync)?;
        self.verifier
            .verify(&package_path, &candidate.signature)
            .map_err(|_| StagingError::VerificationFailed)?;
        self.faults
            .checkpoint(StagingCommitPoint::AfterVerification)?;

        let metadata = StoredMetadata {
            schema_version: METADATA_SCHEMA_VERSION,
            version: candidate.version.to_string(),
            signature: candidate.signature,
            target: candidate.target,
            install_kind: candidate.install_kind,
            release_notes: candidate.release_notes,
        };
        write_metadata_file(&generation.path().join(METADATA_FILE), &metadata)?;
        self.faults
            .checkpoint(StagingCommitPoint::AfterMetadataSync)?;
        sync_directory(generation.path())?;
        self.faults
            .checkpoint(StagingCommitPoint::AfterGenerationSync)?;

        let generation_path = generation.keep();
        let generation_name = generation_path
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| valid_generation_name(name))
            .ok_or(StagingError::StorageFailed)?
            .to_owned();
        self.faults
            .checkpoint(StagingCommitPoint::BeforePointerCommit)?;
        write_commit_pointer(&self.root, &generation_name)?;
        self.faults
            .checkpoint(StagingCommitPoint::AfterPointerCommit)?;
        sync_directory(&self.root)?;
        self.faults
            .checkpoint(StagingCommitPoint::AfterPointerSync)?;

        cleanup_uncommitted(&self.root, Some(&generation_name))?;
        self.staged = Some(CommittedGeneration {
            name: generation_name,
            metadata: metadata.clone(),
        });
        self.recovery_failure = None;
        Ok(StagedUpdate {
            version: metadata.version,
            release_notes: metadata.release_notes,
        })
    }

    /// 提交前使用锁内最新 pointer 执行严格 SemVer CAS 与目标校验。
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
            let current = Version::parse(&current.metadata.version)
                .map_err(|_| StagingError::VerificationFailed)?;
            if candidate.version <= current {
                return Err(StagingError::NotNewer);
            }
        }
        Ok(())
    }

    /// 在已持有跨进程锁时按 commit pointer 刷新本实例权威视图。
    fn refresh_locked(&mut self) -> Result<(), StagingError> {
        let recovery = recover_committed(&self.root, &self.context, &self.verifier)?;
        self.staged = recovery.committed;
        self.recovery_failure = recovery.discarded;
        Ok(())
    }

    /// 原子删除 commit pointer 后清理所有 generation，保证崩溃恢复不再安装。
    fn clear_committed_locked(&mut self) -> Result<(), StagingError> {
        remove_if_exists(&self.root.join(COMMIT_POINTER))?;
        sync_directory(&self.root)?;
        cleanup_uncommitted(&self.root, None)?;
        self.staged = None;
        Ok(())
    }
}

/// 跨实例/进程持有 staging 根独占文件锁的 RAII 句柄。
struct StagingLock {
    file: File,
}

impl Drop for StagingLock {
    /// 离开唯一事务临界区时释放操作系统文件锁。
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

/// 打开并独占 staging 根锁文件，macOS 与 Windows 使用 fs2 平台实现。
fn lock_root(root: &Path) -> Result<StagingLock, StagingError> {
    let file = OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .open(root.join(LOCK_FILE))
        .map_err(|_| StagingError::StorageFailed)?;
    FileExt::lock_exclusive(&file).map_err(|_| StagingError::StorageFailed)?;
    Ok(StagingLock { file })
}

/// 汇总一次恢复得到的 committed generation 和可报告丢弃原因。
struct RecoveryResult {
    committed: Option<CommittedGeneration>,
    discarded: Option<StagingError>,
}

/// 恢复 commit pointer 指向的唯一 generation，并清除所有未提交 generation。
fn recover_committed<V: MinisignVerifier>(
    root: &Path,
    context: &StagingContext,
    verifier: &V,
) -> Result<RecoveryResult, StagingError> {
    let pointer = match read_pointer(root) {
        Ok(pointer) => pointer,
        Err(error) => {
            remove_if_exists(&root.join(COMMIT_POINTER))?;
            cleanup_uncommitted(root, None)?;
            return Ok(RecoveryResult {
                committed: None,
                discarded: Some(error),
            });
        }
    };
    let Some(generation_name) = pointer else {
        let incomplete = contains_generation(root)?;
        cleanup_uncommitted(root, None)?;
        return Ok(RecoveryResult {
            committed: None,
            discarded: incomplete.then_some(StagingError::StorageFailed),
        });
    };

    let generation_path = root.join(&generation_name);
    let metadata_path = generation_path.join(METADATA_FILE);
    let package_path = generation_path.join(PACKAGE_FILE);
    let recovered = read_metadata(&metadata_path).and_then(|metadata| {
        validate_metadata(&metadata, context)?;
        verifier
            .verify(&package_path, &metadata.signature)
            .map_err(|_| StagingError::VerificationFailed)?;
        Ok(metadata)
    });
    match recovered {
        Ok(metadata) => {
            cleanup_uncommitted(root, Some(&generation_name))?;
            Ok(RecoveryResult {
                committed: Some(CommittedGeneration {
                    name: generation_name,
                    metadata,
                }),
                discarded: None,
            })
        }
        Err(error) => {
            remove_if_exists(&root.join(COMMIT_POINTER))?;
            sync_directory(root)?;
            cleanup_uncommitted(root, None)?;
            Ok(RecoveryResult {
                committed: None,
                discarded: (error != StagingError::NotNewer).then_some(error),
            })
        }
    }
}

/// 有界读取并验证原子 commit pointer 只引用本目录 generation 名称。
fn read_pointer(root: &Path) -> Result<Option<String>, StagingError> {
    let path = root.join(COMMIT_POINTER);
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(StagingError::StorageFailed),
    };
    let mut bytes = Vec::new();
    file.take(MAX_POINTER_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| StagingError::StorageFailed)?;
    if bytes.len() as u64 > MAX_POINTER_BYTES {
        return Err(StagingError::VerificationFailed);
    }
    let name = std::str::from_utf8(&bytes).map_err(|_| StagingError::VerificationFailed)?;
    if !valid_generation_name(name) {
        return Err(StagingError::VerificationFailed);
    }
    Ok(Some(name.to_owned()))
}

/// 校验 pointer 名称不包含路径分隔符或父目录跳转。
fn valid_generation_name(name: &str) -> bool {
    name.starts_with(GENERATION_PREFIX)
        && name.len() > GENERATION_PREFIX.len()
        && name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

/// 原子写入并 fsync 唯一 commit pointer；父目录同步由调用方完成。
fn write_commit_pointer(root: &Path, generation: &str) -> Result<(), StagingError> {
    let mut temporary = tempfile::Builder::new()
        .prefix(POINTER_TEMP_PREFIX)
        .tempfile_in(root)
        .map_err(|_| StagingError::StorageFailed)?;
    temporary
        .write_all(generation.as_bytes())
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|_| StagingError::StorageFailed)?;
    atomic_replace_file(temporary.path(), &root.join(COMMIT_POINTER))
        .map_err(|_| StagingError::StorageFailed)
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
    if !version.pre.is_empty() || version <= context.current_version {
        return Err(StagingError::NotNewer);
    }
    if metadata.signature.trim().is_empty()
        || metadata.release_notes.len() > super::MAX_RELEASE_NOTES_BYTES
    {
        return Err(StagingError::VerificationFailed);
    }
    Ok(())
}

/// 写入并同步 generation 内首次创建的 metadata 文件。
fn write_metadata_file(path: &Path, metadata: &StoredMetadata) -> Result<(), StagingError> {
    let bytes = serde_json::to_vec(metadata).map_err(|_| StagingError::StorageFailed)?;
    let mut file = File::create(path).map_err(|_| StagingError::StorageFailed)?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| StagingError::StorageFailed)
}

/// 在已提交 generation 内原子回填 metadata，并同步 generation 目录。
fn write_metadata_atomic(generation: &Path, metadata: &StoredMetadata) -> Result<(), StagingError> {
    let mut temporary =
        tempfile::NamedTempFile::new_in(generation).map_err(|_| StagingError::StorageFailed)?;
    let bytes = serde_json::to_vec(metadata).map_err(|_| StagingError::StorageFailed)?;
    temporary
        .write_all(&bytes)
        .and_then(|_| temporary.as_file().sync_all())
        .map_err(|_| StagingError::StorageFailed)?;
    atomic_replace_file(temporary.path(), &generation.join(METADATA_FILE))
        .map_err(|_| StagingError::StorageFailed)?;
    sync_directory(generation)
}

/// 清理 pointer 未引用的 generation 与 pointer 临时文件，并持久化删除目录项。
fn cleanup_uncommitted(root: &Path, active: Option<&str>) -> Result<(), StagingError> {
    let mut changed = false;
    for entry in fs::read_dir(root).map_err(|_| StagingError::StorageFailed)? {
        let entry = entry.map_err(|_| StagingError::StorageFailed)?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let path = entry.path();
        if entry
            .file_type()
            .map_err(|_| StagingError::StorageFailed)?
            .is_dir()
            && name.starts_with(GENERATION_PREFIX)
            && active != Some(name.as_ref())
        {
            fs::remove_dir_all(path).map_err(|_| StagingError::StorageFailed)?;
            changed = true;
        } else if name.starts_with(POINTER_TEMP_PREFIX) {
            remove_if_exists(&path)?;
            changed = true;
        }
    }
    if changed {
        sync_directory(root)?;
    }
    Ok(())
}

/// 判断 root 是否含有 pointer 未提交或崩溃遗留的 generation。
fn contains_generation(root: &Path) -> Result<bool, StagingError> {
    for entry in fs::read_dir(root).map_err(|_| StagingError::StorageFailed)? {
        let entry = entry.map_err(|_| StagingError::StorageFailed)?;
        if entry
            .file_type()
            .map_err(|_| StagingError::StorageFailed)?
            .is_dir()
            && entry
                .file_name()
                .to_string_lossy()
                .starts_with(GENERATION_PREFIX)
        {
            return Ok(true);
        }
    }
    Ok(false)
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

/// 在 Unix 上同步目录项，确保 pointer 与清理在崩溃恢复后仍可见。
#[cfg(unix)]
fn sync_directory(directory: &Path) -> Result<(), StagingError> {
    File::open(directory)
        .and_then(|file| file.sync_all())
        .map_err(|_| StagingError::StorageFailed)
}

/// Windows 原子覆盖已请求 WRITE_THROUGH；目录句柄不能按 Unix 方式同步。
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
