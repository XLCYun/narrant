use crate::application_update::{
    Clock, DownloadProgress, InstallIntent, InstallKind, PreferenceError, PreferenceStore,
    ReleasePackage, StableRelease, UpdateController, UpdateEffect, UpdateFailure,
    UpdateFailureCategory, UpdateInput, UpdateOperation, UpdateState,
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
    StableRelease::new(
        version,
        "修复启动问题",
        ReleasePackage::new(
            format!("https://example.test/{version}"),
            format!("signature-{version}"),
            "darwin-aarch64",
            InstallKind::MacosAppArchive,
        )
        .unwrap(),
    )
}

/// Ready 后立即检查并安排六小时后的下一次检查。
#[test]
fn ready_starts_check_and_schedules_six_hour_interval() {
    let output = controller(None).handle(UpdateInput::Ready).unwrap();
    assert_eq!(output.snapshot.sequence, 1);
    assert_eq!(output.snapshot.current_version, "2.0.15");
    assert_eq!(output.snapshot.state, UpdateState::Checking);
    assert_eq!(
        output.snapshot.next_check_at,
        Some(Duration::from_secs(21_610))
    );
    assert_eq!(
        output.effects,
        vec![
            UpdateEffect::CheckStable,
            UpdateEffect::ScheduleCheck {
                after: Duration::from_secs(21_600)
            },
        ]
    );
}

/// 手动检查使用同一状态机，且与启动或周期检查合并为一个操作。
#[test]
fn startup_periodic_and_manual_checks_coalesce() {
    let mut controller = controller(None);
    controller.handle(UpdateInput::Ready).unwrap();
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
            after: Duration::from_secs(21_600)
        }]
    );
    controller
        .handle(UpdateInput::CheckSucceeded { release: None })
        .unwrap();
    let manual = controller.handle(UpdateInput::ManualCheck).unwrap();
    assert_eq!(manual.effects, vec![UpdateEffect::CheckStable]);
}

/// Ready 之前的周期信号不会偷偷启动后台调度。
#[test]
fn periodic_check_does_not_start_before_ready() {
    let output = controller(None)
        .handle(UpdateInput::PeriodicCheckDue)
        .unwrap();
    assert_eq!(output.snapshot.state, UpdateState::Idle);
    assert!(output.effects.is_empty());
}

/// 启动 Ready 与已经开始的手动检查合并时仍会建立后续六小时计划。
#[test]
fn ready_coalesces_existing_manual_check_and_keeps_periodic_schedule() {
    let mut controller = controller(None);
    controller.handle(UpdateInput::ManualCheck).unwrap();
    let output = controller.handle(UpdateInput::Ready).unwrap();
    assert_eq!(
        output.effects,
        vec![UpdateEffect::ScheduleCheck {
            after: Duration::from_secs(21_600)
        }]
    );
    assert_eq!(output.snapshot.state, UpdateState::Checking);
}

/// 当前版本、旧版本与非法版本都不会触发候选下载。
#[test]
fn semver_policy_prevents_downgrade_and_reinstall() {
    for version in ["2.0.14", "2.0.15"] {
        let output = controller(None)
            .handle(UpdateInput::CheckSucceeded {
                release: Some(release(version)),
            })
            .unwrap();
        assert_eq!(output.snapshot.state, UpdateState::UpToDate);
        assert!(output.effects.is_empty());
    }
    let error = controller(None)
        .handle(UpdateInput::CheckSucceeded {
            release: Some(release("not-semver")),
        })
        .unwrap_err();
    assert!(matches!(
        error,
        crate::application_update::UpdateDomainError::InvalidReleaseVersion(_)
    ));
    let prerelease = controller(None)
        .handle(UpdateInput::CheckSucceeded {
            release: Some(release("2.1.0-beta.1")),
        })
        .unwrap_err();
    assert!(matches!(
        prerelease,
        crate::application_update::UpdateDomainError::InvalidReleaseVersion(_)
    ));
}

/// 可信包契约拒绝非 HTTPS 地址和空信任字段。
#[test]
fn release_package_contract_fails_closed() {
    for url in ["http://example.test/package", "https://"] {
        assert!(ReleasePackage::new(
            url,
            "signature",
            "darwin-aarch64",
            InstallKind::MacosAppArchive,
        )
        .is_err());
    }
    assert!(ReleasePackage::new(
        "https://example.test/package",
        "",
        "darwin-aarch64",
        InstallKind::MacosAppArchive,
    )
    .is_err());
}

