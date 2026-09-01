use super::{Clock, DurableStagedUpdate, InstallIntent, PreferenceError, PreferenceStore};
use semver::Version;
use serde::Serialize;
use std::fmt;
use std::time::Duration;

const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_AUTOMATIC_RETRIES: u8 = 3;

/// Rust 持有并交给可信下载适配器的目标包描述。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct ReleasePackage {
    download_url: String,
    signature: String,
    target: String,
    install_kind: super::InstallKind,
}

impl ReleasePackage {
    /// 创建由 Stable manifest 解析出的 HTTPS 目标包描述。
    pub(crate) fn new(
        download_url: impl Into<String>,
        signature: impl Into<String>,
        target: impl Into<String>,
        install_kind: super::InstallKind,
    ) -> Result<Self, UpdateDomainError> {
        let package = Self {
            download_url: download_url.into(),
            signature: signature.into(),
            target: target.into(),
            install_kind,
        };
        let download_url = package.download_url.trim();
        if !download_url.starts_with("https://")
            || download_url.trim_start_matches("https://").is_empty()
            || package.signature.trim().is_empty()
            || package.target.trim().is_empty()
        {
            return Err(UpdateDomainError::InvalidReleasePackage);
        }
        Ok(package)
    }

    /// 返回只供可信网络适配器使用的包地址。
    pub(crate) fn download_url(&self) -> &str {
        &self.download_url
    }

    /// 返回只供 minisign 验证边界使用的 manifest 签名。
    pub(crate) fn signature(&self) -> &str {
        &self.signature
    }

    /// 返回 manifest 中已选择的平台目标。
    pub(crate) fn target(&self) -> &str {
        &self.target
    }

    /// 返回与目标平台绑定的安装类型。
    pub(crate) fn install_kind(&self) -> super::InstallKind {
        self.install_kind
    }
}

impl fmt::Debug for ReleasePackage {
    /// 调试输出隐藏 Rust 持有的地址和签名字面量。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ReleasePackage")
            .field("download_url", &"[redacted]")
            .field("signature", &"[redacted]")
            .field("target", &self.target)
            .field("install_kind", &self.install_kind)
            .finish()
    }
}

/// 单一快照中可公开展示的候选版本与有界 Release Notes。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateCandidate {
    pub(crate) version: String,
    pub(crate) release_notes: String,
}

impl UpdateCandidate {
    /// 创建 provider-neutral 候选摘要并限制 Release Notes 大小。
    fn new(version: impl Into<String>, release_notes: impl AsRef<str>) -> Self {
        Self {
            version: version.into(),
            release_notes: super::bounded_release_notes(release_notes.as_ref()),
        }
    }
}

/// 可信元数据适配器交给领域层的 Stable 发布信息。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct StableRelease {
    candidate: UpdateCandidate,
    package: ReleasePackage,
}

impl StableRelease {
    /// 创建绑定公开候选摘要和 Rust 私有包描述的 Stable 发布。
    pub(crate) fn new(
        version: impl Into<String>,
        release_notes: impl AsRef<str>,
        package: ReleasePackage,
    ) -> Self {
        Self {
            candidate: UpdateCandidate::new(version, release_notes),
            package,
        }
    }

    /// 返回 Stable 候选版本。
    pub(crate) fn version(&self) -> &str {
        &self.candidate.version
    }

    /// 返回可公开展示且有界的 Release Notes。
    pub(crate) fn release_notes(&self) -> &str {
        &self.candidate.release_notes
    }

    /// 返回 provider-neutral 候选摘要。
    pub(crate) fn candidate(&self) -> UpdateCandidate {
        self.candidate.clone()
    }

    /// 返回只供可信下载与暂存适配器使用的包描述。
    pub(crate) fn package(&self) -> &ReleasePackage {
        &self.package
    }
}

impl fmt::Debug for StableRelease {
    /// 调试输出只展示 provider-neutral 字段与已脱敏包摘要。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StableRelease")
            .field("candidate", &self.candidate)
            .field("package", &self.package)
            .finish()
    }
}

/// 表示下载长度已知或未知的完整进度。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(crate) enum DownloadProgress {
    /// 服务端没有提供可信总长度。
    UnknownTotal { downloaded_bytes: u64 },
    /// 服务端提供总长度，可计算确定进度。
    KnownTotal {
        downloaded_bytes: u64,
        total_bytes: u64,
    },
}

