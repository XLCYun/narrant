use super::*;
use crate::application_update::{
    PreferenceCommitPoint, PreferenceError, PreferenceFaultInjector, PreferenceSaveOutcome,
    PreferenceStore, UpdatePreferenceWarningKind,
};
use base64::Engine;
use bytes::Bytes;
use std::io::Cursor;
use std::sync::Arc;
use std::time::Duration;

const PUBLIC_KEY: &str = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
const SIGNATURE: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";

/// 在一个精确偏好提交点注入稳定存储失败。
struct PreferenceFailAt(PreferenceCommitPoint);

impl PreferenceFaultInjector for PreferenceFailAt {
    /// 只拒绝指定提交点，其他真实文件系统步骤继续执行。
    fn checkpoint(&self, point: PreferenceCommitPoint) -> Result<(), PreferenceError> {
        if point == self.0 {
            Err(PreferenceError::Storage)
        } else {
            Ok(())
        }
    }
}

fn encode(value: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(value)
}

fn trusted_config() -> Result<TrustedUpdaterConfig, super::super::config::UpdaterConfigError> {
    TrustedUpdaterConfig::parse(
        Some(super::super::config::PRODUCTION_UPDATE_ENDPOINT),
        Some(&encode(PUBLIC_KEY)),
    )
}

/// 创建真实 verifier/staging runtime 并推进到 Downloading。
fn downloading_runtime() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    Arc<ApplicationUpdateRuntime>,
    UpdateOperationId,
    StableRelease,
) {
    let cache = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let (runtime, _) = ApplicationUpdateRuntime::new_at_paths(
        data.path().to_path_buf(),
        cache.path().to_path_buf(),
        "1.0.0",
        trusted_config(),
    )
    .unwrap();
    let ready = runtime.transition(UpdateInput::Ready).unwrap();
    let check_id = ready
        .effects
        .into_iter()
        .find_map(|effect| match effect {
            UpdateEffect::CheckStable { operation_id } => Some(operation_id),
            _ => None,
        })
        .unwrap();
    let (target, install_kind) = current_install_plan().unwrap();
    let package = super::super::ReleasePackage::new(
        "https://github.com/XLCYun/narrant/releases/download/v2.0.0/Narrant.tar.gz",
        encode(SIGNATURE),
        target,
        install_kind,
    )
    .unwrap();
    let release = StableRelease::new("2.0.0", "notes", package);
    let available = runtime
        .transition(UpdateInput::CheckSucceeded {
            operation_id: check_id,
            release: Some(release.clone()),
        })
        .unwrap();
    let download_id = available
        .effects
        .into_iter()
        .find_map(|effect| match effect {
            UpdateEffect::StartDownload { operation_id, .. } => Some(operation_id),
            _ => None,
        })
        .unwrap();
    runtime
        .transition(UpdateInput::DownloadStarted {
            operation_id: download_id,
        })
        .unwrap();
    (cache, data, runtime, download_id, release)
}

/// 返回 staging 是否持有 opaque durable proof。
fn has_staged(runtime: &ApplicationUpdateRuntime) -> bool {
    runtime
        .staging
        .lock()
        .unwrap()
        .as_ref()
        .and_then(StagingRepository::snapshot)
        .is_some()
}

#[test]
fn updater_registry_identity_is_namespaced_and_bounded() {
    let _ = managed_operation_id("download", 42);
}

/// 真实 Ready→检查成功→自动下载链路必须为下载编号绑定候选，不能沿用检查编号。
#[test]
fn automatic_download_binds_the_approved_download_identity() {
    let (_cache, _data, runtime, download_id, release) = downloading_runtime();
    assert_eq!(download_id.value(), 2, "检查与下载必须有独立任务编号");
    assert_eq!(
        runtime.pending_release(download_id, &release),
        Some(release)
    );
}

