use crate::application_update::{
    Clock, InstallAttemptError, InstallCleanupDisposition, InstallError, InstallExecutionOutcome,
    InstallIntent, InstallKind, InstallOutcome, MinisignVerifier, PendingStagedUpdate,
    PreferenceError, PreferenceStore, ReleasePackage, StableRelease, StageCandidate,
    StageCommitOutcome, StagingCommitPoint, StagingContext, StagingError, StagingFaultInjector,
    StagingRepository, UpdateController, UpdateEffect, UpdateInput, UpdateInstaller, UpdateState,
    VerifiedStagedPackage,
};
use fs2::FileExt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

const TARGET: &str = "darwin-aarch64";

/// Repository→Controller 组合测试使用的固定单调时钟。
struct FixedClock;

impl Clock for FixedClock {
    /// 返回不影响本测试的固定时间。
    fn now(&self) -> Duration {
        Duration::ZERO
    }
}

/// Repository→Controller 组合测试使用的默认开启偏好。
struct EnabledPreferences;

impl PreferenceStore for EnabledPreferences {
    /// 始终返回默认开启的自动下载偏好。
    fn load_automatic_download(&self) -> Result<Option<bool>, PreferenceError> {
        Ok(Some(true))
    }

    /// 本测试不需要跨实例保存偏好。
    fn save_automatic_download(&mut self, _enabled: bool) -> Result<(), PreferenceError> {
        Ok(())
    }
}

/// 仅接受内容与签名完全匹配的确定性 minisign 测试验证器。
struct DeterministicVerifier;

impl MinisignVerifier for DeterministicVerifier {
    /// 验证测试包内容与测试签名的绑定关系。
    fn verify(&self, package: &Path, signature: &str) -> Result<(), String> {
        let content = fs::read(package).map_err(|error| error.to_string())?;
        if signature == format!("trusted:{}", String::from_utf8_lossy(&content)) {
            Ok(())
        } else {
            Err("signature mismatch".into())
        }
    }
}

/// 在 lease 内读取并核对包内容的同步测试安装器。
struct InspectingInstaller {
    version: &'static str,
    content: &'static [u8],
    outcome: InstallOutcome,
}

impl UpdateInstaller for InspectingInstaller {
    /// 读取 lease 保护的路径并返回测试指定的稳定安装结果。
    fn install(
        &self,
        package: &VerifiedStagedPackage,
        _intent: InstallIntent,
    ) -> Result<InstallOutcome, InstallError> {
        if package.version() == self.version
            && package.target() == TARGET
            && package.install_kind() == InstallKind::MacosAppArchive
            && fs::read(package.path()).map_err(|error| InstallError::new(error.to_string()))?
                == self.content
        {
            Ok(self.outcome)
        } else {
            Err(InstallError::new("unexpected verified package"))
        }
    }
}

/// 在安装调用内保存句柄 Debug 文本的测试安装器。
struct DebugCaptureInstaller(Arc<Mutex<String>>);

impl UpdateInstaller for DebugCaptureInstaller {
    /// 捕获只能在 lease 内观察的已验证句柄 Debug 文本。
    fn install(
        &self,
        package: &VerifiedStagedPackage,
        _intent: InstallIntent,
    ) -> Result<InstallOutcome, InstallError> {
        *self.0.lock().unwrap() = format!("{package:?}");
        Ok(InstallOutcome::HandedOff)
    }
}

/// 指定并发测试在 installer 打开包之前或读取期间暂停。
#[derive(Clone, Copy)]
enum GateMoment {
    BeforeOpen,
    DuringRead,
}

/// 用 channel 暂停同步安装器以观察另一个 repository 是否越过 lease。
struct GateInstaller {
    moment: GateMoment,
    reached: mpsc::SyncSender<()>,
    proceed: Mutex<mpsc::Receiver<()>>,
    outcome: InstallOutcome,
}

impl UpdateInstaller for GateInstaller {
    /// 在指定时刻暂停，并确认 package 在完整 lease 期间始终可读取。
    fn install(
        &self,
        package: &VerifiedStagedPackage,
        _intent: InstallIntent,
    ) -> Result<InstallOutcome, InstallError> {
        let content = match self.moment {
            GateMoment::BeforeOpen => {
                self.reached.send(()).unwrap();
                self.proceed.lock().unwrap().recv().unwrap();
                fs::read(package.path()).map_err(|error| InstallError::new(error.to_string()))?
            }
            GateMoment::DuringRead => {
                let mut file = fs::File::open(package.path())
                    .map_err(|error| InstallError::new(error.to_string()))?;
                self.reached.send(()).unwrap();
                self.proceed.lock().unwrap().recv().unwrap();
                let mut content = Vec::new();
                file.read_to_end(&mut content)
                    .map_err(|error| InstallError::new(error.to_string()))?;
                content
            }
        };
        if content != b"package-v2" {
            return Err(InstallError::new("package changed during install lease"));
        }
        Ok(self.outcome)
    }
}

