use super::config::{trusted_redirect_url, TrustedUpdaterConfig};
use super::diagnostics::UpdateDiagnostics;
use super::metadata::BoundedManifestAdapter;
use super::platform::{current_install_plan, PlatformUpdateInstaller};
use super::{
    Clock, DownloadProgress, FilePreferenceStore, InstallExecutionOutcome, InstallIntent,
    StableRelease, StageCandidate, StageCommitOutcome, StagingContext, StagingRepository,
    UpdateController, UpdateEffect, UpdateFailure, UpdateFailureCategory, UpdateInput,
    UpdateOperationId, UpdateOutput, UpdateSnapshot,
};
use crate::operation::{CancellableOperations, CancellationSignal, OperationId};
use futures_util::{Stream, StreamExt};
use std::io::{Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};

use super::verifier::DiskMinisignVerifier;
use crate::tauri_app::shutdown::{PostStopCoordinator, PostStopIntent};

pub(crate) const UPDATE_SNAPSHOT_EVENT: &str = "application-update:snapshot";
const PACKAGE_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const PACKAGE_READ_TIMEOUT: Duration = Duration::from_secs(60);

/// 使用进程内单调时间驱动六小时调度与 retry timer。
pub(crate) struct SystemClock {
    started: Instant,
}

impl SystemClock {
    /// 创建从当前进程装配时刻起算的单调时钟。
    fn new() -> Self {
        Self {
            started: Instant::now(),
        }
    }
}

impl Clock for SystemClock {
    /// 返回从 runtime 装配后经过的单调时长。
    fn now(&self) -> Duration {
        self.started.elapsed()
    }
}

/// 保存有界 trusted metadata adapter 返回的候选及其领域 operation identity。
struct PendingUpdate {
    operation_id: UpdateOperationId,
    release: StableRelease,
}

/// 一次受管 stream→durable/core 提交的稳定结果。
enum DownloadTaskOutcome {
    Committed(UpdateOutput),
    Cancelled,
    Failed(UpdateFailureCategory, &'static str),
}

/// Tauri-managed 的 Application Update 运行时与单一快照发布 owner。
pub(crate) struct ApplicationUpdateRuntime {
    controller: Mutex<UpdateController<SystemClock, FilePreferenceStore>>,
    config: Result<TrustedUpdaterConfig, super::config::UpdaterConfigError>,
    staging: Mutex<Option<StagingRepository<DiskMinisignVerifier>>>,
    pending: Mutex<Option<PendingUpdate>>,
    transition_gate: Mutex<()>,
    check_gate: tokio::sync::Mutex<()>,
    diagnostics: UpdateDiagnostics,
    managed_sequence: AtomicU64,
}

impl ApplicationUpdateRuntime {
    /// 从应用私有目录、当前版本和编译期 trust root 创建 fail-closed runtime。
    pub(crate) fn new<R: tauri::Runtime>(
        app: &tauri::AppHandle<R>,
        config: Result<TrustedUpdaterConfig, super::config::UpdaterConfigError>,
    ) -> Result<Arc<Self>, String> {
        let data_dir = app
            .path()
            .app_data_dir()
            .map_err(|_| "应用更新目录不可用".to_owned())?;
        let cache_dir = app
            .path()
            .app_cache_dir()
            .map_err(|_| "应用更新目录不可用".to_owned())?;
        let version = app.package_info().version.to_string();
        let (runtime, startup_output) = Self::new_at_paths(data_dir, cache_dir, &version, config)?;
        if let Some(output) = startup_output {
            let _ = app.emit(UPDATE_SNAPSHOT_EVENT, &output.snapshot);
        }
        Ok(runtime)
    }

