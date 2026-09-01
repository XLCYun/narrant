use crate::application_update::{
    Clock, DownloadProgress, DurableStagedUpdate, InstallIntent, InstallKind, PreferenceError,
    PreferenceStore, ReleasePackage, StableRelease, UpdateController, UpdateDomainError,
    UpdateEffect, UpdateFailure, UpdateFailureCategory, UpdateInput, UpdateOperationId,
    UpdateState,
};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// 测试用的可共享单调时钟。
#[derive(Clone)]
struct TestClock(Rc<Cell<Duration>>);

impl Clock for TestClock {
    /// 返回测试控制的单调时间。
    fn now(&self) -> Duration {
        self.0.get()
    }
}

/// 测试用的可跨 Controller 重建共享偏好存储。
#[derive(Clone, Default)]
struct MemoryPreferences(Rc<Cell<Option<bool>>>);

impl PreferenceStore for MemoryPreferences {
    /// 读取测试内存中的自动下载偏好。
    fn load_automatic_download(&self) -> Result<Option<bool>, PreferenceError> {
        Ok(self.0.get())
    }

    /// 保存测试内存中的自动下载偏好。
    fn save_automatic_download(&mut self, enabled: bool) -> Result<(), PreferenceError> {
        self.0.set(Some(enabled));
        Ok(())
    }
}

/// 创建可控制时钟的测试领域控制器。
fn controller(preference: Option<bool>) -> UpdateController<TestClock, MemoryPreferences> {
    UpdateController::new(
        "2.0.15",
        TestClock(Rc::new(Cell::new(Duration::from_secs(10)))),
        MemoryPreferences(Rc::new(Cell::new(preference))),
    )
    .unwrap()
}

/// 创建用于检查结果的可信 Stable 发布。
fn release(version: &str) -> StableRelease {
    release_with_notes(version, "修复启动问题")
}

/// 创建带指定公开说明的可信 Stable 发布。
fn release_with_notes(version: &str, notes: &str) -> StableRelease {
    StableRelease::new(
        version,
        notes,
        ReleasePackage::new(
            format!("https://example.test/{version}"),
            format!("signature-{version}"),
            "darwin-aarch64",
            InstallKind::MacosAppArchive,
        )
        .unwrap(),
    )
}

/// 创建只在测试构建可用、与 Stable target 一致的 durable staging proof。
fn durable(version: &str, notes: &str) -> DurableStagedUpdate {
    DurableStagedUpdate::test_fixture(
        version,
        notes,
        "darwin-aarch64",
        InstallKind::MacosAppArchive,
    )
}

/// 从副作用中提取本次检查 identity。
fn check_id(effects: &[UpdateEffect]) -> UpdateOperationId {
    effects
        .iter()
        .find_map(|effect| match effect {
            UpdateEffect::CheckStable { operation_id } => Some(*operation_id),
            _ => None,
        })
        .expect("expected CheckStable effect")
}

/// 从副作用中提取本次下载 identity。
fn download_id(effects: &[UpdateEffect]) -> UpdateOperationId {
    effects
        .iter()
        .find_map(|effect| match effect {
            UpdateEffect::StartDownload { operation_id, .. } => Some(*operation_id),
            _ => None,
        })
        .expect("expected StartDownload effect")
}

/// 从副作用中提取本次 retry timer generation。
fn retry_token(effects: &[UpdateEffect]) -> (u64, Duration) {
    effects
        .iter()
        .find_map(|effect| match effect {
            UpdateEffect::ScheduleRetry { token, after, .. } => Some((*token, *after)),
            _ => None,
        })
        .expect("expected ScheduleRetry effect")
}