/// 标识失败及重试所针对的更新操作。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UpdateOperation {
    /// Stable manifest 检查。
    Check,
    /// 更新包下载和持久化暂存。
    Download,
}

/// 前端可依赖的稳定错误类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum UpdateFailureCategory {
    Network,
    RequestTimeout,
    RateLimited,
    Server,
    Manifest,
    Verification,
    Storage,
    Installer,
}

impl UpdateFailureCategory {
    /// 只有网络、408、429 与服务端失败允许自动或显式重试。
    pub(crate) const fn retryable(self) -> bool {
        matches!(
            self,
            Self::Network | Self::RequestTimeout | Self::RateLimited | Self::Server
        )
    }

    /// 返回由核心控制、不会拼接 adapter 诊断的安全展示文本。
    const fn safe_message(self) -> &'static str {
        match self {
            Self::Network => "无法连接应用更新服务，请检查网络后重试。",
            Self::RequestTimeout => "应用更新请求超时，请稍后重试。",
            Self::RateLimited => "应用更新检查过于频繁，请稍后重试。",
            Self::Server => "应用更新服务暂时不可用，请稍后重试。",
            Self::Manifest => "应用更新信息无效，已停止本次更新。",
            Self::Verification => "应用更新包验证失败，已安全丢弃。",
            Self::Storage => "应用更新无法安全写入本机存储。",
            Self::Installer => "应用更新安装失败，暂存包已保留。",
        }
    }
}

/// 可信适配器提交给状态机的失败事实；原始诊断永不序列化或 Debug 输出。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct UpdateFailure {
    category: UpdateFailureCategory,
    diagnostic: String,
}

impl UpdateFailure {
    /// 创建一个由稳定类别决定重试策略的私有失败事实。
    pub(crate) fn new(category: UpdateFailureCategory, diagnostic: impl Into<String>) -> Self {
        Self {
            category,
            diagnostic: diagnostic.into(),
        }
    }

    /// 按 HTTP 状态码收敛 408、429、5xx 与不可重试 manifest 响应。
    pub(crate) fn from_http_status(status: u16, diagnostic: impl Into<String>) -> Self {
        let category = match status {
            408 => UpdateFailureCategory::RequestTimeout,
            429 => UpdateFailureCategory::RateLimited,
            500..=599 => UpdateFailureCategory::Server,
            _ => UpdateFailureCategory::Manifest,
        };
        Self::new(category, diagnostic)
    }

    /// 返回稳定失败类别。
    pub(crate) const fn category(&self) -> UpdateFailureCategory {
        self.category
    }

    /// 返回只供 Rust 诊断适配器使用的原始信息，调用方必须继续脱敏。
    pub(crate) fn diagnostic(&self) -> &str {
        &self.diagnostic
    }

    /// 返回由类别固定决定的自动与显式重试能力。
    pub(crate) const fn retryable(&self) -> bool {
        self.category.retryable()
    }
}

impl fmt::Debug for UpdateFailure {
    /// Debug 只显示稳定类别并固定隐藏原始 adapter 诊断。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UpdateFailure")
            .field("category", &self.category)
            .field("diagnostic", &"[redacted]")
            .finish()
    }
}

/// 单一快照中可公开展示的稳定失败投影。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateFailureSnapshot {
    pub(crate) operation: UpdateOperation,
    pub(crate) category: UpdateFailureCategory,
    pub(crate) retryable: bool,
    pub(crate) message: String,
}

impl UpdateFailureSnapshot {
    /// 从私有失败事实构造仅含核心安全文本的公开投影。
    fn from_failure(operation: UpdateOperation, failure: &UpdateFailure) -> Self {
        Self {
            operation,
            category: failure.category(),
            retryable: failure.retryable(),
            message: failure.category().safe_message().to_owned(),
        }
    }
}

/// 更新能力对上层暴露的 provider-neutral 状态。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub(crate) enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available,
    Downloading { progress: DownloadProgress },
    Staged,
    Failed,
}

/// 每次已接受领域输入产生的完整、单调有序快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateSnapshot {
    pub(crate) sequence: u64,
    pub(crate) current_version: String,
    pub(crate) state: UpdateState,
    pub(crate) automatic_download: bool,
    pub(crate) candidate: Option<UpdateCandidate>,
    pub(crate) failure: Option<UpdateFailureSnapshot>,
    #[serde(skip)]
    pub(crate) next_check_at: Option<Duration>,
}