/// 自动重试使用新下载编号，旧下载与不同候选不能借用新任务的绑定。
#[test]
fn download_retry_rebinds_candidate_and_rejects_previous_attempt() {
    let (_cache, _data, runtime, previous_id, release) = downloading_runtime();
    let failed = runtime
        .transition(UpdateInput::DownloadFailed {
            operation_id: previous_id,
            failure: UpdateFailure::new(UpdateFailureCategory::Network, "fixture"),
        })
        .unwrap();
    let token = failed
        .effects
        .into_iter()
        .find_map(|effect| match effect {
            UpdateEffect::ScheduleRetry { token, .. } => Some(token),
            _ => None,
        })
        .unwrap();
    let retry = runtime.transition(UpdateInput::RetryDue { token }).unwrap();
    let retry_id = retry
        .effects
        .into_iter()
        .find_map(|effect| match effect {
            UpdateEffect::StartDownload { operation_id, .. } => Some(operation_id),
            _ => None,
        })
        .unwrap();
    assert_ne!(previous_id, retry_id);
    assert!(runtime.pending_release(previous_id, &release).is_none());
    assert_eq!(
        runtime.pending_release(retry_id, &release),
        Some(release.clone())
    );
    let other = StableRelease::new("3.0.0", "other", release.package().clone());
    assert!(runtime.pending_release(retry_id, &other).is_none());
}

#[test]
fn runtime_converts_leaked_prerelease_domain_error_into_ordered_manifest_failure() {
    let cache = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let (runtime, startup) = ApplicationUpdateRuntime::new_at_paths(
        data.path().to_path_buf(),
        cache.path().to_path_buf(),
        "1.0.0",
        trusted_config(),
    )
    .unwrap();
    assert!(startup.is_none());
    let ready = runtime.transition(UpdateInput::Ready).unwrap();
    let operation_id = ready
        .effects
        .into_iter()
        .find_map(|effect| match effect {
            UpdateEffect::CheckStable { operation_id } => Some(operation_id),
            _ => None,
        })
        .unwrap();
    let (target, install_kind) = current_install_plan().unwrap();
    let package = super::super::ReleasePackage::new(
        "https://github.com/XLCYun/narrant/releases/download/v2.0.0/Narrant.tar.gz",
        "signature",
        target,
        install_kind,
    )
    .unwrap();
    let failed = runtime
        .transition(UpdateInput::CheckSucceeded {
            operation_id,
            release: Some(StableRelease::new("2.0.0-beta.1", "notes", package)),
        })
        .unwrap();
    assert_eq!(failed.snapshot.state, super::super::UpdateState::Failed);
    assert_eq!(
        failed.snapshot.failure.unwrap().category,
        UpdateFailureCategory::Manifest
    );
    assert!(failed.snapshot.sequence > ready.snapshot.sequence);
    assert!(runtime
        .transition(UpdateInput::ManualCheck)
        .unwrap()
        .effects
        .iter()
        .any(|effect| matches!(effect, UpdateEffect::CheckStable { .. })));
    let diagnostics = std::fs::read_to_string(runtime.diagnostics.path()).unwrap();
    assert!(diagnostics.contains("manifest"));
    assert!(!diagnostics.contains("beta"));
}