/// Ready 后立即检查并安排六小时后的下一次检查。
#[test]
fn ready_starts_identity_scoped_check_and_six_hour_schedule() {
    let mut controller = controller(None);
    let output = controller.handle(UpdateInput::Ready).unwrap();
    assert_eq!(output.snapshot.sequence, 1);
    assert_eq!(output.snapshot.current_version, "2.0.15");
    assert_eq!(output.snapshot.state, UpdateState::Checking);
    assert_eq!(
        output.snapshot.next_check_at,
        Some(Duration::from_secs(21_610))
    );
    assert!(check_id(&output.effects).value() > 0);
    assert!(output.effects.contains(&UpdateEffect::ScheduleCheck {
        after: Duration::from_secs(21_600),
    }));
}

/// 手动、启动和周期检查在忙碌时合并，但不会丢失下一周期。
#[test]
fn startup_periodic_and_manual_checks_coalesce() {
    let mut controller = controller(None);
    let ready = controller.handle(UpdateInput::Ready).unwrap();
    let id = check_id(&ready.effects);
    assert!(controller
        .handle(UpdateInput::ManualCheck)
        .unwrap()
        .effects
        .is_empty());
    assert_eq!(
        controller
            .handle(UpdateInput::PeriodicCheckDue)
            .unwrap()
            .effects,
        vec![UpdateEffect::ScheduleCheck {
            after: Duration::from_secs(21_600),
        }]
    );
    controller
        .handle(UpdateInput::CheckSucceeded {
            operation_id: id,
            release: None,
        })
        .unwrap();
    assert!(
        check_id(&controller.handle(UpdateInput::ManualCheck).unwrap().effects).value()
            > id.value()
    );
}

/// Ready 前周期信号不启动后台任务，Ready 可与已有手动检查合并。
#[test]
fn readiness_controls_periodic_schedule_without_duplicate_check() {
    let mut controller = controller(None);
    assert!(controller
        .handle(UpdateInput::PeriodicCheckDue)
        .unwrap()
        .effects
        .is_empty());
    let manual = controller.handle(UpdateInput::ManualCheck).unwrap();
    let manual_id = check_id(&manual.effects);
    let ready = controller.handle(UpdateInput::Ready).unwrap();
    assert_eq!(ready.snapshot.state, UpdateState::Checking);
    assert_eq!(
        ready.effects,
        vec![UpdateEffect::ScheduleCheck {
            after: Duration::from_secs(21_600),
        }]
    );
    controller
        .handle(UpdateInput::CheckSucceeded {
            operation_id: manual_id,
            release: None,
        })
        .unwrap();
}

/// 当前版、旧版和预发布不会进入下载，包契约拒绝不安全来源。
#[test]
fn semver_and_package_policy_fail_closed() {
    for version in ["2.0.14", "2.0.15"] {
        let mut controller = controller(None);
        let check = controller.handle(UpdateInput::ManualCheck).unwrap();
        let output = controller
            .handle(UpdateInput::CheckSucceeded {
                operation_id: check_id(&check.effects),
                release: Some(release(version)),
            })
            .unwrap();
        assert_eq!(output.snapshot.state, UpdateState::UpToDate);
    }
    let mut controller = controller(None);
    let check = controller.handle(UpdateInput::ManualCheck).unwrap();
    assert!(matches!(
        controller.handle(UpdateInput::CheckSucceeded {
            operation_id: check_id(&check.effects),
            release: Some(release("2.1.0-beta.1")),
        }),
        Err(UpdateDomainError::InvalidReleaseVersion(_))
    ));
    for url in ["http://example.test/package", "https://"] {
        assert!(ReleasePackage::new(
            url,
            "signature",
            "darwin-aarch64",
            InstallKind::MacosAppArchive,
        )
        .is_err());
    }
}