/// 自动下载默认开启；关闭后仍展示候选，重新开启会继续可信下载。
#[test]
fn automatic_download_defaults_on_and_can_resume_available_candidate() {
    let preferences = MemoryPreferences::default();
    let mut controller = UpdateController::new(
        "2.0.15",
        TestClock(Rc::new(Cell::new(Duration::ZERO))),
        preferences.clone(),
    )
    .unwrap();
    assert!(controller.snapshot().automatic_download);
    controller
        .handle(UpdateInput::SetAutomaticDownload(false))
        .unwrap();
    let available = controller
        .handle(UpdateInput::CheckSucceeded {
            release: Some(release("2.1.0")),
        })
        .unwrap();
    assert!(matches!(
        available.snapshot.state,
        UpdateState::Available { .. }
    ));
    assert!(available.effects.is_empty());
    let resumed = controller
        .handle(UpdateInput::SetAutomaticDownload(true))
        .unwrap();
    assert_eq!(
        resumed.effects,
        vec![UpdateEffect::StartDownload {
            release: release("2.1.0")
        }]
    );
    let restarted = UpdateController::new(
        "2.0.15",
        TestClock(Rc::new(Cell::new(Duration::ZERO))),
        preferences,
    )
    .unwrap();
    assert!(restarted.snapshot().automatic_download);
}

/// 发现更高候选时先串行清理旧 staging，再按偏好开始下载。
#[test]
fn higher_candidate_replaces_staging_before_download() {
    let mut controller = controller(None);
    controller
        .handle(UpdateInput::RecoverStaged {
            version: "2.1.0".into(),
        })
        .unwrap();
    let discovered = controller
        .handle(UpdateInput::CheckSucceeded {
            release: Some(release("2.2.0")),
        })
        .unwrap();
    assert_eq!(
        discovered.effects,
        vec![UpdateEffect::DiscardStaged {
            replacement_version: "2.2.0".into()
        }]
    );
    assert!(controller
        .handle(UpdateInput::ManualCheck)
        .unwrap()
        .effects
        .is_empty());
    let discarded = controller.handle(UpdateInput::StagingDiscarded).unwrap();
    assert_eq!(
        discarded.effects,
        vec![UpdateEffect::StartDownload {
            release: release("2.2.0")
        }]
    );
}

/// 旧 staging 清理失败会停止下载并产生不可重试的稳定存储错误。
#[test]
fn staging_discard_failure_is_non_retryable_and_stops_replacement() {
    let mut controller = controller(None);
    controller
        .handle(UpdateInput::RecoverStaged {
            version: "2.1.0".into(),
        })
        .unwrap();
    controller
        .handle(UpdateInput::CheckSucceeded {
            release: Some(release("2.2.0")),
        })
        .unwrap();
    let failed = controller
        .handle(UpdateInput::StagingDiscardFailed)
        .unwrap();
    assert!(failed.effects.is_empty());
    assert!(matches!(
        failed.snapshot.state,
        UpdateState::Failed {
            operation: UpdateOperation::Download,
            category: UpdateFailureCategory::Storage,
            retryable: false,
            ..
        }
    ));
}

/// 下载进度同时覆盖未知总量与已知总量并保持 sequence 单调。
#[test]
fn progress_snapshots_are_complete_and_monotonic() {
    let mut controller = controller(None);
    controller
        .handle(UpdateInput::CheckSucceeded {
            release: Some(release("2.1.0")),
        })
        .unwrap();
    let started = controller.handle(UpdateInput::DownloadStarted).unwrap();
    let unknown = controller
        .handle(UpdateInput::DownloadAdvanced(
            DownloadProgress::UnknownTotal {
                downloaded_bytes: 512,
            },
        ))
        .unwrap();
    let known = controller
        .handle(UpdateInput::DownloadAdvanced(
            DownloadProgress::KnownTotal {
                downloaded_bytes: 1024,
                total_bytes: 4096,
            },
        ))
        .unwrap();
    assert!(matches!(
        unknown.snapshot.state,
        UpdateState::Downloading { .. }
    ));
    assert!(matches!(
        known.snapshot.state,
        UpdateState::Downloading {
            progress: DownloadProgress::KnownTotal {
                downloaded_bytes: 1024,
                total_bytes: 4096
            },
            ..
        }
    ));
    assert!(started.snapshot.sequence < unknown.snapshot.sequence);
    assert!(unknown.snapshot.sequence < known.snapshot.sequence);
}