/// 标识一次具体 check/download 尝试，迟到回调必须携带原 identity。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct UpdateOperationId(u64);

impl UpdateOperationId {
    /// 返回便于 adapter timer 或任务表保存的稳定进程内数值。
    pub(crate) const fn value(self) -> u64 {
        self.0
    }

    /// 仅为 adapter 契约测试创建 operation identity，生产构建不存在该入口。
    #[cfg(test)]
    pub(crate) const fn test_fixture(value: u64) -> Self {
        Self(value)
    }
}

/// 请求可信适配器执行、但不向前端开放资源参数的副作用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UpdateEffect {
    CheckStable {
        operation_id: UpdateOperationId,
    },
    ScheduleCheck {
        after: Duration,
    },
    StartDownload {
        operation_id: UpdateOperationId,
        release: StableRelease,
    },
    CancelDownload {
        operation_id: UpdateOperationId,
    },
    ScheduleRetry {
        token: u64,
        operation: UpdateOperation,
        after: Duration,
    },
    PersistStagedReleaseNotes {
        version: String,
        release_notes: String,
    },
    RequestInstall {
        intent: InstallIntent,
        version: String,
    },
}

/// Controller 接收的可信结果与窄化用户意图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UpdateInput {
    RecoverStaged {
        staged: DurableStagedUpdate,
    },
    Ready,
    PeriodicCheckDue,
    ManualCheck,
    CheckSucceeded {
        operation_id: UpdateOperationId,
        release: Option<StableRelease>,
    },
    CheckFailed {
        operation_id: UpdateOperationId,
        failure: UpdateFailure,
    },
    SetAutomaticDownload(bool),
    DownloadStarted {
        operation_id: UpdateOperationId,
    },
    DownloadAdvanced {
        operation_id: UpdateOperationId,
        progress: DownloadProgress,
    },
    DownloadSucceeded {
        operation_id: UpdateOperationId,
        staged: DurableStagedUpdate,
    },
    DownloadFailed {
        operation_id: UpdateOperationId,
        failure: UpdateFailure,
    },
    UserRetry,
    RetryDue {
        token: u64,
    },
    RequestUpdateRestart,
}

/// 汇总一次领域转换产生的完整快照与可信副作用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpdateOutput {
    pub(crate) snapshot: UpdateSnapshot,
    pub(crate) effects: Vec<UpdateEffect>,
}

/// 描述更新领域初始化或输入验证失败。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UpdateDomainError {
    InvalidCurrentVersion(String),
    InvalidReleaseVersion(String),
    InvalidReleasePackage,
    Preference(PreferenceError),
}

impl From<PreferenceError> for UpdateDomainError {
    /// 将独立偏好仓储错误收敛到领域初始化或更新失败。
    fn from(error: PreferenceError) -> Self {
        Self::Preference(error)
    }
}

/// 一次活动操作所处的唯一阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperationPhase {
    Checking,
    DownloadPending,
    Downloading,
}

/// 保存活动尝试的 identity、阶段和是否由自动策略发起。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActiveOperation {
    id: UpdateOperationId,
    phase: OperationPhase,
    automatic: bool,
}

/// 保存可重试操作所需的最小领域上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingRetry {
    token: u64,
    operation: UpdateOperation,
    automatic_retries: u8,
}

/// 维护 Stable 更新生命周期、偏好、identity 与调度策略的框架无关状态机。
pub(crate) struct UpdateController<C, P> {
    clock: C,
    preferences: P,
    current_version: Version,
    current_release: Option<StableRelease>,
    staged_candidate: Option<UpdateCandidate>,
    ready: bool,
    active: Option<ActiveOperation>,
    snapshot: UpdateSnapshot,
    pending_retry: Option<PendingRetry>,
    next_retry_token: u64,
    next_operation_id: u64,
}