/// 在指定事务边界模拟存储失败。
struct FailAt(StagingCommitPoint);

impl StagingFaultInjector for FailAt {
    /// 命中目标提交点时返回稳定存储错误。
    fn checkpoint(&self, point: StagingCommitPoint) -> Result<(), StagingError> {
        if point == self.0 {
            Err(StagingError::StorageFailed)
        } else {
            Ok(())
        }
    }
}

/// 在多个指定事务边界模拟存储失败。
struct FailAtMany(Vec<StagingCommitPoint>);

impl StagingFaultInjector for FailAtMany {
    /// 命中任一目标提交点时返回稳定存储错误。
    fn checkpoint(&self, point: StagingCommitPoint) -> Result<(), StagingError> {
        if self.0.contains(&point) {
            Err(StagingError::StorageFailed)
        } else {
            Ok(())
        }
    }
}

/// 可在 repository 打开后才启用的单点故障注入器。
struct ArmedFailAt {
    point: StagingCommitPoint,
    armed: Arc<AtomicBool>,
}

impl StagingFaultInjector for ArmedFailAt {
    /// 只有测试显式 armed 后才在目标边界返回存储错误。
    fn checkpoint(&self, point: StagingCommitPoint) -> Result<(), StagingError> {
        if point == self.point && self.armed.load(Ordering::SeqCst) {
            Err(StagingError::StorageFailed)
        } else {
            Ok(())
        }
    }
}

/// 统计平台安装调用次数并返回固定主结果。
struct CountingInstaller {
    calls: Arc<AtomicUsize>,
    outcome: InstallOutcome,
}

impl UpdateInstaller for CountingInstaller {
    /// 读取已复验包并记录一次不可逆的平台安装调用。
    fn install(
        &self,
        package: &VerifiedStagedPackage,
        _intent: InstallIntent,
    ) -> Result<InstallOutcome, InstallError> {
        if fs::read(package.path()).map_err(|error| InstallError::new(error.to_string()))?
            != b"package-v2"
        {
            return Err(InstallError::new("unexpected package content"));
        }
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.outcome)
    }
}

/// 先产生部分内容再模拟网络读取中断的输入流。
struct InterruptedReader {
    emitted: bool,
}

impl Read for InterruptedReader {
    /// 第一次读取返回部分内容，之后返回确定性错误。
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.emitted {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "interrupted"));
        }
        self.emitted = true;
        let partial = b"partial";
        buffer[..partial.len()].copy_from_slice(partial);
        Ok(partial.len())
    }
}

/// 创建与当前测试目标绑定的恢复上下文。
fn context(current_version: &str) -> StagingContext {
    StagingContext::new(current_version, TARGET, InstallKind::MacosAppArchive).unwrap()
}

/// 创建绑定目标、内容签名和公开说明的暂存候选。
fn candidate(version: &str, content: &str) -> StageCandidate {
    candidate_with_notes(version, content, format!("{version} 说明"))
}

/// 创建带指定 Release Notes 的暂存候选。
fn candidate_with_notes(version: &str, content: &str, notes: impl AsRef<str>) -> StageCandidate {
    StageCandidate::new(
        version,
        format!("trusted:{content}"),
        TARGET,
        InstallKind::MacosAppArchive,
        notes,
    )
    .unwrap()
}

/// 创建 Repository→Controller 组合测试使用的 Stable 发布。
fn stable_release(version: &str, content: &str) -> StableRelease {
    StableRelease::new(
        version,
        format!("{version} 说明"),
        ReleasePackage::new(
            format!("https://example.test/{version}"),
            format!("trusted:{content}"),
            TARGET,
            InstallKind::MacosAppArchive,
        )
        .unwrap(),
    )
}

/// 读取原子 pointer 指向的 generation 目录。
fn active_generation(cache_root: &Path) -> PathBuf {
    let root = cache_root.join("application-update");
    let generation = fs::read_to_string(root.join("current")).unwrap();
    root.join(generation)
}

/// 返回 staging 根内 generation 的数量。
fn generation_count(cache_root: &Path) -> usize {
    fs::read_dir(cache_root.join("application-update"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| {
            entry.file_type().is_ok_and(|kind| kind.is_dir())
                && entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("generation-")
        })
        .count()
}

/// 完整 generation 通过单一 pointer 提交并可跨实例恢复。
#[test]
fn staged_update_is_durable_generation_with_bounded_notes() {
    let cache = tempfile::tempdir().unwrap();
    let mut repository =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    let notes = "说".repeat(20_000);
    repository
        .stage(
            candidate_with_notes("2.1.0", "package-v2", &notes),
            "package-v2".as_bytes(),
        )
        .unwrap();
    assert_eq!(generation_count(cache.path()), 1);
    let generation = active_generation(cache.path());
    let mut names = fs::read_dir(generation)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, vec!["metadata.json", "package.bin"]);
    drop(repository);

    let recovered =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    let snapshot = recovered.snapshot().unwrap();
    assert_eq!(snapshot.version(), "2.1.0");
    assert!(!snapshot.release_notes().is_empty());
    assert!(snapshot.release_notes().len() <= 32 * 1024);
    assert_eq!(recovered.recovery_failure(), None);
}

