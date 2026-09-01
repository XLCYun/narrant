use super::{InstallError, InstallIntent, InstallOutcome, UpdateInstaller};
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
const DURABILITY_MARKER: &str = "pending-durability";
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

/// 只有完整提交或 durability barrier 才能产生的不可伪造暂存证明。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DurableStagedUpdate {
    version: String,
    release_notes: String,
    target: String,
    install_kind: InstallKind,
}

/// 描述本次 generation 提交在当前进程可观察到的权威结果。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StageCommitOutcome {
    /// `current` 已切换并成功同步 staging 根目录。
    Committed(DurableStagedUpdate),
    /// `current` 已精确指向新 generation，但父目录同步结果不确定。
    DurabilityUnknown(PendingStagedUpdate),
}

/// 尚未跨过 staging 根目录 durability barrier 的不可安装候选身份。
#[derive(PartialEq, Eq)]
pub(crate) struct PendingStagedUpdate {
    generation: String,
    version: String,
    release_notes: String,
}

impl PendingStagedUpdate {
    /// 返回仅供失败/确认界面展示的候选版本，不代表已经可安装。
    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    /// 返回仅供失败/确认界面展示的有界 Release Notes。
    pub(crate) fn release_notes(&self) -> &str {
        &self.release_notes
    }
}

impl fmt::Debug for PendingStagedUpdate {
    /// 调试输出隐藏内部 generation identity，只展示不可安装候选摘要。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PendingStagedUpdate")
            .field("version", &self.version)
            .field("release_notes_bytes", &self.release_notes.len())
            .finish()
    }
}

impl DurableStagedUpdate {
    /// 返回暂存更新版本，不暴露签名或本机路径。
    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    /// 返回与暂存候选一起持久化的有界 Release Notes。
    pub(crate) fn release_notes(&self) -> &str {
        &self.release_notes
    }

    /// 返回已由仓储上下文验证的目标平台。
    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    /// 返回已由仓储上下文验证的平台安装类型。
    pub(crate) fn install_kind(&self) -> InstallKind {
        self.install_kind
    }

    /// 仅为状态机单元测试创建 opaque durable proof；生产构建不存在该入口。
    #[cfg(test)]
    pub(crate) fn test_fixture(
        version: impl Into<String>,
        release_notes: impl AsRef<str>,
        target: impl Into<String>,
        install_kind: InstallKind,
    ) -> Self {
        Self {
            version: version.into(),
            release_notes: super::bounded_release_notes(release_notes.as_ref()),
            target: target.into(),
            install_kind,
        }
    }
}

/// 通过安装前复验、可交给受信安装适配器的包句柄。
#[derive(PartialEq, Eq)]
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
    DurabilityUnconfirmed,
}

/// 平台安装主结果之后的 staging 清理状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallCleanupDisposition {
    /// pointer 与 generation 均已清理完成。
    Complete,
    /// 清理尚未完成，但新进程可通过版本 reconciliation 安全重试。
    Pending,
    /// pointer 删除已发生，但 staging 根目录同步结果不确定。
    DurabilityUnknown,
}

/// 保留不可逆平台安装主结果并独立携带可恢复 cleanup 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InstallExecutionOutcome {
    /// 当前进程已替换应用；cleanup 失败不得改变这个主事实。
    Installed { cleanup: InstallCleanupDisposition },
    /// 外部安装器已安全 open/launch，staging 留给新版本 reconciliation。
    HandedOff,
}

/// 汇总锁内复验或平台安装器执行产生的稳定错误。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum InstallAttemptError {
    /// staging 恢复、复验或收尾失败。
    Staging(StagingError),
    /// 平台安装器拒绝消费已复验包。
    Installer(InstallError),
}