impl<C: Clock, P: PreferenceStore> UpdateController<C, P> {
    /// 从当前应用版本和持久化偏好创建控制器。
    pub(crate) fn new(
        current_version: &str,
        clock: C,
        preferences: P,
    ) -> Result<Self, UpdateDomainError> {
        let parsed = Version::parse(current_version)
            .map_err(|_| UpdateDomainError::InvalidCurrentVersion(current_version.to_owned()))?;
        let automatic_download = preferences.load_automatic_download()?.unwrap_or(true);
        Ok(Self {
            clock,
            preferences,
            current_version: parsed,
            current_release: None,
            staged_candidate: None,
            ready: false,
            active: None,
            snapshot: UpdateSnapshot {
                sequence: 0,
                current_version: current_version.to_owned(),
                state: UpdateState::Idle,
                automatic_download,
                candidate: None,
                failure: None,
                next_check_at: None,
            },
            pending_retry: None,
            next_retry_token: 1,
            next_operation_id: 1,
        })
    }

    /// 返回当前完整快照而不改变顺序号。
    pub(crate) fn snapshot(&self) -> &UpdateSnapshot {
        &self.snapshot
    }

    /// 消费可信结果或窄化用户意图；迟到 identity 返回完全不变的快照。
    pub(crate) fn handle(&mut self, input: UpdateInput) -> Result<UpdateOutput, UpdateDomainError> {
        let mut effects = Vec::new();
        let accepted = match input {
            UpdateInput::RecoverStaged { staged } => self.recover_staged(staged)?,
            UpdateInput::Ready if !self.ready => {
                self.ready = true;
                if self.active.is_none() {
                    self.begin_check(true, true, &mut effects);
                } else {
                    self.schedule_next_check(&mut effects);
                }
                true
            }
            UpdateInput::Ready => false,
            UpdateInput::PeriodicCheckDue if self.ready && self.active.is_none() => {
                self.begin_check(true, true, &mut effects);
                true
            }
            UpdateInput::PeriodicCheckDue if self.ready => {
                self.schedule_next_check(&mut effects);
                true
            }
            UpdateInput::PeriodicCheckDue => false,
            UpdateInput::ManualCheck if self.active.is_none() => {
                self.begin_check(false, true, &mut effects);
                true
            }
            UpdateInput::ManualCheck => false,
            UpdateInput::CheckSucceeded {
                operation_id,
                release,
            } => self.accept_check_result(operation_id, release, &mut effects)?,
            UpdateInput::CheckFailed {
                operation_id,
                failure,
            } => self.accept_failure(operation_id, UpdateOperation::Check, failure, &mut effects),
            UpdateInput::SetAutomaticDownload(enabled) => {
                self.set_automatic_download(enabled, &mut effects)?;
                true
            }
            UpdateInput::DownloadStarted { operation_id } => self.start_download(operation_id),
            UpdateInput::DownloadAdvanced {
                operation_id,
                progress,
            } => self.advance_download(operation_id, progress),
            UpdateInput::DownloadSucceeded {
                operation_id,
                staged,
            } => self.finish_download(operation_id, staged),
            UpdateInput::DownloadFailed {
                operation_id,
                failure,
            } => self.accept_failure(
                operation_id,
                UpdateOperation::Download,
                failure,
                &mut effects,
            ),
            UpdateInput::UserRetry => self.user_retry(&mut effects),
            UpdateInput::RetryDue { token } => self.retry(token, &mut effects),
            UpdateInput::RequestUpdateRestart => self.request_update_restart(&mut effects),
        };
        if accepted {
            self.snapshot.sequence = self.snapshot.sequence.saturating_add(1);
        }
        Ok(UpdateOutput {
            snapshot: self.snapshot.clone(),
            effects,
        })
    }

    /// 在启动 Ready 前恢复严格高于运行版本的可信 staging 摘要。
    fn recover_staged(&mut self, staged: DurableStagedUpdate) -> Result<bool, UpdateDomainError> {
        if self.ready || self.active.is_some() {
            return Ok(false);
        }
        let version = staged.version().to_owned();
        let parsed = Version::parse(&version)
            .map_err(|_| UpdateDomainError::InvalidReleaseVersion(version.clone()))?;
        if !parsed.pre.is_empty() || parsed <= self.current_version {
            return Ok(false);
        }
        let candidate = UpdateCandidate::new(version, staged.release_notes());
        self.staged_candidate = Some(candidate.clone());
        self.snapshot.state = UpdateState::Staged;
        self.snapshot.candidate = Some(candidate);
        self.snapshot.failure = None;
        Ok(true)
    }