/// 自动下载默认开启，关闭时保存偏好，重新开启后继续当前候选。
#[test]
fn automatic_download_preference_persists_and_resumes_candidate() {
    let preferences = MemoryPreferences::default();
    let mut controller = UpdateController::new(
        "2.0.15",
        TestClock(Rc::new(Cell::new(Duration::ZERO))),
        preferences.clone(),
    )
    .unwrap();
    controller
        .handle(UpdateInput::SetAutomaticDownload(false))
        .unwrap();
    let check = controller.handle(UpdateInput::ManualCheck).unwrap();
    let available = controller
        .handle(UpdateInput::CheckSucceeded {
            operation_id: check_id(&check.effects),
            release: Some(release("2.1.0")),
        })
        .unwrap();
    assert_eq!(available.snapshot.state, UpdateState::Available);
    assert!(available.effects.is_empty());
    assert!(
        download_id(
            &controller
                .handle(UpdateInput::SetAutomaticDownload(true))
                .unwrap()
                .effects
        )
        .value()
            > 0
    );
    assert!(
        UpdateController::new(
            "2.0.15",
            TestClock(Rc::new(Cell::new(Duration::ZERO))),
            preferences,
        )
        .unwrap()
        .snapshot()
        .automatic_download
    );
}

/// 更高候选下载、验签或存储失败时，旧可信 staging 仍可安装。
#[test]
fn higher_candidate_failure_keeps_old_staging_installable() {
    for category in [
        UpdateFailureCategory::Network,
        UpdateFailureCategory::Verification,
        UpdateFailureCategory::Storage,
    ] {
        let mut controller = controller(None);
        controller
            .handle(UpdateInput::RecoverStaged {
                staged: durable("2.1.0", "旧版说明"),
            })
            .unwrap();
        let check = controller.handle(UpdateInput::ManualCheck).unwrap();
        let discovered = controller
            .handle(UpdateInput::CheckSucceeded {
                operation_id: check_id(&check.effects),
                release: Some(release_with_notes("2.2.0", "新版说明")),
            })
            .unwrap();
        let operation_id = download_id(&discovered.effects);
        assert!(!discovered
            .effects
            .iter()
            .any(|effect| matches!(effect, UpdateEffect::RequestInstall { .. })));
        let failed = controller
            .handle(UpdateInput::DownloadFailed {
                operation_id,
                failure: UpdateFailure::new(category, "private diagnostic"),
            })
            .unwrap();
        assert_eq!(failed.snapshot.state, UpdateState::Staged);
        assert_eq!(failed.snapshot.candidate.unwrap().version, "2.1.0");
        assert_eq!(
            controller
                .handle(UpdateInput::RequestUpdateRestart)
                .unwrap()
                .effects,
            vec![UpdateEffect::RequestInstall {
                intent: InstallIntent::UpdateRestart,
                version: "2.1.0".into(),
            }]
        );
    }
}

/// 下载进度只接受当前 identity，成功后快照携带候选与说明。
#[test]
fn progress_and_completion_are_identity_scoped() {
    let mut controller = controller(None);
    let check = controller.handle(UpdateInput::ManualCheck).unwrap();
    let discovered = controller
        .handle(UpdateInput::CheckSucceeded {
            operation_id: check_id(&check.effects),
            release: Some(release_with_notes("2.1.0", "说明")),
        })
        .unwrap();
    let id = download_id(&discovered.effects);
    controller
        .handle(UpdateInput::DownloadStarted { operation_id: id })
        .unwrap();
    let advanced = controller
        .handle(UpdateInput::DownloadAdvanced {
            operation_id: id,
            progress: DownloadProgress::KnownTotal {
                downloaded_bytes: 5,
                total_bytes: 10,
            },
        })
        .unwrap();
    assert_eq!(
        advanced.snapshot.state,
        UpdateState::Downloading {
            progress: DownloadProgress::KnownTotal {
                downloaded_bytes: 5,
                total_bytes: 10,
            }
        }
    );
    for invalid_proof in [
        durable("2.2.0", "说明"),
        durable("2.1.0", "不同说明"),
        DurableStagedUpdate::test_fixture(
            "2.1.0",
            "说明",
            "windows-x86_64",
            InstallKind::MacosAppArchive,
        ),
        DurableStagedUpdate::test_fixture(
            "2.1.0",
            "说明",
            "darwin-aarch64",
            InstallKind::WindowsNsis,
        ),
    ] {
        let rejected = controller
            .handle(UpdateInput::DownloadSucceeded {
                operation_id: id,
                staged: invalid_proof,
            })
            .unwrap();
        assert_eq!(rejected.snapshot.sequence, advanced.snapshot.sequence);
        assert_eq!(rejected.snapshot.state, advanced.snapshot.state);
    }
    let staged = controller
        .handle(UpdateInput::DownloadSucceeded {
            operation_id: id,
            staged: durable("2.1.0", "说明"),
        })
        .unwrap();
    assert_eq!(staged.snapshot.state, UpdateState::Staged);
    assert_eq!(staged.snapshot.candidate.unwrap().release_notes, "说明");
    let sequence = staged.snapshot.sequence;
    assert_eq!(
        controller
            .handle(UpdateInput::DownloadAdvanced {
                operation_id: id,
                progress: DownloadProgress::UnknownTotal {
                    downloaded_bytes: 99
                },
            })
            .unwrap()
            .snapshot
            .sequence,
        sequence
    );
    assert_eq!(
        controller
            .handle(UpdateInput::DownloadSucceeded {
                operation_id: id,
                staged: durable("2.1.0", "说明"),
            })
            .unwrap()
            .snapshot
            .sequence,
        sequence
    );
}

