use super::{Clock, InstallIntent, PreferenceError, PreferenceStore};
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
    /// 创建由 Stable manifest 解析出的目标包描述。
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

/// 可信元数据适配器交给领域层的 Stable 发布信息。
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct StableRelease {
    version: String,
    release_notes: String,
    package: ReleasePackage,
}

impl StableRelease {
    /// 创建绑定公开展示字段和 Rust 私有包描述的 Stable 候选。
    pub(crate) fn new(
        version: impl Into<String>,
        release_notes: impl Into<String>,
        package: ReleasePackage,
    ) -> Self {
        Self {
            version: version.into(),
            release_notes: release_notes.into(),
            package,
        }
    }

    /// 返回 Stable 候选版本。
    pub(crate) fn version(&self) -> &str {
        &self.version
    }

    /// 返回可公开展示的 Release Notes。
    pub(crate) fn release_notes(&self) -> &str {
        &self.release_notes
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
            .field("version", &self.version)
            .field("release_notes", &self.release_notes)
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
    /// 只有网络、408、429 与服务端失败允许自动重试。
    pub(crate) const fn retryable(self) -> bool {
        matches!(
            self,
            Self::Network | Self::RequestTimeout | Self::RateLimited | Self::Server
        )
    }
}

/// 可信适配器提交给状态机的脱敏失败事实。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct UpdateFailure {
    category: UpdateFailureCategory,
    message: String,
}

impl UpdateFailure {
    /// 创建一个由稳定类别决定重试策略的失败。
    pub(crate) fn new(category: UpdateFailureCategory, message: impl Into<String>) -> Self {
        Self {
            category,
            message: message.into(),
        }
    }

    /// 按 HTTP 状态码收敛 408、429、5xx 与不可重试 manifest 响应。
    pub(crate) fn from_http_status(status: u16, message: impl Into<String>) -> Self {
        let category = match status {
            408 => UpdateFailureCategory::RequestTimeout,
            429 => UpdateFailureCategory::RateLimited,
            500..=599 => UpdateFailureCategory::Server,
            _ => UpdateFailureCategory::Manifest,
        };
        Self::new(category, message)
    }

    /// 返回稳定失败类别。
    pub(crate) const fn category(&self) -> UpdateFailureCategory {
        self.category
    }

    /// 返回可安全展示的失败文本。
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    /// 返回由类别固定决定的自动与显式重试能力。
    pub(crate) const fn retryable(&self) -> bool {
        self.category.retryable()
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
    Available {
        version: String,
        release_notes: String,
    },
    Downloading {
        version: String,
        release_notes: String,
        progress: DownloadProgress,
    },
    Staged {
        version: String,
        release_notes: String,
    },
    Failed {
        operation: UpdateOperation,
        category: UpdateFailureCategory,
        retryable: bool,
        message: String,
    },
}

/// 每次领域输入产生的完整、单调有序快照。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpdateSnapshot {
    pub(crate) sequence: u64,
    pub(crate) current_version: String,
    pub(crate) state: UpdateState,
    pub(crate) automatic_download: bool,
    #[serde(skip)]
    pub(crate) next_check_at: Option<Duration>,
}

/// 请求可信适配器执行、但不向前端开放资源参数的副作用。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UpdateEffect {
    CheckStable,
    ScheduleCheck {
        after: Duration,
    },
    DiscardStaged {
        replacement_version: String,
    },
    StartDownload {
        release: StableRelease,
    },
    ScheduleRetry {
        token: u64,
        operation: UpdateOperation,
        after: Duration,
    },
    RequestInstall {
        intent: InstallIntent,
        version: String,
    },
}

/// 公开 Controller 接收的可信结果与窄化用户意图。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum UpdateInput {
    RecoverStaged { version: String },
    Ready,
    PeriodicCheckDue,
    ManualCheck,
    CheckSucceeded { release: Option<StableRelease> },
    CheckFailed(UpdateFailure),
    StagingDiscarded,
    StagingDiscardFailed,
    SetAutomaticDownload(bool),
    DownloadStarted,
    DownloadAdvanced(DownloadProgress),
    DownloadSucceeded,
    DownloadFailed(UpdateFailure),
    UserRetry,
    RetryDue { token: u64 },
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