    /// 开始一次带 identity 的检查，并可维护六小时周期。
    fn begin_check(
        &mut self,
        schedule_next: bool,
        reset_retry: bool,
        effects: &mut Vec<UpdateEffect>,
    ) {
        let id = self.allocate_operation_id();
        self.active = Some(ActiveOperation {
            id,
            phase: OperationPhase::Checking,
            automatic: true,
        });
        self.snapshot.state = UpdateState::Checking;
        self.snapshot.failure = None;
        if reset_retry {
            self.pending_retry = None;
        }
        effects.push(UpdateEffect::CheckStable { operation_id: id });
        if schedule_next {
            self.schedule_next_check(effects);
        }
    }

    /// 仅接受当前 Checking identity，并处理升级、同版 notes 回填或无候选结果。
    fn accept_check_result(
        &mut self,
        operation_id: UpdateOperationId,
        release: Option<StableRelease>,
        effects: &mut Vec<UpdateEffect>,
    ) -> Result<bool, UpdateDomainError> {
        if !self.active_matches(operation_id, OperationPhase::Checking) {
            return Ok(false);
        }
        self.active = None;
        self.pending_retry = None;
        self.snapshot.failure = None;
        let Some(release) = release else {
            self.current_release = None;
            self.restore_staged_or(UpdateState::UpToDate);
            return Ok(true);
        };
        let candidate_version = Version::parse(release.version())
            .map_err(|_| UpdateDomainError::InvalidReleaseVersion(release.version().to_owned()))?;
        if !candidate_version.pre.is_empty() {
            return Err(UpdateDomainError::InvalidReleaseVersion(
                release.version().to_owned(),
            ));
        }
        if candidate_version <= self.current_version {
            self.current_release = None;
            self.restore_staged_or(UpdateState::UpToDate);
            return Ok(true);
        }

        if let Some(staged) = &self.staged_candidate {
            let staged_version = Version::parse(&staged.version)
                .map_err(|_| UpdateDomainError::InvalidReleaseVersion(staged.version.clone()))?;
            if candidate_version <= staged_version {
                if candidate_version == staged_version && !release.release_notes().is_empty() {
                    let candidate = release.candidate();
                    if &candidate != staged {
                        self.staged_candidate = Some(candidate.clone());
                        effects.push(UpdateEffect::PersistStagedReleaseNotes {
                            version: candidate.version.clone(),
                            release_notes: candidate.release_notes.clone(),
                        });
                    }
                }
                self.current_release = None;
                self.restore_staged_or(UpdateState::UpToDate);
                return Ok(true);
            }
        }

        self.current_release = Some(release.clone());
        self.snapshot.state = UpdateState::Available;
        self.snapshot.candidate = Some(release.candidate());
        if self.snapshot.automatic_download {
            self.begin_download(release, true, true, effects);
        }
        Ok(true)
    }

    /// 原子保存偏好；关闭时取消自动下载并使已排定 download retry 失效。
    fn set_automatic_download(
        &mut self,
        enabled: bool,
        effects: &mut Vec<UpdateEffect>,
    ) -> Result<(), UpdateDomainError> {
        self.preferences.save_automatic_download(enabled)?;
        let was_enabled = self.snapshot.automatic_download;
        self.snapshot.automatic_download = enabled;
        if !enabled {
            if self
                .pending_retry
                .as_ref()
                .is_some_and(|pending| pending.operation == UpdateOperation::Download)
            {
                self.pending_retry = None;
            }
            if let Some(active) = self.active {
                if active.automatic
                    && matches!(
                        active.phase,
                        OperationPhase::DownloadPending | OperationPhase::Downloading
                    )
                {
                    self.active = None;
                    effects.push(UpdateEffect::CancelDownload {
                        operation_id: active.id,
                    });
                    self.present_available_release();
                }
            } else if self
                .snapshot
                .failure
                .as_ref()
                .is_some_and(|failure| failure.operation == UpdateOperation::Download)
            {
                self.present_available_release();
            }
        } else if !was_enabled && self.active.is_none() {
            if let Some(release) = self.current_release.clone() {
                self.begin_download(release, true, true, effects);
            }
        }
        Ok(())
    }