    /// 从显式私有目录创建 runtime，生产路径与真实文件系统组合测试共用。
    fn new_at_paths(
        data_dir: std::path::PathBuf,
        cache_dir: std::path::PathBuf,
        version: &str,
        config: Result<TrustedUpdaterConfig, super::config::UpdaterConfigError>,
    ) -> Result<(Arc<Self>, Option<UpdateOutput>), String> {
        let preferences = FilePreferenceStore::new(data_dir.join("application-update.json"));
        let mut controller = UpdateController::new(version, SystemClock::new(), preferences)
            .map_err(|_| "应用更新状态无法初始化".to_owned())?;
        let (staging, recovery_failure) = initialize_staging(&cache_dir, version, &config);
        if let Some(staged) = staging.as_ref().and_then(StagingRepository::snapshot) {
            let _ = controller.handle(UpdateInput::RecoverStaged { staged });
        }
        let startup_output = recovery_failure.and_then(|failure| {
            controller
                .handle(UpdateInput::StartupRecoveryFailed { failure })
                .ok()
        });
        let diagnostics =
            UpdateDiagnostics::new(data_dir.join("logs").join("application-update.jsonl"));
        if let Some(output) = &startup_output {
            diagnostics.record_snapshot(&output.snapshot);
        }
        let runtime = Arc::new(Self {
            controller: Mutex::new(controller),
            config,
            staging: Mutex::new(staging),
            pending: Mutex::new(None),
            transition_gate: Mutex::new(()),
            check_gate: tokio::sync::Mutex::new(()),
            diagnostics,
            managed_sequence: AtomicU64::new(1),
        });
        Ok((runtime, startup_output))
    }

    /// 返回当前 provider-neutral 完整快照，不接受任何资源参数。
    pub(crate) fn snapshot(&self) -> UpdateSnapshot {
        self.controller
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot()
            .clone()
    }

    /// 提交可信输入、按 sequence 发布唯一 snapshot event，再执行窄化副作用。
    pub(crate) fn dispatch<R: tauri::Runtime>(
        self: &Arc<Self>,
        app: &tauri::AppHandle<R>,
        input: UpdateInput,
    ) {
        let Some(output) = self.transition(input) else {
            return;
        };
        let _ = app.emit(UPDATE_SNAPSHOT_EVENT, &output.snapshot);
        for effect in output.effects {
            self.execute_effect(app, effect);
        }
    }

    /// 在单一 gate 内执行领域转换、收敛领域错误并持久化脱敏快照。
    fn transition(&self, input: UpdateInput) -> Option<UpdateOutput> {
        let _gate = self
            .transition_gate
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut controller = self
            .controller
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let input_for_failure = input.clone();
        let output = match controller.handle(input) {
            Ok(output) => output,
            Err(error) => {
                let (operation_id, operation, category, diagnostic) =
                    domain_failure_context(&input_for_failure, &error);
                let failure = UpdateFailure::new(category, diagnostic);
                self.diagnostics.record_failure(operation_id, &failure);
                controller
                    .handle(UpdateInput::RuntimeDomainFailed { operation, failure })
                    .ok()?
            }
        };
        self.diagnostics.record_snapshot(&output.snapshot);
        Some(output)
    }

