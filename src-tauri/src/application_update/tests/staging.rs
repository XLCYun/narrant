use crate::application_update::{
    Clock, InstallAttemptError, InstallError, InstallIntent, InstallKind, InstallOutcome,
    MinisignVerifier, PreferenceError, PreferenceStore, ReleasePackage, StableRelease,
    StageCandidate, StageCommitOutcome, StagingCommitPoint, StagingContext, StagingError,
    StagingFaultInjector, StagingRepository, UpdateController, UpdateEffect, UpdateInput,
    UpdateInstaller, UpdateState, VerifiedStagedPackage,
};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
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
        InstallOutcome::HandedOff
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
        InstallOutcome::HandedOff
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

/// pointer 后 outcome 与新 generation snapshot 保持一致，不会误报普通失败。
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
        if point == StagingCommitPoint::AfterPointerCommit {
            assert!(matches!(&outcome, StageCommitOutcome::DurabilityUnknown(_)));
            assert!(!outcome.is_durable());
        } else {
            assert!(matches!(&outcome, StageCommitOutcome::Committed(_)));
            assert!(outcome.is_durable());
        }
        assert_eq!(outcome.staged().version(), "2.2.0");
        assert_eq!(repository.snapshot().unwrap().version(), "2.2.0");
        drop(repository);
        let recovered =
            StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier)
                .unwrap();
        assert_eq!(recovered.snapshot().unwrap().version(), "2.2.0");
        assert_eq!(generation_count(cache.path()), 1);
    }
}

/// 组合层把 pointer 后显式 outcome 映射为成功，Controller 与仓储不会版本分叉。
#[test]
fn repository_outcome_keeps_controller_snapshot_consistent() {
    let cache = tempfile::tempdir().unwrap();
    let mut initial =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    initial
        .stage(candidate("2.1.0", "old"), "old".as_bytes())
        .unwrap();
    drop(initial);

    let mut controller = UpdateController::new("2.0.15", FixedClock, EnabledPreferences).unwrap();
    controller
        .handle(UpdateInput::RecoverStaged {
            version: "2.1.0".into(),
            release_notes: "旧说明".into(),
        })
        .unwrap();
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
            release: Some(stable_release("2.2.0", "new")),
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
        .stage(candidate("2.2.0", "new"), "new".as_bytes())
        .unwrap();
    assert!(matches!(outcome, StageCommitOutcome::DurabilityUnknown(_)));
    let authoritative = repository.refresh_snapshot().unwrap().unwrap();
    assert_eq!(authoritative.version(), "2.2.0");
    let published = controller
        .handle(UpdateInput::DownloadSucceeded {
            operation_id: download_id,
        })
        .unwrap();
    assert_eq!(published.snapshot.state, UpdateState::Staged);
    assert_eq!(published.snapshot.candidate.unwrap().version, "2.2.0");
    assert_eq!(repository.snapshot().unwrap().version(), "2.2.0");
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

    let (stage_done_tx, stage_done_rx) = mpsc::sync_channel(0);
    let stage_thread = std::thread::spawn(move || {
        let result = competing.stage(candidate("2.2.0", "package-v3"), "package-v3".as_bytes());
        stage_done_tx.send(result).unwrap();
    });
    assert!(matches!(
        stage_done_rx.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));

    proceed_tx.send(()).unwrap();
    assert_eq!(install_thread.join().unwrap().unwrap(), outcome);
    let staged = stage_done_rx
        .recv_timeout(Duration::from_secs(2))
        .unwrap()
        .unwrap();
    assert_eq!(staged.staged().version(), "2.2.0");
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
        InstallOutcome::Installed
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