    /// 分配 identity 并开始一次自动或显式下载尝试。
    fn begin_download(
        &mut self,
        release: StableRelease,
        automatic: bool,
        reset_retry: bool,
        effects: &mut Vec<UpdateEffect>,
    ) {
        let id = self.allocate_operation_id();
        self.active = Some(ActiveOperation {
            id,
            phase: OperationPhase::DownloadPending,
            automatic,
        });
        self.snapshot.candidate = Some(release.candidate());
        self.snapshot.failure = None;
        if reset_retry {
            self.pending_retry = None;
        }
        effects.push(UpdateEffect::StartDownload {
            operation_id: id,
            release,
        });
    }

    /// 仅接受当前 DownloadPending identity 并进入 downloading 状态。
    fn start_download(&mut self, operation_id: UpdateOperationId) -> bool {
        let Some(active) = &mut self.active else {
            return false;
        };
        if active.id != operation_id || active.phase != OperationPhase::DownloadPending {
            return false;
        }
        active.phase = OperationPhase::Downloading;
        self.snapshot.state = UpdateState::Downloading {
            progress: DownloadProgress::UnknownTotal {
                downloaded_bytes: 0,
            },
        };
        true
    }

    /// 仅用当前 Downloading identity 更新进度，迟到或取消后的回调不发布 sequence。
    fn advance_download(
        &mut self,
        operation_id: UpdateOperationId,
        progress: DownloadProgress,
    ) -> bool {
        if !self.active_matches(operation_id, OperationPhase::Downloading) {
            return false;
        }
        self.snapshot.state = UpdateState::Downloading { progress };
        true
    }

    /// 仅接受当前下载 identity 与精确 durable proof，并把可信候选提升为 staged。
    fn finish_download(
        &mut self,
        operation_id: UpdateOperationId,
        staged: DurableStagedUpdate,
    ) -> bool {
        if !self.active_is_download(operation_id) {
            return false;
        }
        let Some(release) = self.current_release.clone() else {
            return false;
        };
        if staged.version() != release.version()
            || staged.release_notes() != release.release_notes()
            || staged.target() != release.package().target()
            || staged.install_kind() != release.package().install_kind()
        {
            return false;
        }
        self.active = None;
        self.pending_retry = None;
        let candidate = UpdateCandidate::new(staged.version(), staged.release_notes());
        self.staged_candidate = Some(candidate.clone());
        self.snapshot.state = UpdateState::Staged;
        self.snapshot.candidate = Some(candidate);
        self.snapshot.failure = None;
        true
    }

    /// 仅接受活动 identity 的失败，并按稳定策略安排真实 1/2/4 秒重试。
    fn accept_failure(
        &mut self,
        operation_id: UpdateOperationId,
        operation: UpdateOperation,
        failure: UpdateFailure,
        effects: &mut Vec<UpdateEffect>,
    ) -> bool {
        let accepted = match operation {
            UpdateOperation::Check => self.active_matches(operation_id, OperationPhase::Checking),
            UpdateOperation::Download => self.active_is_download(operation_id),
        };
        if !accepted {
            return false;
        }
        self.active = None;
        let prior_retries = self
            .pending_retry
            .as_ref()
            .filter(|pending| pending.operation == operation)
            .map_or(0, |pending| pending.automatic_retries);
        let automatic_allowed = operation == UpdateOperation::Check
            || (operation == UpdateOperation::Download && self.snapshot.automatic_download);
        if failure.retryable() && automatic_allowed && prior_retries < MAX_AUTOMATIC_RETRIES {
            let retries = prior_retries + 1;
            let token = self.allocate_retry_token();
            self.pending_retry = Some(PendingRetry {
                token,
                operation,
                automatic_retries: retries,
            });
            effects.push(UpdateEffect::ScheduleRetry {
                token,
                operation,
                after: retry_delay(retries),
            });
        } else {
            self.pending_retry = None;
        }

        self.snapshot.failure = Some(UpdateFailureSnapshot::from_failure(operation, &failure));
        if operation == UpdateOperation::Download && !self.snapshot.automatic_download {
            self.present_available_release();
        } else if self.staged_candidate.is_some() {
            self.restore_staged_or(UpdateState::Failed);
        } else {
            self.snapshot.state = UpdateState::Failed;
            if operation == UpdateOperation::Download {
                self.snapshot.candidate =
                    self.current_release.as_ref().map(StableRelease::candidate);
            }
        }
        true
    }