/// failed、staged 与恢复快照都保留可展示的候选版本和有界说明。
#[test]
fn candidate_and_notes_survive_failure_and_recovery() {
    let mut recovered_controller = controller(None);
    recovered_controller
        .handle(UpdateInput::RecoverStaged {
            staged: durable("2.1.0", "离线说明"),
        })
        .unwrap();
    let recovered = recovered_controller.snapshot();
    assert_eq!(recovered.state, UpdateState::Staged);
    assert_eq!(
        recovered.candidate.as_ref().unwrap().release_notes,
        "离线说明"
    );

    let check = recovered_controller
        .handle(UpdateInput::ManualCheck)
        .unwrap();
    let failed = recovered_controller
        .handle(UpdateInput::CheckFailed {
            operation_id: check_id(&check.effects),
            failure: UpdateFailure::new(UpdateFailureCategory::Manifest, "bad manifest"),
        })
        .unwrap();
    assert_eq!(failed.snapshot.state, UpdateState::Staged);
    assert_eq!(failed.snapshot.candidate.unwrap().version, "2.1.0");

    let mut fresh = controller(None);
    let check = fresh.handle(UpdateInput::ManualCheck).unwrap();
    let discovered = fresh
        .handle(UpdateInput::CheckSucceeded {
            operation_id: check_id(&check.effects),
            release: Some(release_with_notes("2.2.0", "网络说明")),
        })
        .unwrap();
    let failed = fresh
        .handle(UpdateInput::DownloadFailed {
            operation_id: download_id(&discovered.effects),
            failure: UpdateFailure::new(UpdateFailureCategory::Storage, "disk full"),
        })
        .unwrap();
    assert_eq!(failed.snapshot.state, UpdateState::Failed);
    assert_eq!(failed.snapshot.candidate.unwrap().release_notes, "网络说明");
}

/// 同版本 Stable 检查可以回填恢复时缺失的 Release Notes。
#[test]
fn same_staged_version_backfills_release_notes() {
    let mut controller = controller(None);
    controller
        .handle(UpdateInput::RecoverStaged {
            staged: durable("2.1.0", ""),
        })
        .unwrap();
    let check = controller.handle(UpdateInput::ManualCheck).unwrap();
    let output = controller
        .handle(UpdateInput::CheckSucceeded {
            operation_id: check_id(&check.effects),
            release: Some(release_with_notes("2.1.0", "补充说明")),
        })
        .unwrap();
    assert_eq!(output.snapshot.state, UpdateState::Staged);
    assert_eq!(output.snapshot.candidate.unwrap().release_notes, "补充说明");
    assert_eq!(
        output.effects,
        vec![UpdateEffect::PersistStagedReleaseNotes {
            version: "2.1.0".into(),
            release_notes: "补充说明".into(),
        }]
    );
}