    /// 在 Tauri runtime 上执行 controller 产生的 check/download/timer/install effect。
    fn execute_effect<R: tauri::Runtime>(
        self: &Arc<Self>,
        app: &tauri::AppHandle<R>,
        effect: UpdateEffect,
    ) {
        match effect {
            UpdateEffect::ScheduleCheck { after } => {
                let task_id = self.next_managed_id("periodic");
                let runtime = self.clone();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let operations = app.state::<CancellableOperations>().inner().clone();
                    let _ = operations.run(task_id, |context| async move {
                        tokio::select! {
                            _ = context.cancellation().cancelled() => {},
                            _ = tokio::time::sleep(after) => runtime.dispatch(&app, UpdateInput::PeriodicCheckDue),
                        }
                    }).await;
                });
            }
            UpdateEffect::ScheduleRetry { token, after, .. } => {
                let task_id = self.next_managed_id("retry");
                let runtime = self.clone();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let operations = app.state::<CancellableOperations>().inner().clone();
                    let _ = operations.run(task_id, |context| async move {
                        tokio::select! {
                            _ = context.cancellation().cancelled() => {},
                            _ = tokio::time::sleep(after) => runtime.dispatch(&app, UpdateInput::RetryDue { token }),
                        }
                    }).await;
                });
            }
            UpdateEffect::CheckStable { operation_id } => {
                let runtime = self.clone();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let operations = app.state::<CancellableOperations>().inner().clone();
                    let id = managed_operation_id("check", operation_id.value());
                    let _ = operations
                        .run(id, |context| async move {
                            runtime
                                .check_stable(&app, operation_id, context.cancellation())
                                .await;
                        })
                        .await;
                });
            }
            UpdateEffect::StartDownload {
                operation_id,
                release,
            } => {
                let runtime = self.clone();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let operations = app.state::<CancellableOperations>().inner().clone();
                    let id = managed_operation_id("download", operation_id.value());
                    let _ = operations
                        .run(id, |context| async move {
                            runtime
                                .download_pending(
                                    &app,
                                    operation_id,
                                    release,
                                    context.cancellation(),
                                )
                                .await;
                        })
                        .await;
                });
            }
            UpdateEffect::CancelDownload { operation_id } => {
                let operations = app.state::<CancellableOperations>();
                let _ = operations.cancel(&managed_operation_id("download", operation_id.value()));
            }
            UpdateEffect::PersistStagedReleaseNotes {
                version,
                release_notes,
            } => {
                if let Some(repository) = self
                    .staging
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .as_mut()
                {
                    let _ = repository.update_release_notes(&version, &release_notes);
                }
            }
            UpdateEffect::RequestInstall { intent, version } => {
                self.spawn_explicit_install(app, intent, version);
            }
        }
    }

    /// 通过同一 endpoint/public key 的 Tauri builder 获取并绑定可信候选。
    async fn check_stable<R: tauri::Runtime>(
        self: Arc<Self>,
        app: &tauri::AppHandle<R>,
        operation_id: UpdateOperationId,
        cancellation: &CancellationSignal,
    ) {
        let _check = tokio::select! {
            _ = cancellation.cancelled() => return,
            guard = self.check_gate.lock() => guard,
        };
        let trusted = match &self.config {
            Ok(config) => config,
            Err(_) => {
                self.fail_check(app, operation_id, UpdateFailureCategory::Manifest, "config");
                return;
            }
        };
        let (target, install_kind) = match current_install_plan() {
            Ok(plan) => plan,
            Err(_) => {
                self.fail_check(app, operation_id, UpdateFailureCategory::Manifest, "target");
                return;
            }
        };
        let adapter = match BoundedManifestAdapter::new(
            trusted,
            target,
            install_kind,
            app.package_info().version.to_string().as_str(),
        ) {
            Ok(adapter) => adapter,
            Err(_) => {
                self.fail_check(
                    app,
                    operation_id,
                    UpdateFailureCategory::Manifest,
                    "builder",
                );
                return;
            }
        };
        let result = tokio::select! {
            _ = cancellation.cancelled() => return,
            result = adapter.check() => result,
        };
        match result {
            Ok(Some(release)) => {
                *self
                    .pending
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(PendingUpdate {
                    operation_id,
                    release: release.clone(),
                });
                self.dispatch(
                    app,
                    UpdateInput::CheckSucceeded {
                        operation_id,
                        release: Some(release),
                    },
                );
            }
            Ok(None) => self.dispatch(
                app,
                UpdateInput::CheckSucceeded {
                    operation_id,
                    release: None,
                },
            ),
            Err(error) => {
                self.diagnostics.record_failure_with_status(
                    operation_id.value(),
                    error.failure(),
                    error.status(),
                );
                self.dispatch(
                    app,
                    UpdateInput::CheckFailed {
                        operation_id,
                        failure: error.failure().clone(),
                    },
                );
            }
        }
    }

    /// 发布一次带原 operation identity 的脱敏检查失败。
    fn fail_check<R: tauri::Runtime>(
        self: &Arc<Self>,
        app: &tauri::AppHandle<R>,
        operation_id: UpdateOperationId,
        category: UpdateFailureCategory,
        diagnostic: &'static str,
    ) {
        let failure = UpdateFailure::new(category, diagnostic);
        self.diagnostics
            .record_failure(operation_id.value(), &failure);
        self.dispatch(
            app,
            UpdateInput::CheckFailed {
                operation_id,
                failure,
            },
        );
    }

    /// 只下载同一 trusted builder 为当前 operation 返回的候选，并流式提交 staging。
    async fn download_pending<R: tauri::Runtime>(
        self: Arc<Self>,
        app: &tauri::AppHandle<R>,
        operation_id: UpdateOperationId,
        release: StableRelease,
        cancellation: &CancellationSignal,
    ) {
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|pending| pending.operation_id == operation_id && pending.release == release)
            .map(|pending| pending.release.clone());
        let Some(trusted_release) = pending else {
            self.fail_download(
                app,
                operation_id,
                UpdateFailureCategory::Manifest,
                "pending",
            );
            return;
        };
        self.dispatch(app, UpdateInput::DownloadStarted { operation_id });
        let client = match reqwest::Client::builder()
            .connect_timeout(PACKAGE_CONNECT_TIMEOUT)
            .read_timeout(PACKAGE_READ_TIMEOUT)
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 5 {
                    attempt.stop()
                } else if trusted_redirect_url(attempt.url()) {
                    attempt.follow()
                } else {
                    attempt.error("untrusted updater redirect")
                }
            }))
            .build()
        {
            Ok(client) => client,
            Err(_) => {
                self.fail_download(app, operation_id, UpdateFailureCategory::Network, "client");
                return;
            }
        };
        let response = tokio::select! {
            _ = cancellation.cancelled() => return,
            response = client.get(trusted_release.package().download_url()).send() => response,
        };
        let response = match response {
            Ok(response) => response,
            Err(_) => {
                self.fail_download(app, operation_id, UpdateFailureCategory::Network, "request");
                return;
            }
        };
        if !response.status().is_success() {
            self.fail_download(
                app,
                operation_id,
                UpdateFailure::from_http_status(response.status().as_u16(), "status").category(),
                "status",
            );
            return;
        }
        let runtime_for_progress = self.clone();
        let outcome = self
            .consume_stream_and_commit(
                operation_id,
                release,
                response.content_length(),
                response.bytes_stream(),
                cancellation,
                |progress| {
                    runtime_for_progress.dispatch(
                        app,
                        UpdateInput::DownloadAdvanced {
                            operation_id,
                            progress,
                        },
                    );
                },
                || {},
            )
            .await;
        match outcome {
            DownloadTaskOutcome::Committed(output) => {
                self.diagnostics.record_snapshot(&output.snapshot);
                let _ = app.emit(UPDATE_SNAPSHOT_EVENT, &output.snapshot);
                for effect in output.effects {
                    self.execute_effect(app, effect);
                }
            }
            DownloadTaskOutcome::Cancelled => {}
            DownloadTaskOutcome::Failed(category, diagnostic) => {
                self.fail_download(app, operation_id, category, diagnostic);
            }
        }
    }

    /// 流式写临时文件并执行与生产下载共用的 EOF、取消、验签和 durable commit 边界。
    async fn consume_stream_and_commit<S, E, F, G>(
        self: &Arc<Self>,
        operation_id: UpdateOperationId,
        release: StableRelease,
        total: Option<u64>,
        stream: S,
        cancellation: &CancellationSignal,
        mut on_progress: F,
        before_commit: G,
    ) -> DownloadTaskOutcome
    where
        S: Stream<Item = Result<bytes::Bytes, E>>,
        F: FnMut(DownloadProgress),
        G: FnOnce(),
    {
        let mut temporary = match tempfile::NamedTempFile::new() {
            Ok(file) => file,
            Err(_) => {
                return DownloadTaskOutcome::Failed(UpdateFailureCategory::Storage, "temporary")
            }
        };
        let mut downloaded = 0_u64;
        futures_util::pin_mut!(stream);
        loop {
            let next = tokio::select! {
                _ = cancellation.cancelled() => return DownloadTaskOutcome::Cancelled,
                next = stream.next() => next,
            };
            let Some(chunk) = next else { break };
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) => {
                    return DownloadTaskOutcome::Failed(UpdateFailureCategory::Network, "stream")
                }
            };
            if temporary.write_all(&chunk).is_err() {
                return DownloadTaskOutcome::Failed(UpdateFailureCategory::Storage, "write");
            }
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            on_progress(total.map_or(
                DownloadProgress::UnknownTotal {
                    downloaded_bytes: downloaded,
                },
                |total_bytes| DownloadProgress::KnownTotal {
                    downloaded_bytes: downloaded,
                    total_bytes,
                },
            ));
        }
        if cancellation.is_cancelled() {
            return DownloadTaskOutcome::Cancelled;
        }
        if temporary.as_file_mut().sync_all().is_err()
            || temporary.as_file_mut().seek(SeekFrom::Start(0)).is_err()
        {
            return DownloadTaskOutcome::Failed(UpdateFailureCategory::Storage, "sync");
        }
        let candidate = match StageCandidate::new(
            release.version(),
            release.package().signature(),
            release.package().target(),
            release.package().install_kind(),
            release.release_notes(),
        ) {
            Ok(candidate) => candidate,
            Err(_) => {
                return DownloadTaskOutcome::Failed(UpdateFailureCategory::Manifest, "metadata")
            }
        };
        before_commit();
        self.commit_download_file(
            operation_id,
            candidate,
            temporary.as_file_mut(),
            cancellation,
        )
    }

    /// 在 transition gate 内再次观察取消并原子提交磁盘 proof 与 core identity。
    fn commit_download_file(
        &self,
        operation_id: UpdateOperationId,
        candidate: StageCandidate,
        package: &mut std::fs::File,
        cancellation: &CancellationSignal,
    ) -> DownloadTaskOutcome {
        {
            let _gate = self
                .transition_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if cancellation.is_cancelled() {
                return DownloadTaskOutcome::Cancelled;
            }
            let mut controller = self
                .controller
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !controller.accepts_download_completion(operation_id) {
                return DownloadTaskOutcome::Cancelled;
            }
            let mut staging = self
                .staging
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(repository) = staging.as_mut() else {
                drop(controller);
                drop(staging);
                drop(_gate);
                return DownloadTaskOutcome::Failed(UpdateFailureCategory::Storage, "repository");
            };
            let staged = match repository.stage(candidate, package) {
                Ok(StageCommitOutcome::Committed(staged)) => Ok(staged),
                Ok(StageCommitOutcome::DurabilityUnknown(pending)) => {
                    repository.confirm_durability(&pending)
                }
                Err(error) => Err(error),
            };
            match staged {
                Ok(staged) => controller
                    .handle(UpdateInput::DownloadSucceeded {
                        operation_id,
                        staged,
                    })
                    .ok(),
                Err(super::StagingError::VerificationFailed) => {
                    return DownloadTaskOutcome::Failed(
                        UpdateFailureCategory::Verification,
                        "signature",
                    )
                }
                Err(_) => {
                    return DownloadTaskOutcome::Failed(UpdateFailureCategory::Storage, "commit")
                }
            }
        }
        .map_or(
            DownloadTaskOutcome::Failed(UpdateFailureCategory::Manifest, "core-commit"),
            DownloadTaskOutcome::Committed,
        )
    }

    /// 发布一次带原 operation identity 的脱敏下载失败。
    fn fail_download<R: tauri::Runtime>(
        self: &Arc<Self>,
        app: &tauri::AppHandle<R>,
        operation_id: UpdateOperationId,
        category: UpdateFailureCategory,
        diagnostic: &'static str,
    ) {
        let failure = UpdateFailure::new(category, diagnostic);
        self.diagnostics
            .record_failure(operation_id.value(), &failure);
        self.dispatch(
            app,
            UpdateInput::DownloadFailed {
                operation_id,
                failure,
            },
        );
    }

    /// 在共享 stop Completed 之后同步复验并执行一次 staging install transaction。
    pub(crate) fn try_install_after_completed_stop<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        intent: InstallIntent,
    ) -> Result<Option<InstallExecutionOutcome>, String> {
        let installer = PlatformUpdateInstaller::new(app);
        self.try_use_staged(|repository| {
            repository
                .install_with(&installer, intent)
                .map_err(|error| match error {
                    super::InstallAttemptError::Staging(_) => "应用更新暂存复验失败".to_owned(),
                    super::InstallAttemptError::Installer(error) => error.message().to_owned(),
                })
        })
    }

    /// 非阻塞取得 staging mutex，并只在存在 durable proof 时执行 closure。
    fn try_use_staged<T>(
        &self,
        use_staged: impl FnOnce(&mut StagingRepository<DiskMinisignVerifier>) -> Result<T, String>,
    ) -> Result<Option<T>, String> {
        let mut staging = self
            .staging
            .try_lock()
            .map_err(|_| "更新暂存仍被受管任务占用，跳过本次安装".to_owned())?;
        let Some(repository) = staging.as_mut() else {
            return Ok(None);
        };
        if repository.snapshot().is_none() {
            return Ok(None);
        }
        use_staged(repository).map(Some)
    }

    /// 显式 restart 复用 ApplicationShutdown outcome；成功安装后才 relaunch/exit。
    fn spawn_explicit_install<R: tauri::Runtime>(
        self: &Arc<Self>,
        app: &tauri::AppHandle<R>,
        intent: InstallIntent,
        version: String,
    ) {
        if intent != InstallIntent::UpdateRestart
            || self
                .snapshot()
                .candidate
                .as_ref()
                .is_none_or(|candidate| candidate.version != version)
        {
            return;
        }
        app.state::<Arc<PostStopCoordinator>>()
            .request(app.clone(), PostStopIntent::UpdateRestart);
    }

    /// 为每个 timer 分配不重用的 registry identity，避免旧 guard 与后继任务竞态。
    fn next_managed_id(&self, kind: &str) -> OperationId {
        managed_operation_id(kind, self.managed_sequence.fetch_add(1, Ordering::Relaxed))
    }
}