    /// 在有效 retry token 到期时创建新的 operation identity；download 还需偏好保持开启。
    fn retry(&mut self, token: u64, effects: &mut Vec<UpdateEffect>) -> bool {
        let Some(pending) = self.pending_retry.clone() else {
            return false;
        };
        if pending.token != token || self.active.is_some() {
            return false;
        }
        match pending.operation {
            UpdateOperation::Check => self.begin_check(false, false, effects),
            UpdateOperation::Download if self.snapshot.automatic_download => {
                let Some(release) = self.current_release.clone() else {
                    return false;
                };
                self.begin_download(release, true, false, effects);
            }
            UpdateOperation::Download => return false,
        }
        true
    }

    /// 显式 retry 只重放最近一次可重试操作，并创建用户发起的下载 identity。
    fn user_retry(&mut self, effects: &mut Vec<UpdateEffect>) -> bool {
        if self.active.is_some() {
            return false;
        }
        let Some(failure) = &self.snapshot.failure else {
            return false;
        };
        if !failure.retryable {
            return false;
        }
        let operation = failure.operation;
        self.pending_retry = None;
        match operation {
            UpdateOperation::Check => self.begin_check(false, true, effects),
            UpdateOperation::Download => {
                let Some(release) = self.current_release.clone() else {
                    return false;
                };
                self.begin_download(release, false, true, effects);
            }
        }
        true
    }

    /// 只有当前公开状态仍为 staged 时才请求共享停止与更新安装。
    fn request_update_restart(&self, effects: &mut Vec<UpdateEffect>) -> bool {
        if self.snapshot.state != UpdateState::Staged {
            return false;
        }
        let Some(candidate) = &self.staged_candidate else {
            return false;
        };
        effects.push(UpdateEffect::RequestInstall {
            intent: InstallIntent::UpdateRestart,
            version: candidate.version.clone(),
        });
        true
    }

    /// 忙碌时只续订六小时 timer，避免消费唯一周期信号后停止检查。
    fn schedule_next_check(&mut self, effects: &mut Vec<UpdateEffect>) {
        self.snapshot.next_check_at = Some(self.clock.now() + CHECK_INTERVAL);
        effects.push(UpdateEffect::ScheduleCheck {
            after: CHECK_INTERVAL,
        });
    }

    /// 把当前可信 staged 候选恢复为单一快照的权威状态。
    fn restore_staged_or(&mut self, fallback: UpdateState) {
        if let Some(candidate) = &self.staged_candidate {
            self.snapshot.state = UpdateState::Staged;
            self.snapshot.candidate = Some(candidate.clone());
        } else {
            let up_to_date = fallback == UpdateState::UpToDate;
            self.snapshot.state = fallback;
            if up_to_date {
                self.snapshot.candidate = None;
            }
        }
    }

    /// 把已知网络候选恢复为 available；旧 staging 仍只保留在可信内部安装边界。
    fn present_available_release(&mut self) {
        if let Some(release) = &self.current_release {
            self.snapshot.state = UpdateState::Available;
            self.snapshot.candidate = Some(release.candidate());
        }
    }

    /// 判断回调是否属于当前 operation identity 与精确阶段。
    fn active_matches(&self, id: UpdateOperationId, phase: OperationPhase) -> bool {
        self.active
            .is_some_and(|active| active.id == id && active.phase == phase)
    }

    /// 判断回调是否属于当前 download identity 的 pending/downloading 阶段。
    fn active_is_download(&self, id: UpdateOperationId) -> bool {
        self.active.is_some_and(|active| {
            active.id == id
                && matches!(
                    active.phase,
                    OperationPhase::DownloadPending | OperationPhase::Downloading
                )
        })
    }

    /// 分配进程内 operation identity，并跳过保留的零值。
    fn allocate_operation_id(&mut self) -> UpdateOperationId {
        let id = UpdateOperationId(self.next_operation_id);
        self.next_operation_id = self.next_operation_id.wrapping_add(1).max(1);
        id
    }

    /// 分配 retry timer generation，并跳过保留的零值。
    fn allocate_retry_token(&mut self) -> u64 {
        let token = self.next_retry_token;
        self.next_retry_token = self.next_retry_token.wrapping_add(1).max(1);
        token
    }
}

/// 计算第 N 次自动重试的一、二、四秒有界指数延迟。
fn retry_delay(automatic_retry: u8) -> Duration {
    Duration::from_secs(1_u64 << automatic_retry.saturating_sub(1).min(2))
}