/// 更高候选只有在完整验证和提交成功后才替换旧可信 generation。
#[test]
fn successful_higher_candidate_atomically_replaces_old_generation() {
    let cache = tempfile::tempdir().unwrap();
    let mut repository =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    repository
        .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
        .unwrap();
    for version in ["2.0.15", "2.0.14", "2.1.0"] {
        assert_eq!(
            repository.stage(candidate(version, "duplicate"), "duplicate".as_bytes()),
            Err(StagingError::NotNewer)
        );
    }
    repository
        .stage(candidate("2.2.0", "package-v3"), "package-v3".as_bytes())
        .unwrap();
    assert_eq!(generation_count(cache.path()), 1);
    assert_eq!(
        repository
            .install_with(
                &InspectingInstaller {
                    version: "2.2.0",
                    content: b"package-v3",
                    outcome: InstallOutcome::HandedOff,
                },
                InstallIntent::UpdateRestart,
            )
            .unwrap(),
        InstallExecutionOutcome::HandedOff
    );
}

/// 流中断或验签失败不会删除、覆盖或降级旧可信 staging。
#[test]
fn stream_and_verification_failures_preserve_existing_staging() {
    let cache = tempfile::tempdir().unwrap();
    let mut repository =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    repository
        .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
        .unwrap();
    assert_eq!(
        repository.stage(
            candidate("2.2.0", "partial"),
            InterruptedReader { emitted: false },
        ),
        Err(StagingError::StorageFailed)
    );
    assert_eq!(repository.snapshot().unwrap().version(), "2.1.0");
    assert_eq!(
        repository.stage(candidate("2.2.0", "expected"), "wrong".as_bytes()),
        Err(StagingError::VerificationFailed)
    );
    assert_eq!(
        repository
            .install_with(
                &InspectingInstaller {
                    version: "2.1.0",
                    content: b"package-v2",
                    outcome: InstallOutcome::HandedOff,
                },
                InstallIntent::UpdateRestart,
            )
            .unwrap(),
        InstallExecutionOutcome::HandedOff
    );
    drop(repository);
    let recovered =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    assert_eq!(recovered.snapshot().unwrap().version(), "2.1.0");
    assert_eq!(generation_count(cache.path()), 1);
}

/// pointer 提交前任一崩溃点都恢复旧 generation，绝不暴露半提交候选。
#[test]
fn every_pre_pointer_failure_recovers_old_generation() {
    for point in [
        StagingCommitPoint::AfterPackageSync,
        StagingCommitPoint::AfterVerification,
        StagingCommitPoint::AfterMetadataSync,
        StagingCommitPoint::AfterGenerationSync,
        StagingCommitPoint::BeforePointerCommit,
    ] {
        let cache = tempfile::tempdir().unwrap();
        let mut initial =
            StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier)
                .unwrap();
        initial
            .stage(candidate("2.1.0", "old"), "old".as_bytes())
            .unwrap();
        drop(initial);
        let mut repository = StagingRepository::open_with_faults(
            cache.path(),
            context("2.0.15"),
            DeterministicVerifier,
            FailAt(point),
        )
        .unwrap();
        assert_eq!(
            repository.stage(candidate("2.2.0", "new"), "new".as_bytes()),
            Err(StagingError::StorageFailed),
            "failed at {point:?}"
        );
        assert_eq!(repository.snapshot().unwrap().version(), "2.1.0");
        drop(repository);
        let recovered =
            StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier)
                .unwrap();
        assert_eq!(recovered.snapshot().unwrap().version(), "2.1.0");
        assert_eq!(generation_count(cache.path()), 1);
    }
}

