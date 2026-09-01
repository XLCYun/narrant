use crate::application_update::{
    InstallKind, MinisignVerifier, StageCandidate, StagingContext, StagingError, StagingRepository,
};
use std::fs;
use std::io::{self, Read};
use std::path::Path;

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

/// 创建与当前 macOS ARM 测试目标绑定的恢复上下文。
fn context(current_version: &str) -> StagingContext {
    StagingContext::new(current_version, TARGET, InstallKind::MacosAppArchive).unwrap()
}

/// 创建绑定目标和确定性签名的暂存候选。
fn candidate(version: &str, content: &str) -> StageCandidate {
    StageCandidate::new(
        version,
        format!("trusted:{content}"),
        TARGET,
        InstallKind::MacosAppArchive,
    )
    .unwrap()
}

/// 验证已暂存更新只留下一个包和一个元数据文件并可跨进程恢复。
#[test]
fn staged_update_is_durable_and_has_one_canonical_package() {
    let cache = tempfile::tempdir().unwrap();
    let mut repository =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    repository
        .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
        .unwrap();
    drop(repository);

    let root = cache.path().join("application-update");
    let mut names = fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, vec!["metadata.json", "package.bin"]);
    let recovered =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    assert_eq!(recovered.snapshot().unwrap().version(), "2.1.0");
    assert_eq!(recovered.recovery_failure(), None);
}

/// 更高候选原子替换旧包，同版、旧版与运行版本均被拒绝。
#[test]
fn only_newer_candidate_replaces_staging() {
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
    let verified = repository.verify_for_install().unwrap();
    assert_eq!(verified.version(), "2.2.0");
    assert_eq!(fs::read(verified.path()).unwrap(), b"package-v3");
}

/// 状态机请求更高候选替换时，磁盘仓储只清理真正更旧的 staging。
#[test]
fn discard_only_removes_staging_for_higher_replacement() {
    let cache = tempfile::tempdir().unwrap();
    let mut repository =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    repository
        .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
        .unwrap();
    assert!(!repository.discard_if_older_than("2.1.0").unwrap());
    assert!(repository.snapshot().is_some());
    assert!(repository.discard_if_older_than("2.2.0").unwrap());
    assert_eq!(repository.snapshot(), None);
}

/// 中断流只写临时文件，不能破坏已有 staging 或留下可恢复部分包。
#[test]
fn interrupted_stream_preserves_existing_staging_and_removes_temporary_file() {
    let cache = tempfile::tempdir().unwrap();
    let mut repository =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    repository
        .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
        .unwrap();
    assert_eq!(
        repository.stage(
            candidate("2.2.0", "partial"),
            InterruptedReader { emitted: false }
        ),
        Err(StagingError::StorageFailed)
    );
    assert_eq!(repository.snapshot().unwrap().version(), "2.1.0");
    drop(repository);
    let recovered =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    assert_eq!(recovered.snapshot().unwrap().version(), "2.1.0");
    assert!(!cache
        .path()
        .join("application-update/.package.bin.tmp")
        .exists());
}

/// 下载完成后的签名失败不会晋级候选或覆盖已有可信包。
#[test]
fn verification_failure_never_persists_candidate() {
    let cache = tempfile::tempdir().unwrap();
    let mut repository =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    assert_eq!(
        repository.stage(candidate("2.1.0", "different"), "untrusted".as_bytes()),
        Err(StagingError::VerificationFailed)
    );
    assert_eq!(repository.snapshot(), None);
}

/// 安装前会重读元数据并重验包，篡改任一文件都会清除信任状态。
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
        let root = cache.path().join("application-update");
        if tamper_metadata {
            let path = root.join("metadata.json");
            let mut document: serde_json::Value =
                serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            document["target"] = serde_json::Value::String("windows-x86_64".into());
            fs::write(path, serde_json::to_vec(&document).unwrap()).unwrap();
        } else {
            fs::write(root.join("package.bin"), b"tampered").unwrap();
        }
        assert!(matches!(
            repository.verify_for_install(),
            Err(StagingError::VerificationFailed | StagingError::TargetMismatch)
        ));
        assert_eq!(repository.snapshot(), None);
    }
}

/// 进程恢复会拒绝被篡改包并给出稳定验证失败诊断。
#[test]
fn recovery_discards_tampered_package() {
    let cache = tempfile::tempdir().unwrap();
    let mut repository =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    repository
        .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
        .unwrap();
    fs::write(
        cache.path().join("application-update/package.bin"),
        b"tampered",
    )
    .unwrap();
    drop(repository);
    let recovered =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    assert_eq!(recovered.snapshot(), None);
    assert_eq!(
        recovered.recovery_failure(),
        Some(StagingError::VerificationFailed)
    );
}

/// 不同 manifest target 或安装类型不能恢复或写入当前平台 staging。
#[test]
fn target_and_install_kind_mismatch_are_rejected() {
    let cache = tempfile::tempdir().unwrap();
    let mut repository =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    let wrong = StageCandidate::new(
        "2.1.0",
        "trusted:package-v2",
        "windows-x86_64",
        InstallKind::WindowsNsis,
    )
    .unwrap();
    assert_eq!(
        repository.stage(wrong, "package-v2".as_bytes()),
        Err(StagingError::TargetMismatch)
    );
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

/// 预发布版本不是 Stable 候选，不能进入持久化暂存。
#[test]
fn prerelease_candidate_is_rejected() {
    assert!(matches!(
        StageCandidate::new(
            "2.1.0-beta.1",
            "trusted:package",
            TARGET,
            InstallKind::MacosAppArchive,
        ),
        Err(StagingError::InvalidCandidate)
    ));
}

/// 新进程运行版本达到或超过 staging 版本时会在恢复阶段清理旧包。
#[test]
fn recovery_cleans_package_after_running_version_reaches_staging() {
    let cache = tempfile::tempdir().unwrap();
    let mut repository =
        StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier).unwrap();
    repository
        .stage(candidate("2.1.0", "package-v2"), "package-v2".as_bytes())
        .unwrap();
    drop(repository);
    let recovered =
        StagingRepository::open(cache.path(), context("2.1.0"), DeterministicVerifier).unwrap();
    assert_eq!(recovered.snapshot(), None);
    assert_eq!(recovered.recovery_failure(), None);
    assert!(!cache.path().join("application-update/package.bin").exists());
}

/// 安装成功与 installer handoff 后的新版本确认都会清理 staging。
#[test]
fn installation_completion_and_version_reconciliation_cleanup() {
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
}

/// 恢复会清理只有包或只有元数据的崩溃中间态。
#[test]
fn recovery_cleans_incomplete_commit() {
    for file in ["package.bin", "metadata.json"] {
        let cache = tempfile::tempdir().unwrap();
        let root = cache.path().join("application-update");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join(file), b"partial").unwrap();
        let recovered =
            StagingRepository::open(cache.path(), context("2.0.15"), DeterministicVerifier)
                .unwrap();
        assert_eq!(recovered.snapshot(), None);
        assert_eq!(
            recovered.recovery_failure(),
            Some(StagingError::StorageFailed)
        );
        assert_eq!(fs::read_dir(root).unwrap().count(), 0);
    }
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
    let staged = format!("{:?}", repository.snapshot().unwrap());
    let verified = format!("{:?}", repository.verify_for_install().unwrap());
    assert!(!staged.contains(&cache.path().to_string_lossy().to_string()));
    assert!(!verified.contains(&cache.path().to_string_lossy().to_string()));
    assert!(verified.contains("[redacted]"));
}