#[test]
fn runtime_preference_save_failure_keeps_disk_truth_and_emits_redacted_warning() {
    let cache = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let preference_path = data.path().join("application-update.json");
    let mut initial = FilePreferenceStore::new(&preference_path);
    assert_eq!(
        initial.save_automatic_download(true),
        PreferenceSaveOutcome::Committed {
            applied_value: true,
        }
    );
    let preferences = FilePreferenceStore::with_faults(
        &preference_path,
        PreferenceFailAt(PreferenceCommitPoint::BeforeReplace),
    );
    let (runtime, startup) = ApplicationUpdateRuntime::new_at_paths_with_preferences(
        data.path().to_path_buf(),
        cache.path().to_path_buf(),
        "1.0.0",
        trusted_config(),
        preferences,
    )
    .unwrap();
    assert!(startup.is_none());

    let failed = runtime
        .transition(UpdateInput::SetAutomaticDownload(false))
        .unwrap();

    assert_eq!(failed.snapshot.sequence, 1);
    assert_eq!(failed.snapshot.state, super::super::UpdateState::Idle);
    assert!(failed.snapshot.automatic_download);
    assert!(failed.snapshot.failure.is_none());
    assert_eq!(
        failed.snapshot.preference_warning.unwrap().kind,
        UpdatePreferenceWarningKind::Rejected
    );
    assert_eq!(
        FilePreferenceStore::new(&preference_path)
            .load_automatic_download()
            .unwrap(),
        Some(true)
    );
    let serialized = serde_json::to_string(&runtime.snapshot()).unwrap();
    assert!(serialized.contains("preferenceWarning"));
    assert!(serialized.contains("storage"));
    assert!(!serialized.contains(data.path().to_string_lossy().as_ref()));
    assert!(!serialized.contains("application-update.json"));
    let diagnostics = std::fs::read_to_string(runtime.diagnostics.path()).unwrap();
    assert!(diagnostics.contains("storage"));
    assert!(!diagnostics.contains(data.path().to_string_lossy().as_ref()));
    assert!(!diagnostics.contains("application-update.json"));
}

