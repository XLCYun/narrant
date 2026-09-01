use super::config::{trusted_redirect_url, TrustedUpdaterConfig};
use super::diagnostics::UpdateDiagnostics;
use super::metadata::BoundedManifestAdapter;
use super::platform::{current_install_plan, PlatformUpdateInstaller};
use super::{
    Clock, DownloadProgress, FilePreferenceStore, InstallExecutionOutcome, InstallIntent,
    StableRelease, StageCandidate, StageCommitOutcome, StagingContext, StagingRepository,
    UpdateController, UpdateEffect, UpdateFailure, UpdateFailureCategory, UpdateInput,
    UpdateOperationId, UpdateSnapshot,
};
use crate::operation::{CancellableOperations, CancellationSignal, OperationId};
use futures_util::StreamExt;
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
        let preferences = FilePreferenceStore::new(data_dir.join("application-update.json"));
        let mut controller = UpdateController::new(&version, SystemClock::new(), preferences)
            .map_err(|_| "应用更新状态无法初始化".to_owned())?;
        let (staging, recovery_failure) = initialize_staging(&cache_dir, &version, &config);
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
            let _ = app.emit(UPDATE_SNAPSHOT_EVENT, &output.snapshot);
        }
        Ok(Arc::new(Self {
            controller: Mutex::new(controller),
            config,
            staging: Mutex::new(staging),
            pending: Mutex::new(None),
            transition_gate: Mutex::new(()),
            check_gate: tokio::sync::Mutex::new(()),
            diagnostics,
            managed_sequence: AtomicU64::new(1),
        }))
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
        let output = {
            let _gate = self
                .transition_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Ok(output) = self
                .controller
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .handle(input)
            else {
                return;
            };
            self.diagnostics.record_snapshot(&output.snapshot);
            let _ = app.emit(UPDATE_SNAPSHOT_EVENT, &output.snapshot);
            output
        };
        for effect in output.effects {
            self.execute_effect(app, effect);
        }
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
        let total = response.content_length();
        let mut temporary = match tempfile::NamedTempFile::new() {
            Ok(file) => file,
            Err(_) => {
                self.fail_download(
                    app,
                    operation_id,
                    UpdateFailureCategory::Storage,
                    "temporary",
                );
                return;
            }
        };
        let mut downloaded = 0_u64;
        let mut stream = response.bytes_stream();
        loop {
            let next = tokio::select! {
                _ = cancellation.cancelled() => return,
                next = stream.next() => next,
            };
            let Some(chunk) = next else { break };
            let chunk = match chunk {
                Ok(chunk) => chunk,
                Err(_) => {
                    self.fail_download(app, operation_id, UpdateFailureCategory::Network, "stream");
                    return;
                }
            };
            if temporary.write_all(&chunk).is_err() {
                self.fail_download(app, operation_id, UpdateFailureCategory::Storage, "write");
                return;
            }
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            let progress = total.map_or(
                DownloadProgress::UnknownTotal {
                    downloaded_bytes: downloaded,
                },
                |total_bytes| DownloadProgress::KnownTotal {
                    downloaded_bytes: downloaded,
                    total_bytes,
                },
            );
            self.dispatch(
                app,
                UpdateInput::DownloadAdvanced {
                    operation_id,
                    progress,
                },
            );
        }
        if temporary.as_file_mut().sync_all().is_err()
            || temporary.as_file_mut().seek(SeekFrom::Start(0)).is_err()
        {
            self.fail_download(app, operation_id, UpdateFailureCategory::Storage, "sync");
            return;
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
                self.fail_download(
                    app,
                    operation_id,
                    UpdateFailureCategory::Manifest,
                    "metadata",
                );
                return;
            }
        };
        if cancellation.is_cancelled() {
            return;
        }
        let output = {
            let _gate = self
                .transition_gate
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut controller = self
                .controller
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !controller.accepts_download_completion(operation_id) {
                return;
            }
            let mut staging = self
                .staging
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(repository) = staging.as_mut() else {
                drop(controller);
                drop(staging);
                drop(_gate);
                self.fail_download(
                    app,
                    operation_id,
                    UpdateFailureCategory::Storage,
                    "repository",
                );
                return;
            };
            let staged = match repository.stage(candidate, temporary.as_file_mut()) {
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
                    drop(controller);
                    drop(staging);
                    drop(_gate);
                    self.fail_download(
                        app,
                        operation_id,
                        UpdateFailureCategory::Verification,
                        "signature",
                    );
                    return;
                }
                Err(_) => {
                    drop(controller);
                    drop(staging);
                    drop(_gate);
                    self.fail_download(app, operation_id, UpdateFailureCategory::Storage, "commit");
                    return;
                }
            }
        };
        if let Some(output) = output {
            self.diagnostics.record_snapshot(&output.snapshot);
            let _ = app.emit(UPDATE_SNAPSHOT_EVENT, &output.snapshot);
            for effect in output.effects {
                self.execute_effect(app, effect);
            }
        }
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
        repository
            .install_with(&installer, intent)
            .map(Some)
            .map_err(|error| match error {
                super::InstallAttemptError::Staging(_) => "应用更新暂存复验失败".to_owned(),
                super::InstallAttemptError::Installer(error) => error.message().to_owned(),
            })
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