impl From<StagingError> for InstallAttemptError {
    /// 将 staging 错误提升到完整安装尝试错误。
    fn from(error: StagingError) -> Self {
        Self::Staging(error)
    }
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
    DurabilityConfirmationSync,
    RefreshInspect,
    RefreshVerify,
    RefreshDurabilitySync,
    RefreshCleanup,
    BeforeInstallPointerDelete,
    AfterInstallPointerDelete,
    BeforeInstallGenerationCleanup,
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

/// 从已经验证且由 current pointer 选中的 generation 生成 opaque durable proof。
fn durable_proof(committed: &CommittedGeneration) -> DurableStagedUpdate {
    DurableStagedUpdate {
        version: committed.metadata.version.clone(),
        release_notes: committed.metadata.release_notes.clone(),
        target: committed.metadata.target.clone(),
        install_kind: committed.metadata.install_kind,
    }
}

/// 负责单个已验证更新 generation 持久化、恢复、复验和清理的仓储。
pub(crate) struct StagingRepository<V, F = NoStagingFaults> {
    root: PathBuf,
    verifier: V,
    faults: F,
    context: StagingContext,
    staged: Option<CommittedGeneration>,
    durability_unknown: Option<String>,
    installed_generation: Option<String>,
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
            durability_unknown: None,
            installed_generation: None,
            recovery_failure: None,
        };
        let _lock = lock_root(&repository.root)?;
        repository.refresh_locked()?;
        repository.durability_unknown = None;
        Ok(repository)
    }

    /// 返回不含文件路径与签名、且已经证明 durable 的当前暂存快照。
    pub(crate) fn snapshot(&self) -> Option<DurableStagedUpdate> {
        self.staged
            .as_ref()
            .filter(|generation| {
                self.durability_unknown.as_deref() != Some(&generation.name)
                    && self.installed_generation.as_deref() != Some(&generation.name)
            })
            .map(durable_proof)
    }

    /// 返回当前实例尚未确认的 generation 身份；新实例会先执行恢复 barrier。
    pub(crate) fn pending_durability(&self) -> Option<PendingStagedUpdate> {
        let generation = self.durability_unknown.as_ref()?;
        let committed = self
            .staged
            .as_ref()
            .filter(|committed| &committed.name == generation)?;
        Some(PendingStagedUpdate {
            generation: generation.clone(),
            version: committed.metadata.version.clone(),
            release_notes: committed.metadata.release_notes.clone(),
        })
    }

    /// 对指定 unknown identity 无条件同步 staging 根目录并提升为可安装候选。
    pub(crate) fn confirm_durability(
        &mut self,
        pending: &PendingStagedUpdate,
    ) -> Result<DurableStagedUpdate, StagingError> {
        let _lock = lock_root(&self.root)?;
        self.refresh_locked()?;
        let Some(committed) = &self.staged else {
            return Err(StagingError::NoStagedUpdate);
        };
        if committed.name != pending.generation {
            return Err(StagingError::InvalidCandidate);
        }
        if self.durability_unknown.is_none() {
            return Ok(durable_proof(committed));
        }
        self.faults
            .checkpoint(StagingCommitPoint::DurabilityConfirmationSync)?;
        sync_directory(&self.root)?;
        let proof = durable_proof(committed);
        self.durability_unknown = None;
        // barrier 已确认 primary durability；旧 marker/旧 generation 只剩可恢复 housekeeping。
        let _ = clear_durability_marker(&self.root);
        let _ = cleanup_uncommitted(&self.root, Some(&pending.generation));
        Ok(proof)
    }

    /// 返回恢复时已安全丢弃的损坏或不匹配原因，供上层发布诊断。
    pub(crate) fn recovery_failure(&self) -> Option<StagingError> {
        self.recovery_failure
    }

    /// 在锁内提交更高候选；`Err` 保证本次 generation 未被 `current` 选中。
    ///
    /// 只有 `Committed` 可映射为 `DownloadSucceeded`；`DurabilityUnknown` 必须先通过
    /// `confirm_durability`，否则只能保留为可诊断、不可安装的待确认状态。
    pub(crate) fn stage(
        &mut self,
        candidate: StageCandidate,
        mut package: impl Read,
    ) -> Result<StageCommitOutcome, StagingError> {
        let _lock = lock_root(&self.root)?;
        if self.durability_unknown.is_some() {
            return Err(StagingError::DurabilityUnconfirmed);
        }
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
        if self.durability_unknown.is_some() {
            return Err(StagingError::DurabilityUnconfirmed);
        }
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

    /// 在同一个文件锁 lease 内完成复验、安装器消费与 Installed 收尾。
    ///
    /// `UpdateInstaller` 是同步端口，因此这里不跨 `await` 持锁。`HandedOff` 返回代表外部
    /// 安装器已完成安全 open/launch；只有收到该结果后才释放 lease。
    pub(crate) fn install_with<I: UpdateInstaller>(
        &mut self,
        installer: &I,
        intent: InstallIntent,
    ) -> Result<InstallExecutionOutcome, InstallAttemptError> {
        let _lock = lock_root(&self.root)?;
        if self.durability_unknown.is_some() {
            return Err(StagingError::DurabilityUnconfirmed.into());
        }
        self.refresh_locked()?;
        if self.installed_generation.as_ref().is_some_and(|installed| {
            self.staged
                .as_ref()
                .is_none_or(|committed| &committed.name == installed)
        }) {
            return Err(StagingError::NoStagedUpdate.into());
        }
        let Some(committed) = &self.staged else {
            return Err(self
                .recovery_failure
                .unwrap_or(StagingError::NoStagedUpdate)
                .into());
        };
        if self.durability_unknown.as_deref() == Some(&committed.name) {
            return Err(StagingError::DurabilityUnconfirmed.into());
        }
        let package = self.root.join(&committed.name).join(PACKAGE_FILE);
        self.verifier
            .verify(&package, &committed.metadata.signature)
            .map_err(|_| StagingError::VerificationFailed)?;
        let verified = VerifiedStagedPackage {
            version: committed.metadata.version.clone(),
            path: package,
            target: committed.metadata.target.clone(),
            install_kind: committed.metadata.install_kind,
        };
        let outcome = installer
            .install(&verified, intent)
            .map_err(InstallAttemptError::Installer)?;
        match outcome {
            InstallOutcome::Installed => {
                let generation = committed.name.clone();
                self.installed_generation = Some(generation);
                let cleanup = self.finish_installed_cleanup_locked();
                Ok(InstallExecutionOutcome::Installed { cleanup })
            }
            // HandedOff 只有在外部进程已完成安全 open/launch 后才能返回；此处随后释放 lease。
            InstallOutcome::HandedOff => Ok(InstallExecutionOutcome::HandedOff),
        }
    }

    /// 在新进程版本达到暂存版本后确认 installer handoff 成功并清理。
    pub(crate) fn reconcile_current_version(
        &mut self,
        current_version: &str,
    ) -> Result<bool, StagingError> {
        let current =
            Version::parse(current_version).map_err(|_| StagingError::InvalidCandidate)?;
        let _lock = lock_root(&self.root)?;
        let had_cached_staging = self.staged.is_some();
        // pointer inspect 失败也不能继续暴露调用前缓存 proof。
        self.staged = None;
        self.durability_unknown = None;
        self.recovery_failure = None;
        let had_pointer = read_pointer(&self.root)?.is_some();
        let had_staging = had_cached_staging || had_pointer;
        self.context.current_version = current;
        self.refresh_locked()?;
        Ok(had_staging && self.staged.is_none())
    }

    /// 将候选写入独立 generation，并在最后一步原子切换 commit pointer。
    fn commit_generation_locked(
        &mut self,
        candidate: StageCandidate,
        package: &mut impl Read,
    ) -> Result<StageCommitOutcome, StagingError> {
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
        // generation 目录项先于 pointer 被同步，pointer 后失败由当前实例的 pending identity 保守持有。
        sync_directory(&self.root)?;
        write_commit_pointer(&self.root, &generation_name)?;
        if self
            .faults
            .checkpoint(StagingCommitPoint::AfterPointerCommit)
            .is_err()
            || sync_directory(&self.root).is_err()
        {
            return Ok(self.finish_commit(generation_name, metadata, false));
        }

        if self
            .faults
            .checkpoint(StagingCommitPoint::AfterPointerSync)
            .is_ok()
        {
            // pointer 已持久化后，旧 generation 只属于可恢复 housekeeping。
            let _ = cleanup_uncommitted(&self.root, Some(&generation_name));
        }
        Ok(self.finish_commit(generation_name, metadata, true))
    }

    /// 让返回 outcome 与当前仓储 snapshot 在同一个临界区内保持一致。
    fn finish_commit(
        &mut self,
        generation_name: String,
        metadata: StoredMetadata,
        durable: bool,
    ) -> StageCommitOutcome {
        let staged = DurableStagedUpdate {
            version: metadata.version.clone(),
            release_notes: metadata.release_notes.clone(),
            target: metadata.target.clone(),
            install_kind: metadata.install_kind,
        };
        self.staged = Some(CommittedGeneration {
            name: generation_name.clone(),
            metadata,
        });
        self.durability_unknown = (!durable).then_some(generation_name.clone());
        self.recovery_failure = None;
        if durable {
            StageCommitOutcome::Committed(staged)
        } else {
            StageCommitOutcome::DurabilityUnknown(PendingStagedUpdate {
                generation: generation_name,
                version: staged.version,
                release_notes: staged.release_notes,
            })
        }
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

    /// 在已持有跨进程锁时重读 pointer，并用无条件 root barrier 证明该磁盘视图 durable。
    fn refresh_locked(&mut self) -> Result<(), StagingError> {
        let pending = self.durability_unknown.take();
        // 任何 inspect、verify、barrier 失败都不得继续投影调用前缓存的 durable proof。
        self.staged = None;
        self.recovery_failure = None;
        self.faults.checkpoint(StagingCommitPoint::RefreshInspect)?;
        let inspection =
            inspect_committed(&self.root, &self.context, &self.verifier, &self.faults)?;
        let barrier = self
            .faults
            .checkpoint(StagingCommitPoint::RefreshDurabilitySync)
            .and_then(|()| sync_directory(&self.root));
        barrier?;

        // barrier 之后磁盘 current 才有资格成为本实例的 durable/pending 权威视图。
        self.staged = inspection.committed;
        self.recovery_failure = inspection.discarded;
        self.durability_unknown = pending.filter(|generation| {
            self.staged
                .as_ref()
                .is_some_and(|committed| &committed.name == generation)
        });

        let housekeeping = self
            .faults
            .checkpoint(StagingCommitPoint::RefreshCleanup)
            .and_then(|()| apply_recovery_cleanup(&self.root, &inspection.cleanup))
            .and_then(|()| clear_durability_marker(&self.root));
        if housekeeping.is_err() {
            // current 已经由 barrier 证明；cleanup 只留下显式诊断，不得降级或恢复旧缓存 proof。
            self.recovery_failure = Some(StagingError::StorageFailed);
        }
        Ok(())
    }

    /// 在 Installed 主结果之后尽力清理，并把所有失败收敛为可恢复 disposition。
    fn finish_installed_cleanup_locked(&mut self) -> InstallCleanupDisposition {
        if self
            .faults
            .checkpoint(StagingCommitPoint::BeforeInstallPointerDelete)
            .is_err()
            || remove_if_exists(&self.root.join(COMMIT_POINTER)).is_err()
        {
            return InstallCleanupDisposition::Pending;
        }
        self.staged = None;
        self.durability_unknown = None;
        if self
            .faults
            .checkpoint(StagingCommitPoint::AfterInstallPointerDelete)
            .is_err()
            || sync_directory(&self.root).is_err()
        {
            return InstallCleanupDisposition::DurabilityUnknown;
        }
        if self
            .faults
            .checkpoint(StagingCommitPoint::BeforeInstallGenerationCleanup)
            .is_err()
            || cleanup_uncommitted(&self.root, None).is_err()
        {
            return InstallCleanupDisposition::Pending;
        }
        InstallCleanupDisposition::Complete
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

/// 记录 barrier 成功后才能执行的恢复清理计划。
enum RecoveryCleanup {
    /// 保留 current generation，只清理其他 generation 与临时文件。
    KeepCurrent(String),
    /// 清除无效/缺失 current 及全部未提交 generation。
    ClearCurrent,
}

/// 汇总一次完全只读的 pointer/generation 检查结果。
struct RecoveryInspection {
    committed: Option<CommittedGeneration>,
    discarded: Option<StagingError>,
    cleanup: RecoveryCleanup,
}

/// 只读检查并验证 current 指向的 generation，不删除任何回滚或兼容文件。
fn inspect_committed<V: MinisignVerifier, F: StagingFaultInjector>(
    root: &Path,
    context: &StagingContext,
    verifier: &V,
    faults: &F,
) -> Result<RecoveryInspection, StagingError> {
    let pointer = match read_pointer(root) {
        Ok(pointer) => pointer,
        Err(StagingError::StorageFailed) => return Err(StagingError::StorageFailed),
        Err(error) => {
            return Ok(RecoveryInspection {
                committed: None,
                discarded: Some(error),
                cleanup: RecoveryCleanup::ClearCurrent,
            });
        }
    };
    let Some(generation_name) = pointer else {
        let incomplete = contains_generation(root)?;
        return Ok(RecoveryInspection {
            committed: None,
            discarded: incomplete.then_some(StagingError::StorageFailed),
            cleanup: RecoveryCleanup::ClearCurrent,
        });
    };

    let generation_path = root.join(&generation_name);
    let metadata_path = generation_path.join(METADATA_FILE);
    let package_path = generation_path.join(PACKAGE_FILE);
    let recovered = read_metadata(&metadata_path).and_then(|metadata| {
        validate_metadata(&metadata, context)?;
        faults.checkpoint(StagingCommitPoint::RefreshVerify)?;
        verifier
            .verify(&package_path, &metadata.signature)
            .map_err(|_| StagingError::VerificationFailed)?;
        Ok(metadata)
    });
    match recovered {
        Ok(metadata) => Ok(RecoveryInspection {
            committed: Some(CommittedGeneration {
                name: generation_name.clone(),
                metadata,
            }),
            discarded: None,
            cleanup: RecoveryCleanup::KeepCurrent(generation_name),
        }),
        Err(StagingError::StorageFailed) => Err(StagingError::StorageFailed),
        Err(error) => Ok(RecoveryInspection {
            committed: None,
            discarded: (error != StagingError::NotNewer).then_some(error),
            cleanup: RecoveryCleanup::ClearCurrent,
        }),
    }
}

/// 在 root barrier 成功后应用恢复清理计划，不改变已确认 current 的信任级别。
fn apply_recovery_cleanup(root: &Path, cleanup: &RecoveryCleanup) -> Result<(), StagingError> {
    match cleanup {
        RecoveryCleanup::KeepCurrent(generation) => cleanup_uncommitted(root, Some(generation)),
        RecoveryCleanup::ClearCurrent => {
            remove_if_exists(&root.join(COMMIT_POINTER))?;
            // invalid/no pointer 分支始终同步删除意图，再清理不完整 generation。
            sync_directory(root)?;
            cleanup_uncommitted(root, None)
        }
    }
}

/// 有界读取并验证原子 commit pointer 只引用本目录 generation 名称。
fn read_pointer(root: &Path) -> Result<Option<String>, StagingError> {
    read_generation_reference(&root.join(COMMIT_POINTER))
}

/// 有界读取并验证一个只允许引用同目录 generation 的文件。
fn read_generation_reference(path: &Path) -> Result<Option<String>, StagingError> {
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

/// 删除旧版 durability marker 并持久化目录项；访问错误绝不伪装成不存在。
fn clear_durability_marker(root: &Path) -> Result<(), StagingError> {
    let marker = root.join(DURABILITY_MARKER);
    match fs::remove_file(marker) {
        Ok(()) => sync_directory(root),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(StagingError::StorageFailed),
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