/// pointer 后只有跨过显式 durability barrier 才会暴露可安装快照。
#[test]
fn post_pointer_outcome_matches_new_generation() {
    for point in [
        StagingCommitPoint::AfterPointerCommit,
        StagingCommitPoint::AfterPointerSync,
    ] {
        let cache = tempfile::tempdir().unwrap();
        let mut initial =
            StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier)
                .unwrap();
        initial
            .stage(candidate("2.1.0", "old"), "old".as_bytes())
            .unwrap();
        drop(initial);
        let mut repository = StagingRepository::open_with_faults(
            cache.path(),
            context("2.0.15"),
            DeterministicVerifier,
            FailAt(point),
        )
        .unwrap();
        let outcome = repository
            .stage(candidate("2.2.0", "new"), "new".as_bytes())
            .unwrap();
        match outcome {
            StageCommitOutcome::DurabilityUnknown(pending) => {
                let pending: PendingStagedUpdate = pending;
                assert_eq!(point, StagingCommitPoint::AfterPointerCommit);
                assert_eq!(pending.version(), "2.2.0");
                assert!(!pending.release_notes().is_empty());
                assert_eq!(repository.snapshot(), None);
                let confirmed = repository.confirm_durability(&pending).unwrap();
                assert_eq!(confirmed.version(), "2.2.0");
            }
            StageCommitOutcome::Committed(staged) => {
                assert_eq!(point, StagingCommitPoint::AfterPointerSync);
                assert_eq!(staged.version(), "2.2.0");
            }
        }
        assert_eq!(repository.snapshot().unwrap().version(), "2.2.0");
        drop(repository);
        let recovered =
            StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier)
                .unwrap();
        let mut recovered = recovered;
        if recovered.snapshot().is_none() {
            let pending = recovered.pending_durability().unwrap();
            recovered.confirm_durability(&pending).unwrap();
        }
        assert_eq!(recovered.snapshot().unwrap().version(), "2.2.0");
        assert_eq!(generation_count(cache.path()), 1);
    }
}