/// 序列化快照使用 camelCase 字段且不会暴露包地址或签名。
#[test]
fn serialized_snapshot_is_provider_neutral_and_redacted() {
    let output = controller(Some(false))
        .handle(UpdateInput::CheckSucceeded {
            release: Some(release("2.1.0")),
        })
        .unwrap();
    let json = serde_json::to_value(output.snapshot).unwrap();
    assert_eq!(json["currentVersion"], "2.0.15");
    assert_eq!(json["automaticDownload"], false);
    assert_eq!(json["state"]["kind"], "available");
    assert_eq!(json["state"]["releaseNotes"], "修复启动问题");
    let text = json.to_string();
    assert!(!text.contains("example.test"));
    assert!(!text.contains("signature-"));
}

/// 网络失败只安排三次一、二、四秒重试，陈旧 token 不能重复启动操作。
#[test]
fn retryable_failures_have_bounded_backoff_and_generation_tokens() {
    let mut controller = controller(None);
    let mut scheduled = Vec::new();
    let mut first_token = None;
    for attempt in 0..4 {
        let output = controller
            .handle(UpdateInput::CheckFailed(UpdateFailure::new(
                UpdateFailureCategory::Network,
                "offline",
            )))
            .unwrap();
        for effect in output.effects {
            if let UpdateEffect::ScheduleRetry { token, after, .. } = effect {
                if attempt == 0 {
                    first_token = Some(token);
                }
                scheduled.push(after);
            }
        }
    }
    assert_eq!(
        scheduled,
        vec![
            Duration::from_secs(1),
            Duration::from_secs(2),
            Duration::from_secs(4)
        ]
    );
    assert!(controller
        .handle(UpdateInput::RetryDue {
            token: first_token.unwrap()
        })
        .unwrap()
        .effects
        .is_empty());
}

/// 408、429、5xx 与验证失败由固定类别决定重试能力。
#[test]
fn failure_categories_have_stable_retryability() {
    for status in [408, 429, 500, 503] {
        assert!(UpdateFailure::from_http_status(status, "http").retryable());
    }
    assert!(!UpdateFailure::from_http_status(404, "http").retryable());
    let output = controller(None)
        .handle(UpdateInput::DownloadFailed(UpdateFailure::new(
            UpdateFailureCategory::Verification,
            "signature mismatch",
        )))
        .unwrap();
    assert!(output.effects.is_empty());
    assert!(matches!(
        output.snapshot.state,
        UpdateState::Failed {
            operation: UpdateOperation::Download,
            category: UpdateFailureCategory::Verification,
            retryable: false,
            ..
        }
    ));
}

/// 用户显式 retry 只重放 Rust 已记录的可信下载上下文。
#[test]
fn user_retry_reuses_trusted_release_and_invalidates_timer() {
    let mut controller = controller(None);
    controller
        .handle(UpdateInput::CheckSucceeded {
            release: Some(release("2.1.0")),
        })
        .unwrap();
    let failed = controller
        .handle(UpdateInput::DownloadFailed(UpdateFailure::new(
            UpdateFailureCategory::Network,
            "reset",
        )))
        .unwrap();
    let token = failed
        .effects
        .iter()
        .find_map(|effect| match effect {
            UpdateEffect::ScheduleRetry { token, .. } => Some(*token),
            _ => None,
        })
        .unwrap();
    let retried = controller.handle(UpdateInput::UserRetry).unwrap();
    assert_eq!(
        retried.effects,
        vec![UpdateEffect::StartDownload {
            release: release("2.1.0")
        }]
    );
    assert!(controller
        .handle(UpdateInput::RetryDue { token })
        .unwrap()
        .effects
        .is_empty());
}

/// 只有 staged 状态允许发出不含路径或签名的更新重启请求。
#[test]
fn update_restart_requires_staged_package() {
    let mut controller = controller(None);
    assert!(controller
        .handle(UpdateInput::RequestUpdateRestart)
        .unwrap()
        .effects
        .is_empty());
    controller
        .handle(UpdateInput::CheckSucceeded {
            release: Some(release("2.1.0")),
        })
        .unwrap();
    controller.handle(UpdateInput::DownloadSucceeded).unwrap();
    assert_eq!(
        controller
            .handle(UpdateInput::RequestUpdateRestart)
            .unwrap()
            .effects,
        vec![UpdateEffect::RequestInstall {
            intent: InstallIntent::UpdateRestart,
            version: "2.1.0".into()
        }]
    );
}
