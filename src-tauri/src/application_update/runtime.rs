use super::config::{trusted_package_url, trusted_redirect_url, TrustedUpdaterConfig};
use super::diagnostics::UpdateDiagnostics;
use super::platform::{current_install_plan, PlatformUpdateInstaller};
use super::{
    Clock, DownloadProgress, FilePreferenceStore, InstallExecutionOutcome, InstallIntent,
    ReleasePackage, StableRelease, StageCandidate, StageCommitOutcome, StagingContext,
    StagingRepository, UpdateController, UpdateEffect, UpdateFailure, UpdateFailureCategory,
    UpdateInput, UpdateOperationId, UpdateSnapshot,
};
use futures_util::StreamExt;
use std::collections::HashMap;
use std::io::{Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio_util::sync::CancellationToken;

use super::verifier::DiskMinisignVerifier;
use crate::tauri_app::shutdown::{
    install_after_successful_stop, ApplicationShutdown, ApplicationStopIntent,
};

pub(crate) const UPDATE_SNAPSHOT_EVENT: &str = "application-update:snapshot";
const MANIFEST_TIMEOUT: Duration = Duration::from_secs(30);
const PACKAGE_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);
const PACKAGE_READ_TIMEOUT: Duration = Duration::from_secs(60);
const MAX_MANIFEST_BYTES: usize = 64 * 1024;

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

/// 保存 Tauri trusted builder 返回的候选及其领域 operation identity。
struct PendingUpdate {
    operation_id: UpdateOperationId,
    update: Update,
}