/// 公开失败只包含核心固定文本，URL、签名、credential 与路径保持私有。
#[test]
fn serialized_failure_snapshot_redacts_adapter_diagnostics() {
    let diagnostic = "https://updates.test/pkg?token=secret signature=ABC /Users/alice/pkg C:\\Users\\alice\\pkg";
    let failure = UpdateFailure::new(UpdateFailureCategory::Verification, diagnostic);
    assert_eq!(failure.diagnostic(), diagnostic);
    let debug = format!("{failure:?}");
    assert!(!debug.contains("updates.test"));
    assert!(!debug.contains("ABC"));

    let mut controller = controller(None);
    let check = controller.handle(UpdateInput::ManualCheck).unwrap();
    let output = controller
        .handle(UpdateInput::CheckFailed {
            operation_id: check_id(&check.effects),
            failure,
        })
        .unwrap();
    let serialized = serde_json::to_string(&output.snapshot).unwrap();
    for secret in [
        "updates.test",
        "token",
        "secret",
        "ABC",
        "/Users",
        "C:\\\\Users",
    ] {
        assert!(
            !serialized.contains(secret),
            "leaked {secret}: {serialized}"
        );
    }
    assert!(serialized.contains("verification"));
    assert!(serialized.contains("应用更新包验证失败"));
}

/// HTTP 状态被核心收敛为稳定类别和规范要求的重试策略。
#[test]
fn http_failures_map_to_stable_retry_categories() {
    for (status, category, retryable) in [
        (408, UpdateFailureCategory::RequestTimeout, true),
        (429, UpdateFailureCategory::RateLimited, true),
        (503, UpdateFailureCategory::Server, true),
        (404, UpdateFailureCategory::Manifest, false),
    ] {
        let failure = UpdateFailure::from_http_status(status, "private response");
        assert_eq!(failure.category(), category);
        assert_eq!(failure.retryable(), retryable);
    }
}

/// 关闭自动下载使已排定 timer generation 失效，但显式 retry 仍可下载。
#[test]
fn disabling_auto_invalidates_download_retry_but_explicit_retry_works() {
    let mut controller = controller(None);
    let check = controller.handle(UpdateInput::ManualCheck).unwrap();
    let discovered = controller
        .handle(UpdateInput::CheckSucceeded {
            operation_id: check_id(&check.effects),
            release: Some(release("2.1.0")),
        })
        .unwrap();
    let failed = controller
        .handle(UpdateInput::DownloadFailed {
            operation_id: download_id(&discovered.effects),
            failure: UpdateFailure::new(UpdateFailureCategory::Network, "offline"),
        })
        .unwrap();
    let (token, _) = retry_token(&failed.effects);
    controller
        .handle(UpdateInput::SetAutomaticDownload(false))
        .unwrap();
    let sequence = controller.snapshot().sequence;
    let stale = controller.handle(UpdateInput::RetryDue { token }).unwrap();
    assert_eq!(stale.snapshot.sequence, sequence);
    assert!(stale.effects.is_empty());
    assert_eq!(stale.snapshot.state, UpdateState::Available);
    let explicit = controller.handle(UpdateInput::UserRetry).unwrap();
    assert!(download_id(&explicit.effects).value() > 0);
}