/// 保存可重试操作所需的最小领域上下文。
#[derive(Debug, Clone, PartialEq, Eq)]
struct PendingRetry {
    token: u64,
    operation: UpdateOperation,
    automatic_retries: u8,
}

/// 维护 Stable 更新生命周期、偏好与调度策略的框架无关状态机。
pub(crate) struct UpdateController<C, P> {
    clock: C,
    preferences: P,
    current_version: Version,
    current_release: Option<StableRelease>,
    staged_version: Option<Version>,
    ready: bool,
    operation_active: bool,
    snapshot: UpdateSnapshot,
    pending_retry: Option<PendingRetry>,
    next_retry_token: u64,
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
            staged_version: None,
            ready: false,
            operation_active: false,
            snapshot: UpdateSnapshot {
                sequence: 0,
                current_version: current_version.to_owned(),
                state: UpdateState::Idle,
                automatic_download,
                next_check_at: None,
            },
            pending_retry: None,
            next_retry_token: 1,
        })
    }

    /// 返回当前完整快照而不改变顺序号。
    pub(crate) fn snapshot(&self) -> &UpdateSnapshot {
        &self.snapshot
    }

    /// 消费可信结果或窄化用户意图并返回下一份完整快照。
    pub(crate) fn handle(&mut self, input: UpdateInput) -> Result<UpdateOutput, UpdateDomainError> {
        let mut effects = Vec::new();
        match input {
            UpdateInput::RecoverStaged { version } => self.recover_staged(version)?,
            UpdateInput::Ready if !self.ready => {
                self.ready = true;
                if !self.operation_active {
                    self.start_check(true, &mut effects);
                } else {
                    self.schedule_next_check(&mut effects);
                }
            }
            UpdateInput::Ready => {}
            UpdateInput::PeriodicCheckDue if self.ready && !self.operation_active => {
                self.start_check(true, &mut effects)
            }
            UpdateInput::PeriodicCheckDue if self.ready => self.schedule_next_check(&mut effects),
            UpdateInput::PeriodicCheckDue => {}
            UpdateInput::ManualCheck if !self.operation_active => {
                self.start_check(false, &mut effects)
            }
            UpdateInput::ManualCheck => {}
            UpdateInput::CheckSucceeded { release } => {
                self.accept_check_result(release, &mut effects)?
            }
            UpdateInput::CheckFailed(failure) => {
                self.accept_failure(UpdateOperation::Check, failure, &mut effects)
            }
            UpdateInput::StagingDiscarded => self.finish_staging_discard(&mut effects),
            UpdateInput::StagingDiscardFailed => self.fail_staging_discard(),
            UpdateInput::SetAutomaticDownload(enabled) => {
                self.set_automatic_download(enabled, &mut effects)?
            }
            UpdateInput::DownloadStarted => self.start_download(),
            UpdateInput::DownloadAdvanced(progress) => self.advance_download(progress),
            UpdateInput::DownloadSucceeded => self.finish_download(),
            UpdateInput::DownloadFailed(failure) => {
                self.accept_failure(UpdateOperation::Download, failure, &mut effects)
            }
            UpdateInput::UserRetry => self.user_retry(&mut effects),
            UpdateInput::RetryDue { token } => self.retry(token, &mut effects),
            UpdateInput::RequestUpdateRestart => self.request_update_restart(&mut effects),
        }
        self.snapshot.sequence = self.snapshot.sequence.saturating_add(1);
        Ok(UpdateOutput {
            snapshot: self.snapshot.clone(),
            effects,
        })
    }

    /// 恢复严格高于运行版本的可信 staging 摘要。
    fn recover_staged(&mut self, version: String) -> Result<(), UpdateDomainError> {
        let candidate = Version::parse(&version)
            .map_err(|_| UpdateDomainError::InvalidReleaseVersion(version.clone()))?;
        if candidate > self.current_version {
            self.staged_version = Some(candidate);
            self.snapshot.state = UpdateState::Staged {
                version,
                release_notes: String::new(),
            };
        }
        Ok(())
    }

    /// 开始一次非阻塞检查并维护六小时周期。
    fn start_check(&mut self, schedule_next: bool, effects: &mut Vec<UpdateEffect>) {
        self.operation_active = true;
        self.snapshot.state = UpdateState::Checking;
        self.pending_retry = None;
        effects.push(UpdateEffect::CheckStable);
        if schedule_next {
            self.schedule_next_check(effects);
        }
    }

    /// 接受可信 Stable 检查结果并阻止降级、重复安装和旧 staging 覆盖。
    fn accept_check_result(
        &mut self,
        release: Option<StableRelease>,
        effects: &mut Vec<UpdateEffect>,
    ) -> Result<(), UpdateDomainError> {
        self.pending_retry = None;
        self.operation_active = false;
        let Some(release) = release else {
            self.current_release = None;
            self.snapshot.state = self.staged_state_or(UpdateState::UpToDate);
            return Ok(());
        };
        let candidate = Version::parse(release.version())
            .map_err(|_| UpdateDomainError::InvalidReleaseVersion(release.version().to_owned()))?;
        if !candidate.pre.is_empty() {
            return Err(UpdateDomainError::InvalidReleaseVersion(
                release.version().to_owned(),
            ));
        }
        if candidate <= self.current_version {
            self.current_release = None;
            self.snapshot.state = self.staged_state_or(UpdateState::UpToDate);
            return Ok(());
        }
        if self
            .staged_version
            .as_ref()
            .is_some_and(|staged| candidate <= *staged)
        {
            self.snapshot.state = self.staged_state_or(UpdateState::UpToDate);
            return Ok(());
        }

        self.snapshot.state = available_state(&release);
        self.current_release = Some(release.clone());
        if self.staged_version.is_some() {
            self.operation_active = true;
            effects.push(UpdateEffect::DiscardStaged {
                replacement_version: release.version().to_owned(),
            });
        } else if self.snapshot.automatic_download {
            self.operation_active = true;
            effects.push(UpdateEffect::StartDownload { release });
        }
        Ok(())
    }

    /// 完成旧 staging 清理，并按当前偏好继续可信候选下载。
    fn finish_staging_discard(&mut self, effects: &mut Vec<UpdateEffect>) {
        self.staged_version = None;
        self.operation_active = false;
        if self.snapshot.automatic_download {
            if let Some(release) = self.current_release.clone() {
                self.operation_active = true;
                effects.push(UpdateEffect::StartDownload { release });
            }
        }
    }

    /// 将旧 staging 清理失败固定为不可自动重试的存储失败。
    fn fail_staging_discard(&mut self) {
        self.operation_active = false;
        self.staged_version = None;
        self.pending_retry = None;
        self.snapshot.state = UpdateState::Failed {
            operation: UpdateOperation::Download,
            category: UpdateFailureCategory::Storage,
            retryable: false,
            message: "无法替换已暂存的应用更新".into(),
        };
    }

    /// 原子保存自动下载偏好，并在重新开启时继续当前可用候选。
    fn set_automatic_download(
        &mut self,
        enabled: bool,
        effects: &mut Vec<UpdateEffect>,
    ) -> Result<(), UpdateDomainError> {
        self.preferences.save_automatic_download(enabled)?;
        self.snapshot.automatic_download = enabled;
        if enabled
            && !self.operation_active
            && matches!(self.snapshot.state, UpdateState::Available { .. })
        {
            if let Some(release) = self.current_release.clone() {
                self.operation_active = true;
                effects.push(UpdateEffect::StartDownload { release });
            }
        }
        Ok(())
    }

    /// 忙碌时只续订六小时 timer，避免消费唯一周期信号后停止检查。
    fn schedule_next_check(&mut self, effects: &mut Vec<UpdateEffect>) {
        self.snapshot.next_check_at = Some(self.clock.now() + CHECK_INTERVAL);
        effects.push(UpdateEffect::ScheduleCheck {
            after: CHECK_INTERVAL,
        });
    }

    /// 将已接受的 Stable 发布转换为下载中状态。
    fn start_download(&mut self) {
        if let Some(release) = &self.current_release {
            self.snapshot.state = UpdateState::Downloading {
                version: release.version().to_owned(),
                release_notes: release.release_notes().to_owned(),
                progress: DownloadProgress::UnknownTotal {
                    downloaded_bytes: 0,
                },
            };
        }
    }

    /// 用 Adapter 报告的已知或未知总量更新完整下载进度。
    fn advance_download(&mut self, progress: DownloadProgress) {
        if let Some(release) = &self.current_release {
            self.snapshot.state = UpdateState::Downloading {
                version: release.version().to_owned(),
                release_notes: release.release_notes().to_owned(),
                progress,
            };
        }
    }

    /// 将成功持久化的包表示为 staged 领域状态。
    fn finish_download(&mut self) {
        self.operation_active = false;
        self.pending_retry = None;
        if let Some(release) = &self.current_release {
            self.staged_version = Version::parse(release.version()).ok();
            self.snapshot.state = UpdateState::Staged {
                version: release.version().to_owned(),
                release_notes: release.release_notes().to_owned(),
            };
        }
    }

    /// 记录失败，并只为稳定可重试类别安排一、二、四秒三次重试。
    fn accept_failure(
        &mut self,
        operation: UpdateOperation,
        failure: UpdateFailure,
        effects: &mut Vec<UpdateEffect>,
    ) {
        self.operation_active = false;
        let prior_retries = self
            .pending_retry
            .as_ref()
            .filter(|pending| pending.operation == operation)
            .map_or(0, |pending| pending.automatic_retries);
        let retries = if failure.retryable() && prior_retries < MAX_AUTOMATIC_RETRIES {
            let retries = prior_retries + 1;
            let token = self.next_retry_token;
            self.next_retry_token = self.next_retry_token.saturating_add(1);
            effects.push(UpdateEffect::ScheduleRetry {
                token,
                operation,
                after: retry_delay(retries),
            });
            self.pending_retry = Some(PendingRetry {
                token,
                operation,
                automatic_retries: retries,
            });
            retries
        } else {
            prior_retries
        };
        if !failure.retryable() || retries == prior_retries {
            self.pending_retry = Some(PendingRetry {
                token: 0,
                operation,
                automatic_retries: retries,
            });
        }
        self.snapshot.state = self.staged_state_or(UpdateState::Failed {
            operation,
            category: failure.category(),
            retryable: failure.retryable(),
            message: failure.message().to_owned(),
        });
    }

    /// 在有效计时器到期时只重放领域已记录的操作。
    fn retry(&mut self, token: u64, effects: &mut Vec<UpdateEffect>) {
        let Some(pending) = &self.pending_retry else {
            return;
        };
        if pending.token != token || token == 0 || self.operation_active {
            return;
        }
        match pending.operation {
            UpdateOperation::Check => {
                self.operation_active = true;
                self.snapshot.state = UpdateState::Checking;
                effects.push(UpdateEffect::CheckStable);
            }
            UpdateOperation::Download => {
                if let Some(release) = self.current_release.clone() {
                    self.operation_active = true;
                    effects.push(UpdateEffect::StartDownload { release });
                }
            }
        }
    }

    /// 按当前失败状态显式重放操作，并使尚未触发的自动 timer 失效。
    fn user_retry(&mut self, effects: &mut Vec<UpdateEffect>) {
        let operation = match &self.snapshot.state {
            UpdateState::Failed {
                operation,
                retryable: true,
                ..
            } => *operation,
            _ => return,
        };
        self.pending_retry = None;
        match operation {
            UpdateOperation::Check => self.start_check(false, effects),
            UpdateOperation::Download => {
                if let Some(release) = self.current_release.clone() {
                    self.operation_active = true;
                    effects.push(UpdateEffect::StartDownload { release });
                }
            }
        }
    }

    /// 只有可信 staging 存在时才向上层请求共享停止与更新安装。
    fn request_update_restart(&self, effects: &mut Vec<UpdateEffect>) {
        if let UpdateState::Staged { version, .. } = &self.snapshot.state {
            effects.push(UpdateEffect::RequestInstall {
                intent: InstallIntent::UpdateRestart,
                version: version.clone(),
            });
        }
    }

    /// 在 staging 仍可信时优先保留可安装状态。
    fn staged_state_or(&self, fallback: UpdateState) -> UpdateState {
        self.staged_version
            .as_ref()
            .map(|version| UpdateState::Staged {
                version: version.to_string(),
                release_notes: String::new(),
            })
            .unwrap_or(fallback)
    }
}

/// 把可信发布投影为不含 URL 与签名的 available 状态。
fn available_state(release: &StableRelease) -> UpdateState {
    UpdateState::Available {
        version: release.version().to_owned(),
        release_notes: release.release_notes().to_owned(),
    }
}

/// 计算第 N 次自动重试的一、二、四秒有界指数延迟。
fn retry_delay(automatic_retry: u8) -> Duration {
    Duration::from_secs(1_u64 << automatic_retry.saturating_sub(1).min(2))
}
