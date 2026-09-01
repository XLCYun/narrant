use crate::application_update::{
    InstallKind, MinisignVerifier, StageCandidate, StagingCommitPoint, StagingContext,
    StagingError, StagingFaultInjector, StagingRepository,
};
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

const TARGET: &str = "darwin-aarch64";

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
    let verified = repository.verify_for_install().unwrap();
    assert_eq!(verified.version(), "2.2.0");
    assert_eq!(fs::read(verified.path()).unwrap(), b"package-v3");
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
    assert_eq!(repository.verify_for_install().unwrap().version(), "2.1.0");
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

/// pointer 原子切换后的失败以新 generation 为权威，不会回退到旧包。
#[test]
fn post_pointer_failure_recovers_new_generation() {
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
        assert_eq!(
            repository.stage(candidate("2.2.0", "new"), "new".as_bytes()),
            Err(StagingError::StorageFailed)
        );
        assert_eq!(repository.snapshot().unwrap().version(), "2.2.0");
        drop(repository);
        let recovered =
            StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier)
                .unwrap();
        assert_eq!(recovered.snapshot().unwrap().version(), "2.2.0");
        assert_eq!(generation_count(cache.path()), 1);
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
            repository.verify_for_install(),
            Err(StagingError::VerificationFailed | StagingError::TargetMismatch)
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

/// 安装完成、版本 reconciliation 和无 pointer 的不完整 generation 都会同步清理。
#[test]
fn completion_reconciliation_and_incomplete_recovery_cleanup() {
    let cache = tempfile::tempdir().unwrap();
    let mut repository =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    repository
        .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
        .unwrap();
    let verified = repository.verify_for_install().unwrap();
    assert_eq!(verified.target(), TARGET);
    assert_eq!(verified.install_kind(), InstallKind::MacosAppArchive);
    repository.complete_install(&verified).unwrap();
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
    let verified = format!("{:?}", repository.verify_for_install().unwrap());
    assert!(!verified.contains(&cache.path().to_string_lossy().to_string()));
    assert!(verified.contains("[redacted]"));
}