/// 关闭自动下载会请求取消当前自动任务，取消后的回调全部失效。
#[test]
fn disabling_auto_cancels_inflight_automatic_download() {
    let mut controller = controller(None);
    let check = controller.handle(UpdateInput::ManualCheck).unwrap();
    let discovered = controller
        .handle(UpdateInput::CheckSucceeded {
            operation_id: check_id(&check.effects),
            release: Some(release("2.1.0")),
        })
        .unwrap();
    let id = download_id(&discovered.effects);
    controller
        .handle(UpdateInput::DownloadStarted { operation_id: id })
        .unwrap();
    let cancelled = controller
        .handle(UpdateInput::SetAutomaticDownload(false))
        .unwrap();
    assert_eq!(
        cancelled.effects,
        vec![UpdateEffect::CancelDownload { operation_id: id }]
    );
    assert_eq!(cancelled.snapshot.state, UpdateState::Available);
    let sequence = cancelled.snapshot.sequence;
    assert_eq!(
        controller
            .handle(UpdateInput::DownloadSucceeded {
                operation_id: id,
                staged: durable("2.1.0", "修复启动问题"),
            })
            .unwrap()
            .snapshot
            .sequence,
        sequence
    );
}

/// 旧 check identity 不能覆盖新操作，重复和迟到 completion 也不发布快照。
#[test]
fn stale_check_result_cannot_replace_new_operation() {
    let mut controller = controller(None);
    let first = controller.handle(UpdateInput::ManualCheck).unwrap();
    let first_id = check_id(&first.effects);
    controller
        .handle(UpdateInput::CheckFailed {
            operation_id: first_id,
            failure: UpdateFailure::new(UpdateFailureCategory::Network, "offline"),
        })
        .unwrap();
    let retry = controller.handle(UpdateInput::UserRetry).unwrap();
    let second_id = check_id(&retry.effects);
    assert_ne!(first_id, second_id);
    let sequence = retry.snapshot.sequence;
    let stale = controller
        .handle(UpdateInput::CheckSucceeded {
            operation_id: first_id,
            release: Some(release("9.0.0")),
        })
        .unwrap();
    assert_eq!(stale.snapshot.sequence, sequence);
    assert_eq!(stale.snapshot.state, UpdateState::Checking);
}

/// 自动失败使用真实 1/2/4 秒三次退避，每次尝试都有新 identity。
#[test]
fn automatic_retry_uses_real_one_two_four_sequence() {
    let mut controller = controller(None);
    let started = controller.handle(UpdateInput::ManualCheck).unwrap();
    let mut operation_id = check_id(&started.effects);
    let mut identities = vec![operation_id.value()];
    for expected in [1, 2, 4] {
        let failed = controller
            .handle(UpdateInput::CheckFailed {
                operation_id,
                failure: UpdateFailure::new(UpdateFailureCategory::Network, "offline"),
            })
            .unwrap();
        let (token, after) = retry_token(&failed.effects);
        assert_eq!(after, Duration::from_secs(expected));
        let retried = controller.handle(UpdateInput::RetryDue { token }).unwrap();
        operation_id = check_id(&retried.effects);
        identities.push(operation_id.value());
    }
    let exhausted = controller
        .handle(UpdateInput::CheckFailed {
            operation_id,
            failure: UpdateFailure::new(UpdateFailureCategory::Network, "offline"),
        })
        .unwrap();
    assert!(!exhausted
        .effects
        .iter()
        .any(|effect| matches!(effect, UpdateEffect::ScheduleRetry { .. })));
    identities.sort_unstable();
    identities.dedup();
    assert_eq!(identities.len(), 4);
    assert!(
        check_id(&controller.handle(UpdateInput::UserRetry).unwrap().effects).value()
            > operation_id.value()
    );
}

/// 只有 staged 状态可生成 Update Restart 安装意图。
#[test]
fn update_restart_requires_staged_state() {
    let mut controller = controller(None);
    assert!(controller
        .handle(UpdateInput::RequestUpdateRestart)
        .unwrap()
        .effects
        .is_empty());
    controller
        .handle(UpdateInput::RecoverStaged {
            staged: durable("2.1.0", ""),
        })
        .unwrap();
    assert_eq!(
        controller
            .handle(UpdateInput::RequestUpdateRestart)
            .unwrap()
            .effects,
        vec![UpdateEffect::RequestInstall {
            intent: InstallIntent::UpdateRestart,
            version: "2.1.0".into(),
        }]
    );
}