#[test]
fn runtime_post_replace_sync_failure_applies_formal_value_and_warns_about_durability() {
    let cache = tempfile::tempdir().unwrap();
    let data = tempfile::tempdir().unwrap();
    let preference_path = data.path().join("application-update.json");
    let mut initial = FilePreferenceStore::new(&preference_path);
    assert!(matches!(
        initial.save_automatic_download(true),
        PreferenceSaveOutcome::Committed { .. }
    ));
    let preferences = FilePreferenceStore::with_faults(
        &preference_path,
        PreferenceFailAt(PreferenceCommitPoint::AfterReplaceBeforeDirectorySync),
    );
    let (runtime, startup) = ApplicationUpdateRuntime::new_at_paths_with_preferences(
        data.path().to_path_buf(),
        cache.path().to_path_buf(),
        "1.0.0",
        trusted_config(),
        preferences,
    )
    .unwrap();
    assert!(startup.is_none());

    let uncertain = runtime
        .transition(UpdateInput::SetAutomaticDownload(false))
        .unwrap();

    assert_eq!(uncertain.snapshot.sequence, 1);
    assert_eq!(uncertain.snapshot.state, super::super::UpdateState::Idle);
    assert!(!uncertain.snapshot.automatic_download);
    assert!(uncertain.snapshot.failure.is_none());
    assert_eq!(
        uncertain.snapshot.preference_warning.unwrap().kind,
        UpdatePreferenceWarningKind::DurabilityUnknown
    );
    assert_eq!(
        FilePreferenceStore::new(&preference_path)
            .load_automatic_download()
            .unwrap(),
        Some(false)
    );
    let diagnostics = std::fs::read_to_string(runtime.diagnostics.path()).unwrap();
    assert!(diagnostics.contains("transition-warning"));
    assert!(diagnostics.contains("storage"));
    assert!(!diagnostics.contains(data.path().to_string_lossy().as_ref()));
    drop(runtime);

    let (restarted, _) = ApplicationUpdateRuntime::new_at_paths(
        data.path().to_path_buf(),
        cache.path().to_path_buf(),
        "1.0.0",
        trusted_config(),
    )
    .unwrap();
    assert!(!restarted.snapshot().automatic_download);
    assert!(restarted.snapshot().preference_warning.is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn managed_download_cancel_before_eof_leaves_disk_and_core_unstaged_and_releases_raii() {
    let (_cache, _data, runtime, operation_id, release) = downloading_runtime();
    let operations = CancellableOperations::default();
    let registry_id = managed_operation_id("download-test", operation_id.value());
    let (body_tx, body_rx) = tokio::sync::mpsc::channel::<Result<Bytes, ()>>(2);
    let stream = futures_util::stream::unfold(body_rx, |mut receiver| async move {
        receiver.recv().await.map(|item| (item, receiver))
    });
    let (progress_tx, progress_rx) = std::sync::mpsc::sync_channel(1);
    let task_operations = operations.clone();
    let task_id = registry_id.clone();
    let task_runtime = runtime.clone();
    let task = tokio::spawn(async move {
        task_operations
            .run(task_id, |context| async move {
                task_runtime
                    .consume_stream_and_commit(
                        operation_id,
                        release,
                        None,
                        stream,
                        context.cancellation(),
                        |_| {
                            let _ = progress_tx.send(());
                        },
                        || {},
                    )
                    .await
            })
            .await
    });
    body_tx.send(Ok(Bytes::from_static(b"te"))).await.unwrap();
    progress_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    runtime
        .transition(UpdateInput::SetAutomaticDownload(false))
        .unwrap();
    operations.cancel(&registry_id).unwrap();
    drop(body_tx);
    assert!(matches!(
        task.await.unwrap().unwrap(),
        DownloadTaskOutcome::Cancelled
    ));
    assert!(!has_staged(&runtime));
    assert_eq!(
        runtime.snapshot().state,
        super::super::UpdateState::Available
    );
    assert!(operations.run(registry_id, |_| async {}).await.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn managed_download_cancel_after_eof_while_waiting_transition_gate_does_not_commit() {
    let (_cache, _data, runtime, operation_id, release) = downloading_runtime();
    let operations = CancellableOperations::default();
    let registry_id = managed_operation_id("download-test", operation_id.value());
    let gate = runtime.transition_gate.lock().unwrap();
    let (reached_tx, reached_rx) = std::sync::mpsc::sync_channel(1);
    let task_operations = operations.clone();
    let task_id = registry_id.clone();
    let task_runtime = runtime.clone();
    let task = tokio::spawn(async move {
        task_operations
            .run(task_id, |context| async move {
                task_runtime
                    .consume_stream_and_commit(
                        operation_id,
                        release,
                        Some(4),
                        futures_util::stream::iter([Ok::<Bytes, ()>(Bytes::from_static(b"test"))]),
                        context.cancellation(),
                        |_| {},
                        || reached_tx.send(()).unwrap(),
                    )
                    .await
            })
            .await
    });
    reached_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    operations.cancel(&registry_id).unwrap();
    drop(gate);
    assert!(matches!(
        task.await.unwrap().unwrap(),
        DownloadTaskOutcome::Cancelled
    ));
    runtime
        .transition(UpdateInput::SetAutomaticDownload(false))
        .unwrap();
    assert!(!has_staged(&runtime));
    assert_eq!(
        runtime.snapshot().state,
        super::super::UpdateState::Available
    );
    assert!(operations.run(registry_id, |_| async {}).await.is_ok());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn managed_download_stage_winner_commits_disk_and_core_then_releases_raii() {
    let (_cache, _data, runtime, operation_id, release) = downloading_runtime();
    let operations = CancellableOperations::default();
    let registry_id = managed_operation_id("download-test", operation_id.value());
    let task_runtime = runtime.clone();
    let outcome = operations
        .run(registry_id.clone(), |context| async move {
            task_runtime
                .consume_stream_and_commit(
                    operation_id,
                    release,
                    Some(4),
                    futures_util::stream::iter([Ok::<Bytes, ()>(Bytes::from_static(b"test"))]),
                    context.cancellation(),
                    |_| {},
                    || {},
                )
                .await
        })
        .await
        .unwrap();
    assert!(matches!(outcome, DownloadTaskOutcome::Committed(_)));
    assert!(has_staged(&runtime));
    assert_eq!(runtime.snapshot().state, super::super::UpdateState::Staged);
    assert!(operations.run(registry_id, |_| async {}).await.is_ok());
}

#[test]
fn post_stop_staging_try_lock_busy_returns_without_waiting() {
    let (_cache, _data, runtime, _operation_id, _release) = downloading_runtime();
    let _guard = runtime.staging.lock().unwrap();
    let started = std::time::Instant::now();
    let result = runtime.try_use_staged(|_| Ok::<_, String>(()));
    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_millis(50));
}

#[test]
fn runtime_ready_keeps_recovery_warning_queryable_and_repository_can_continue() {
    let cache = tempfile::tempdir().expect("cache");
    let data = tempfile::tempdir().expect("data");
    let (target, install_kind) = current_install_plan().unwrap();
    let context = StagingContext::new("1.0.0", target, install_kind).unwrap();
    let verifier = DiskMinisignVerifier::new(&encode(PUBLIC_KEY)).unwrap();
    let mut repository = StagingRepository::open(cache.path(), context, verifier).unwrap();
    let candidate =
        StageCandidate::new("2.0.0", encode(SIGNATURE), target, install_kind, "notes").unwrap();
    repository.stage(candidate, Cursor::new(b"test")).unwrap();
    drop(repository);
    let staging_root = cache.path().join("application-update");
    let generation = std::fs::read_to_string(staging_root.join("current")).unwrap();
    std::fs::write(
        staging_root.join(generation.trim()).join("package.bin"),
        b"tampered",
    )
    .unwrap();

    let (runtime, startup) = ApplicationUpdateRuntime::new_at_paths(
        data.path().to_path_buf(),
        cache.path().to_path_buf(),
        "1.0.0",
        trusted_config(),
    )
    .unwrap();
    assert_eq!(
        startup.unwrap().snapshot.recovery_warning.unwrap().category,
        UpdateFailureCategory::Verification
    );
    let ready = runtime.transition(UpdateInput::Ready).unwrap();
    assert_eq!(ready.snapshot.state, super::super::UpdateState::Checking);
    assert_eq!(
        runtime.snapshot().recovery_warning.unwrap().category,
        UpdateFailureCategory::Verification
    );
    let serialized = serde_json::to_value(runtime.snapshot()).unwrap();
    assert_eq!(serialized["recoveryWarning"]["category"], "verification");
    assert!(ready
        .effects
        .iter()
        .any(|effect| matches!(effect, UpdateEffect::CheckStable { .. })));
    assert!(runtime.staging.lock().unwrap().as_ref().is_some());
    let diagnostics = std::fs::read_to_string(runtime.diagnostics.path()).unwrap();
    assert!(diagnostics.contains("verification"));
    assert!(!diagnostics.contains(cache.path().to_string_lossy().as_ref()));
    assert!(!diagnostics.contains("signature"));
}

#[test]
fn runtime_ready_keeps_unavailable_repository_warning_queryable() {
    let root = tempfile::tempdir().expect("root");
    let data = tempfile::tempdir().expect("data");
    let unavailable_root = root.path().join("not-a-directory");
    std::fs::write(&unavailable_root, b"file").unwrap();
    let (runtime, startup) = ApplicationUpdateRuntime::new_at_paths(
        data.path().to_path_buf(),
        unavailable_root.clone(),
        "1.0.0",
        trusted_config(),
    )
    .unwrap();
    assert_eq!(
        startup.unwrap().snapshot.recovery_warning.unwrap().category,
        UpdateFailureCategory::Storage
    );
    let ready = runtime.transition(UpdateInput::Ready).unwrap();
    assert_eq!(ready.snapshot.state, super::super::UpdateState::Checking);
    assert_eq!(
        runtime.snapshot().recovery_warning.unwrap().category,
        UpdateFailureCategory::Storage
    );
    let serialized = serde_json::to_value(runtime.snapshot()).unwrap();
    assert_eq!(serialized["recoveryWarning"]["category"], "storage");
    assert!(runtime.staging.lock().unwrap().is_none());
    let diagnostics = std::fs::read_to_string(runtime.diagnostics.path()).unwrap();
    assert!(diagnostics.contains("storage"));
    assert!(!diagnostics.contains(unavailable_root.to_string_lossy().as_ref()));
}