/// Tauri-managed 的 Application Update 运行时与单一快照发布 owner。
pub(crate) struct ApplicationUpdateRuntime {
    controller: Mutex<UpdateController<SystemClock, FilePreferenceStore>>,
    config: Result<TrustedUpdaterConfig, super::config::UpdaterConfigError>,
    staging: Mutex<Option<StagingRepository<DiskMinisignVerifier>>>,
    pending: Mutex<Option<PendingUpdate>>,
    downloads: Mutex<HashMap<u64, CancellationToken>>,
    transition_gate: Mutex<()>,
    check_gate: tokio::sync::Mutex<()>,
    install_requested: AtomicBool,
    diagnostics: UpdateDiagnostics,
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
        let staging = config.as_ref().ok().and_then(|trusted| {
            let (target, install_kind) = current_install_plan().ok()?;
            let context = StagingContext::new(&version, target, install_kind).ok()?;
            let verifier = DiskMinisignVerifier::new(trusted.public_key()).ok()?;
            StagingRepository::open(cache_dir, context, verifier).ok()
        });
        if let Some(staged) = staging.as_ref().and_then(StagingRepository::snapshot) {
            let _ = controller.handle(UpdateInput::RecoverStaged { staged });
        }
        Ok(Arc::new(Self {
            controller: Mutex::new(controller),
            config,
            staging: Mutex::new(staging),
            pending: Mutex::new(None),
            downloads: Mutex::new(HashMap::new()),
            transition_gate: Mutex::new(()),
            check_gate: tokio::sync::Mutex::new(()),
            install_requested: AtomicBool::new(false),
            diagnostics: UpdateDiagnostics::new(
                data_dir.join("logs").join("application-update.jsonl"),
            ),
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

    /// 判断仓储是否持有已经跨过 durability barrier 的可安装候选。
    pub(crate) fn has_durable_staged_update(&self) -> bool {
        self.staging
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .and_then(StagingRepository::snapshot)
            .is_some()
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
                let runtime = self.clone();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(after).await;
                    runtime.dispatch(&app, UpdateInput::PeriodicCheckDue);
                });
            }
            UpdateEffect::ScheduleRetry { token, after, .. } => {
                let runtime = self.clone();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    tokio::time::sleep(after).await;
                    runtime.dispatch(&app, UpdateInput::RetryDue { token });
                });
            }
            UpdateEffect::CheckStable { operation_id } => {
                let runtime = self.clone();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    runtime.check_stable(&app, operation_id).await;
                });
            }
            UpdateEffect::StartDownload {
                operation_id,
                release,
            } => {
                let token = CancellationToken::new();
                self.downloads
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(operation_id.value(), token.clone());
                let runtime = self.clone();
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    runtime
                        .download_pending(&app, operation_id, release, token)
                        .await;
                });
            }
            UpdateEffect::CancelDownload { operation_id } => {
                if let Some(token) = self
                    .downloads
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .remove(&operation_id.value())
                {
                    token.cancel();
                }
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
    ) {
        let _check = self.check_gate.lock().await;
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
        let updater = match app
            .updater_builder()
            .target(target)
            .timeout(MANIFEST_TIMEOUT)
            .endpoints(vec![trusted.endpoint().clone()])
            .map(|builder| builder.pubkey(trusted.public_key()))
            .and_then(|builder| builder.build())
        {
            Ok(updater) => updater,
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
        match updater.check().await {
            Ok(Some(update)) => {
                if update.target != target
                    || update.signature.trim().is_empty()
                    || !trusted_package_url(&update.download_url)
                    || !manifest_within_limit(&update.raw_json)
                {
                    self.fail_check(
                        app,
                        operation_id,
                        UpdateFailureCategory::Manifest,
                        "candidate",
                    );
                    return;
                }
                let package = match ReleasePackage::new(
                    update.download_url.to_string(),
                    update.signature.clone(),
                    target,
                    install_kind,
                ) {
                    Ok(package) => package,
                    Err(_) => {
                        self.fail_check(
                            app,
                            operation_id,
                            UpdateFailureCategory::Manifest,
                            "package",
                        );
                        return;
                    }
                };
                let release = StableRelease::new(
                    update.version.clone(),
                    update.body.clone().unwrap_or_default(),
                    package,
                );
                *self
                    .pending
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(PendingUpdate {
                    operation_id,
                    update,
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
            Err(_) => self.fail_check(app, operation_id, UpdateFailureCategory::Network, "request"),
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
        cancellation: CancellationToken,
    ) {
        let pending = self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_ref()
            .filter(|pending| {
                pending.operation_id == operation_id
                    && pending.update.version == release.version()
                    && pending.update.download_url.as_str() == release.package().download_url()
                    && pending.update.signature == release.package().signature()
            })
            .map(|pending| pending.update.clone());
        let Some(update) = pending else {
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
            response = client.get(update.download_url.clone()).headers(update.headers.clone()).send() => response,
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
        let staged = {
            let mut staging = self
                .staging
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let Some(repository) = staging.as_mut() else {
                self.fail_download(
                    app,
                    operation_id,
                    UpdateFailureCategory::Storage,
                    "repository",
                );
                return;
            };
            match repository.stage(candidate, temporary.as_file_mut()) {
                Ok(StageCommitOutcome::Committed(staged)) => Ok(staged),
                Ok(StageCommitOutcome::DurabilityUnknown(pending)) => {
                    repository.confirm_durability(&pending)
                }
                Err(error) => Err(error),
            }
        };
        match staged {
            Ok(staged) => self.dispatch(
                app,
                UpdateInput::DownloadSucceeded {
                    operation_id,
                    staged,
                },
            ),
            Err(super::StagingError::VerificationFailed) => self.fail_download(
                app,
                operation_id,
                UpdateFailureCategory::Verification,
                "signature",
            ),
            Err(_) => {
                self.fail_download(app, operation_id, UpdateFailureCategory::Storage, "commit")
            }
        }
        self.downloads
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&operation_id.value());
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
    pub(crate) fn install_after_completed_stop<R: tauri::Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        intent: InstallIntent,
    ) -> Result<InstallExecutionOutcome, String> {
        let installer = PlatformUpdateInstaller::new(app);
        self.staging
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .as_mut()
            .ok_or_else(|| "没有可安装的可信应用更新".to_owned())?
            .install_with(&installer, intent)
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
        if self
            .install_requested
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let runtime = self.clone();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let outcome = app
                .state::<ApplicationShutdown>()
                .stop_once(&app, ApplicationStopIntent::UpdateRestart)
                .await;
            let Some(installed) = install_after_successful_stop(outcome, || {
                runtime.install_after_completed_stop(&app, intent)
            }) else {
                return;
            };
            let installed = match installed {
                Ok(installed) => installed,
                Err(error) => {
                    tracing::warn!(target: "narrant::application_update", %error, "更新重启安装失败，退出已停止的旧进程");
                    app.state::<ApplicationShutdown>().mark_final_exit();
                    app.exit(1);
                    return;
                }
            };
            app.state::<ApplicationShutdown>().mark_final_exit();
            match installed {
                InstallExecutionOutcome::Installed { .. } => app.restart(),
                InstallExecutionOutcome::HandedOff => app.exit(0),
            }
        });
    }
}

/// 对 trusted builder 解析后的 manifest 继续施加序列化字节上限。
fn manifest_within_limit(manifest: &serde_json::Value) -> bool {
    serde_json::to_vec(manifest)
        .map(|bytes| bytes.len() <= MAX_MANIFEST_BYTES)
        .unwrap_or(false)
}

#[cfg(test)]
#[path = "tests/runtime.rs"]
mod tests;