/// 首次 staging 的 unknown 在确认前拒绝替换、Controller 发布与安装器消费。
#[test]
fn first_staging_requires_durability_confirmation_before_publish_and_install() {
    let cache = tempfile::tempdir().unwrap();
    let mut controller = UpdateController::new("2.0.15", FixedClock, EnabledPreferences).unwrap();
    let check = controller.handle(UpdateInput::ManualCheck).unwrap();
    let check_id = check
        .effects
        .into_iter()
        .find_map(|effect| match effect {
            UpdateEffect::CheckStable { operation_id } => Some(operation_id),
            _ => None,
        })
        .unwrap();
    let download = controller
        .handle(UpdateInput::CheckSucceeded {
            operation_id: check_id,
            release: Some(stable_release("2.1.0", "package-v2")),
        })
        .unwrap();
    let download_id = download
        .effects
        .into_iter()
        .find_map(|effect| match effect {
            UpdateEffect::StartDownload { operation_id, .. } => Some(operation_id),
            _ => None,
        })
        .unwrap();

    let mut repository = StagingRepository::open_with_faults(
        cache.path(),
        context("2.0.15"),
        DeterministicVerifier,
        FailAt(StagingCommitPoint::AfterPointerCommit),
    )
    .unwrap();
    let outcome = repository
        .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
        .unwrap();
    let StageCommitOutcome::DurabilityUnknown(pending) = outcome else {
        panic!("pointer 后同步故障必须产生 unknown identity");
    };
    assert_eq!(repository.snapshot(), None);
    let current_before = fs::read(cache.path().join("application-update/current")).unwrap();
    assert_eq!(generation_count(cache.path()), 1);
    assert_eq!(
        repository.stage(candidate("2.2.0", "new"), "new".as_bytes()),
        Err(StagingError::DurabilityUnconfirmed)
    );
    assert_eq!(
        fs::read(cache.path().join("application-update/current")).unwrap(),
        current_before
    );
    assert_eq!(generation_count(cache.path()), 1);
    assert!(!cache
        .path()
        .join("application-update/pending-durability")
        .exists());
    assert!(controller
        .handle(UpdateInput::RequestUpdateRestart)
        .unwrap()
        .effects
        .is_empty());
    let calls = Arc::new(AtomicUsize::new(0));
    let installer = CountingInstaller {
        calls: calls.clone(),
        outcome: InstallOutcome::HandedOff,
    };
    assert_eq!(
        repository.install_with(&installer, InstallIntent::UpdateRestart),
        Err(InstallAttemptError::Staging(
            StagingError::DurabilityUnconfirmed
        ))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let authoritative = repository.confirm_durability(&pending).unwrap();
    assert_eq!(authoritative.version(), "2.1.0");
    let published = controller
        .handle(UpdateInput::DownloadSucceeded {
            operation_id: download_id,
            staged: authoritative,
        })
        .unwrap();
    assert_eq!(published.snapshot.state, UpdateState::Staged);
    assert_eq!(published.snapshot.candidate.unwrap().version, "2.1.0");
    assert_eq!(repository.snapshot().unwrap().version(), "2.1.0");
    assert_eq!(
        repository
            .install_with(&installer, InstallIntent::UpdateRestart)
            .unwrap(),
        InstallExecutionOutcome::HandedOff
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// 预先打开的第二实例先确认 G1 磁盘视图，G2 pointer 前失败也只能保留 G1。
#[test]
fn second_repository_barriers_unknown_before_attempting_higher_generation() {
    let cache = tempfile::tempdir().unwrap();
    let mut first = StagingRepository::open_with_faults(
        cache.path(),
        context("2.0.15"),
        DeterministicVerifier,
        FailAt(StagingCommitPoint::AfterPointerCommit),
    )
    .unwrap();
    let mut second = StagingRepository::open_with_faults(
        cache.path(),
        context("2.0.15"),
        DeterministicVerifier,
        FailAt(StagingCommitPoint::BeforePointerCommit),
    )
    .unwrap();
    assert!(matches!(
        first
            .stage(candidate("2.1.0", "old"), "old".as_bytes())
            .unwrap(),
        StageCommitOutcome::DurabilityUnknown(_)
    ));
    assert_eq!(
        second.stage(candidate("2.2.0", "new"), "new".as_bytes()),
        Err(StagingError::StorageFailed)
    );
    assert_eq!(second.snapshot().unwrap().version(), "2.1.0");
    assert_eq!(generation_count(cache.path()), 1);
    drop(first);
    drop(second);
    let recovered =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    assert_eq!(recovered.snapshot().unwrap().version(), "2.1.0");
}

/// 第二实例的 refresh barrier 失败时隐藏 G1，且不得开始写入 G2。
#[test]
fn second_repository_refresh_failure_keeps_unknown_hidden_and_untouched() {
    let cache = tempfile::tempdir().unwrap();
    let mut first = StagingRepository::open_with_faults(
        cache.path(),
        context("2.0.15"),
        DeterministicVerifier,
        FailAt(StagingCommitPoint::AfterPointerCommit),
    )
    .unwrap();
    let armed = Arc::new(AtomicBool::new(false));
    let mut second = StagingRepository::open_with_faults(
        cache.path(),
        context("2.0.15"),
        DeterministicVerifier,
        ArmedFailAt {
            point: StagingCommitPoint::RefreshDurabilitySync,
            armed: armed.clone(),
        },
    )
    .unwrap();
    assert!(matches!(
        first
            .stage(candidate("2.1.0", "old"), "old".as_bytes())
            .unwrap(),
        StageCommitOutcome::DurabilityUnknown(_)
    ));
    let pointer = fs::read(cache.path().join("application-update/current")).unwrap();
    armed.store(true, Ordering::SeqCst);
    assert_eq!(
        second.stage(candidate("2.2.0", "new"), "new".as_bytes()),
        Err(StagingError::StorageFailed)
    );
    assert_eq!(second.snapshot(), None);
    assert_eq!(second.pending_durability().unwrap().version(), "2.1.0");
    assert_eq!(
        fs::read(cache.path().join("application-update/current")).unwrap(),
        pointer
    );
    assert_eq!(generation_count(cache.path()), 1);
}

/// durability barrier 自身失败时保持不可安装，并给出稳定仓储诊断。
#[test]
fn failed_durability_confirmation_remains_non_installable() {
    let cache = tempfile::tempdir().unwrap();
    let mut repository = StagingRepository::open_with_faults(
        cache.path(),
        context("2.0.15"),
        DeterministicVerifier,
        FailAtMany(vec![
            StagingCommitPoint::AfterPointerCommit,
            StagingCommitPoint::DurabilityConfirmationSync,
        ]),
    )
    .unwrap();
    let StageCommitOutcome::DurabilityUnknown(pending) = repository
        .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
        .unwrap()
    else {
        panic!("必须保留 unknown identity");
    };
    assert_eq!(
        repository.confirm_durability(&pending),
        Err(StagingError::StorageFailed)
    );
    assert_eq!(repository.snapshot(), None);
    let calls = Arc::new(AtomicUsize::new(0));
    assert_eq!(
        repository.install_with(
            &CountingInstaller {
                calls: calls.clone(),
                outcome: InstallOutcome::Installed,
            },
            InstallIntent::NormalExit,
        ),
        Err(InstallAttemptError::Staging(
            StagingError::DurabilityUnconfirmed
        ))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

/// unknown 进程退出后，新实例只有完成恢复 root barrier 才能产生 durable proof。
#[test]
fn recovery_barrier_fails_closed_before_promoting_unknown_pointer() {
    let cache = tempfile::tempdir().unwrap();
    let mut repository = StagingRepository::open_with_faults(
        cache.path(),
        context("2.0.15"),
        DeterministicVerifier,
        FailAt(StagingCommitPoint::AfterPointerCommit),
    )
    .unwrap();
    assert!(matches!(
        repository
            .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
            .unwrap(),
        StageCommitOutcome::DurabilityUnknown(_)
    ));
    drop(repository);

    assert!(matches!(
        StagingRepository::open_with_faults(
            cache.path(),
            context("2.0.15"),
            DeterministicVerifier,
            FailAt(StagingCommitPoint::RecoveryDurabilitySync),
        ),
        Err(StagingError::StorageFailed)
    ));
    let recovered =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    let proof = recovered.snapshot().unwrap();
    assert_eq!(proof.version(), "2.1.0");
    let mut controller = UpdateController::new("2.0.15", FixedClock, EnabledPreferences).unwrap();
    let restored = controller
        .handle(UpdateInput::RecoverStaged { staged: proof })
        .unwrap();
    assert_eq!(restored.snapshot.state, UpdateState::Staged);
}

/// malformed/mismatched 旧 marker 必须先经过 root barrier，清理后才可恢复 durable proof。
#[test]
fn malformed_and_mismatched_legacy_markers_never_fail_open() {
    for marker in ["../malformed", "generation-mismatched"] {
        let cache = tempfile::tempdir().unwrap();
        let mut initial =
            StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier)
                .unwrap();
        initial
            .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
            .unwrap();
        drop(initial);
        let marker_path = cache.path().join("application-update/pending-durability");
        fs::write(&marker_path, marker).unwrap();

        assert!(matches!(
            StagingRepository::open_with_faults(
                cache.path(),
                context("2.0.15"),
                DeterministicVerifier,
                FailAt(StagingCommitPoint::RecoveryDurabilitySync),
            ),
            Err(StagingError::StorageFailed)
        ));
        assert!(marker_path.exists());
        let recovered =
            StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier)
                .unwrap();
        assert_eq!(recovered.snapshot().unwrap().version(), "2.1.0");
        assert!(!marker_path.exists());
    }
}

/// 旧 marker 无法删除时，每次恢复仍先做 barrier，不会把 durable current 反向降级。
#[test]
fn legacy_marker_cleanup_failure_cannot_downgrade_durable_current() {
    let cache = tempfile::tempdir().unwrap();
    let mut initial =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    initial
        .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
        .unwrap();
    drop(initial);
    let marker_path = cache.path().join("application-update/pending-durability");
    fs::create_dir(&marker_path).unwrap();

    for _ in 0..2 {
        let recovered =
            StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier)
                .unwrap();
        assert_eq!(recovered.snapshot().unwrap().version(), "2.1.0");
        assert!(marker_path.is_dir());
    }
}

/// 两个先后打开的仓储都在锁内重读 pointer，并以 SemVer CAS 防止降级。
#[test]
fn two_repositories_interleave_high_and_low_candidates_without_downgrade() {
    let cache = tempfile::tempdir().unwrap();
    let mut first =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    let mut second =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    first
        .stage(candidate("2.2.0", "high"), "high".as_bytes())
        .unwrap();
    assert_eq!(
        second.stage(candidate("2.1.0", "low"), "low".as_bytes()),
        Err(StagingError::NotNewer)
    );
    second
        .stage(candidate("2.3.0", "higher"), "higher".as_bytes())
        .unwrap();
    assert_eq!(
        first.stage(candidate("2.2.5", "middle"), "middle".as_bytes()),
        Err(StagingError::NotNewer)
    );
    let recovered =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    assert_eq!(recovered.snapshot().unwrap().version(), "2.3.0");
}

/// 两个线程持有不同仓储实例时，共享文件锁仍保证最终版本为最高候选。
#[test]
fn concurrent_repositories_are_serialized_by_cross_process_lock() {
    let cache = tempfile::tempdir().unwrap();
    let first =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    let second =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    let low = std::thread::spawn(move || {
        let mut repository = first;
        repository.stage(candidate("2.1.0", "low"), "low".as_bytes())
    });
    let high = std::thread::spawn(move || {
        let mut repository = second;
        repository.stage(candidate("2.2.0", "high"), "high".as_bytes())
    });
    let _ = low.join().unwrap();
    let _ = high.join().unwrap();
    let recovered =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    assert_eq!(recovered.snapshot().unwrap().version(), "2.2.0");
    assert_eq!(generation_count(cache.path()), 1);
}

/// 运行一次安装 lease 与并发更高候选 stage 的确定性交错。
fn assert_install_lease_blocks_stage(moment: GateMoment, outcome: InstallOutcome) {
    let cache = tempfile::tempdir().unwrap();
    let mut installing =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    installing
        .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
        .unwrap();
    let mut competing =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();

    let (reached_tx, reached_rx) = mpsc::sync_channel(0);
    let (proceed_tx, proceed_rx) = mpsc::sync_channel(0);
    let installer = GateInstaller {
        moment,
        reached: reached_tx,
        proceed: Mutex::new(proceed_rx),
        outcome,
    };
    let install_thread = std::thread::spawn(move || {
        installing.install_with(&installer, InstallIntent::UpdateRestart)
    });
    reached_rx.recv_timeout(Duration::from_secs(2)).unwrap();

    let lock_file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(cache.path().join("application-update/.lock"))
        .unwrap();
    assert!(FileExt::try_lock_exclusive(&lock_file).is_err());

    let (stage_started_tx, stage_started_rx) = mpsc::sync_channel(0);
    let (stage_done_tx, stage_done_rx) = mpsc::sync_channel(0);
    let stage_thread = std::thread::spawn(move || {
        stage_started_tx.send(()).unwrap();
        let result = competing.stage(candidate("2.2.0", "package-v3"), "package-v3".as_bytes());
        stage_done_tx.send(result).unwrap();
    });
    stage_started_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap();
    assert!(matches!(
        stage_done_rx.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));

    proceed_tx.send(()).unwrap();
    let expected = match outcome {
        InstallOutcome::Installed => InstallExecutionOutcome::Installed {
            cleanup: InstallCleanupDisposition::Complete,
        },
        InstallOutcome::HandedOff => InstallExecutionOutcome::HandedOff,
    };
    assert_eq!(install_thread.join().unwrap().unwrap(), expected);
    let staged = stage_done_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    let StageCommitOutcome::Committed(staged) = staged else {
        panic!("释放 install lease 后新候选应完整提交");
    };
    assert_eq!(staged.version(), "2.2.0");
    stage_thread.join().unwrap();
    let recovered =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    assert_eq!(recovered.snapshot().unwrap().version(), "2.2.0");
}

/// 复验完成但 installer 尚未 open 时，另一个 repository 不能删除 generation。
#[test]
fn install_lease_blocks_stage_before_installer_open() {
    assert_install_lease_blocks_stage(GateMoment::BeforeOpen, InstallOutcome::Installed);
}

/// installer 已 open/读取期间仍持有 lease；HandedOff 安全返回后才允许更高候选。
#[test]
fn install_lease_blocks_stage_during_installer_and_releases_after_handoff() {
    assert_install_lease_blocks_stage(GateMoment::DuringRead, InstallOutcome::HandedOff);
}

/// 安装前会重读 pointer 和元数据并重验包，任一篡改都会撤销信任。
#[test]
fn preinstall_reverification_rejects_package_and_metadata_tampering() {
    for tamper_metadata in [false, true] {
        let cache = tempfile::tempdir().unwrap();
        let mut repository =
            StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier)
                .unwrap();
        repository
            .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
            .unwrap();
        let generation = active_generation(cache.path());
        if tamper_metadata {
            let path = generation.join("metadata.json");
            let mut document: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            document["target"] = serde_json::Value::String("windows-x86_64".into());
            fs::write(path, serde_json::to_vec(&document).unwrap()).unwrap();
        } else {
            fs::write(generation.join("package.bin"), b"tampered").unwrap();
        }
        assert!(matches!(
            repository.install_with(
                &InspectingInstaller {
                    version: "2.1.0",
                    content: b"package-v2",
                    outcome: InstallOutcome::Installed,
                },
                InstallIntent::NormalExit,
            ),
            Err(InstallAttemptError::Staging(
                StagingError::VerificationFailed | StagingError::TargetMismatch
            ))
        ));
        assert_eq!(repository.snapshot(), None);
    }
}

/// 不匹配 target、安装类型和预发布候选都不能进入或恢复 staging。
#[test]
fn target_install_kind_and_stable_channel_are_enforced() {
    let cache = tempfile::tempdir().unwrap();
    let mut repository =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    let wrong = StageCandidate::new(
        "2.1.0",
        "trusted:package-v2",
        "windows-x86_64",
        InstallKind::WindowsNsis,
        "notes",
    )
    .unwrap();
    assert_eq!(
        repository.stage(wrong, "package-v2".as_bytes()),
        Err(StagingError::TargetMismatch)
    );
    assert!(matches!(
        StageCandidate::new(
            "2.1.0-beta.1",
            "trusted:package",
            TARGET,
            InstallKind::MacosAppArchive,
            "notes",
        ),
        Err(StagingError::InvalidCandidate)
    ));
    repository
        .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
        .unwrap();
    drop(repository);
    let windows =
        StagingContext::new("2.0.15", "windows-x86_64", InstallKind::WindowsNsis).unwrap();
    let recovered = StagingRepository::open(cache.path(), windows, DeterministicVerifier).unwrap();
    assert_eq!(recovered.snapshot(), None);
    assert_eq!(
        recovered.recovery_failure(),
        Some(StagingError::TargetMismatch)
    );
}

/// 同版本说明可原子回填并跨实例恢复。
#[test]
fn release_notes_backfill_is_durable_and_version_scoped() {
    let cache = tempfile::tempdir().unwrap();
    let mut repository =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    repository
        .stage(
            candidate_with_notes("2.1.0", "package-v2", ""),
            "package-v2".as_bytes(),
        )
        .unwrap();
    assert!(!repository.update_release_notes("2.2.0", "wrong").unwrap());
    assert!(repository
        .update_release_notes("2.1.0", "离线可用说明")
        .unwrap());
    drop(repository);
    let recovered =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    assert_eq!(
        recovered.snapshot().unwrap().release_notes(),
        "离线可用说明"
    );
}

/// invalid current 即使没有 generation 也会被删除并走无条件 root 同步分支。
#[test]
fn invalid_pointer_without_generation_is_removed_durably() {
    let cache = tempfile::tempdir().unwrap();
    let root = cache.path().join("application-update");
    fs::create_dir_all(&root).unwrap();
    fs::write(root.join("current"), b"../invalid-generation").unwrap();

    let recovered =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    assert_eq!(recovered.snapshot(), None);
    assert_eq!(
        recovered.recovery_failure(),
        Some(StagingError::VerificationFailed)
    );
    assert!(!root.join("current").exists());
    assert_eq!(generation_count(cache.path()), 0);
}

/// 平台安装成功是主结果，三个 cleanup 故障点都只产生可恢复 disposition。
#[test]
fn installed_result_survives_every_cleanup_failure_without_retrying_installer() {
    for (point, expected_cleanup) in [
        (
            StagingCommitPoint::BeforeInstallPointerDelete,
            InstallCleanupDisposition::Pending,
        ),
        (
            StagingCommitPoint::AfterInstallPointerDelete,
            InstallCleanupDisposition::DurabilityUnknown,
        ),
        (
            StagingCommitPoint::BeforeInstallGenerationCleanup,
            InstallCleanupDisposition::Pending,
        ),
    ] {
        let cache = tempfile::tempdir().unwrap();
        let mut repository = StagingRepository::open_with_faults(
            cache.path(),
            context("2.0.15"),
            DeterministicVerifier,
            FailAt(point),
        )
        .unwrap();
        repository
            .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
            .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let installer = CountingInstaller {
            calls: calls.clone(),
            outcome: InstallOutcome::Installed,
        };
        assert_eq!(
            repository.install_with(&installer, InstallIntent::NormalExit),
            Ok(InstallExecutionOutcome::Installed {
                cleanup: expected_cleanup,
            }),
            "cleanup fault at {point:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(repository.snapshot(), None);
        assert_eq!(
            repository.install_with(&installer, InstallIntent::NormalExit),
            Err(InstallAttemptError::Staging(StagingError::NoStagedUpdate)),
            "已安装 generation 不得再次调用 installer at {point:?}"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        drop(repository);
        let recovered =
            StagingRepository::open(cache.path(), context("2.1.0"), DeterministicVerifier).unwrap();
        assert_eq!(recovered.snapshot(), None);
        assert!(!cache.path().join("application-update/current").exists());
        assert_eq!(generation_count(cache.path()), 0);
    }
}

/// 安装完成、版本 reconciliation 和无 pointer 的不完整 generation 都会同步清理。
#[test]
fn completion_reconciliation_and_incomplete_recovery_cleanup() {
    let cache = tempfile::tempdir().unwrap();
    let mut repository =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    repository
        .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
        .unwrap();
    assert_eq!(
        repository
            .install_with(
                &InspectingInstaller {
                    version: "2.1.0",
                    content: b"package-v2",
                    outcome: InstallOutcome::Installed,
                },
                InstallIntent::NormalExit,
            )
            .unwrap(),
        InstallExecutionOutcome::Installed {
            cleanup: InstallCleanupDisposition::Complete,
        }
    );
    assert_eq!(repository.snapshot(), None);

    repository
        .stage(candidate("2.2.0", "package-v3"), "package-v3".as_bytes())
        .unwrap();
    assert!(!repository.reconcile_current_version("2.1.0").unwrap());
    assert!(repository.reconcile_current_version("2.2.0").unwrap());
    assert_eq!(repository.snapshot(), None);

    let orphan = cache
        .path()
        .join("application-update/generation-incomplete");
    fs::create_dir_all(&orphan).unwrap();
    fs::write(orphan.join("package.bin"), b"partial").unwrap();
    let recovered =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    assert_eq!(recovered.snapshot(), None);
    assert_eq!(
        recovered.recovery_failure(),
        Some(StagingError::StorageFailed)
    );
    assert_eq!(generation_count(cache.path()), 0);
}

/// 公开 Debug 文本不会泄露签名或本机 staging 路径。
#[test]
fn trusted_handles_redact_sensitive_fields() {
    let cache = tempfile::tempdir().unwrap();
    let candidate = candidate("2.1.0", "private-signature-material");
    assert!(!format!("{candidate:?}").contains("trusted:"));
    let mut repository =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    repository
        .stage(candidate, "private-signature-material".as_bytes())
        .unwrap();
    let captured = Arc::new(Mutex::new(String::new()));
    repository
        .install_with(
            &DebugCaptureInstaller(captured.clone()),
            InstallIntent::UpdateRestart,
        )
        .unwrap();
    let verified = captured.lock().unwrap().clone();
    assert!(!verified.contains(&cache.path().to_string_lossy().to_string()));
    assert!(verified.contains("[redacted]"));
}