/// 将 controller 领域错误转换为有序、可诊断且不泄露输入的失败事实。
fn domain_failure_context(
    input: &UpdateInput,
    error: &super::UpdateDomainError,
) -> (
    u64,
    super::UpdateOperation,
    UpdateFailureCategory,
    &'static str,
) {
    let (operation_id, operation) = match input {
        UpdateInput::CheckSucceeded { operation_id, .. }
        | UpdateInput::CheckFailed { operation_id, .. } => {
            (operation_id.value(), super::UpdateOperation::Check)
        }
        UpdateInput::DownloadStarted { operation_id }
        | UpdateInput::DownloadAdvanced { operation_id, .. }
        | UpdateInput::DownloadSucceeded { operation_id, .. }
        | UpdateInput::DownloadFailed { operation_id, .. } => {
            (operation_id.value(), super::UpdateOperation::Download)
        }
        _ => (0, super::UpdateOperation::Download),
    };
    match error {
        super::UpdateDomainError::Preference(_) => (
            operation_id,
            operation,
            UpdateFailureCategory::Storage,
            "domain-preference",
        ),
        _ => (
            operation_id,
            operation,
            UpdateFailureCategory::Manifest,
            "domain-contract",
        ),
    }
}

/// 显式区分仓储可用但安全丢弃损坏候选，与仓储根目录完全不可用。
fn initialize_staging(
    cache_dir: &std::path::Path,
    version: &str,
    config: &Result<TrustedUpdaterConfig, super::config::UpdaterConfigError>,
) -> (
    Option<StagingRepository<DiskMinisignVerifier>>,
    Option<UpdateFailure>,
) {
    let trusted = match config {
        Ok(trusted) => trusted,
        Err(_) => {
            return (
                None,
                Some(UpdateFailure::new(
                    UpdateFailureCategory::Manifest,
                    "config",
                )),
            )
        }
    };
    let (target, install_kind) = match current_install_plan() {
        Ok(plan) => plan,
        Err(_) => {
            return (
                None,
                Some(UpdateFailure::new(
                    UpdateFailureCategory::Manifest,
                    "target",
                )),
            )
        }
    };
    let context = match StagingContext::new(version, target, install_kind) {
        Ok(context) => context,
        Err(_) => {
            return (
                None,
                Some(UpdateFailure::new(
                    UpdateFailureCategory::Storage,
                    "context",
                )),
            )
        }
    };
    let verifier = match DiskMinisignVerifier::new(trusted.public_key()) {
        Ok(verifier) => verifier,
        Err(_) => {
            return (
                None,
                Some(UpdateFailure::new(UpdateFailureCategory::Manifest, "key")),
            )
        }
    };
    match StagingRepository::open(cache_dir, context, verifier) {
        Ok(repository) => {
            let failure = repository.recovery_failure().map(|error| {
                let category = if error == super::StagingError::VerificationFailed {
                    UpdateFailureCategory::Verification
                } else {
                    UpdateFailureCategory::Storage
                };
                UpdateFailure::new(category, "recovery-discarded")
            });
            (Some(repository), failure)
        }
        Err(_) => (
            None,
            Some(UpdateFailure::new(
                UpdateFailureCategory::Storage,
                "repository-open",
            )),
        ),
    }
}

/// 为 updater task 创建不会与业务 operation ID 冲突的 registry identity。
fn managed_operation_id(kind: &str, value: u64) -> OperationId {
    OperationId::new(format!("application-update:{kind}:{value}"))
        .expect("bounded internal updater operation id")
}

#[cfg(test)]
#[path = "tests/runtime.rs"]
mod tests;
