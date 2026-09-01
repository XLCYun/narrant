use crate::application_update::{
    InstallError, InstallIntent, InstallKind, InstallOutcome, MinisignVerifier, ReleasePackage,
    StableRelease, StableReleaseProvider, StageCandidate, StagingContext, StagingRepository,
    UpdateFailure, UpdateFailureCategory, UpdateInstaller,
};
use async_trait::async_trait;
use std::fs;
use std::path::Path;

/// 返回固定 Stable 元数据的注入式网络端口。
struct FixedProvider;

#[async_trait]
impl StableReleaseProvider for FixedProvider {
    /// 返回不访问网络的确定性候选。
    async fn check_stable(&self) -> Result<Option<StableRelease>, UpdateFailure> {
        Ok(Some(StableRelease::new(
            "2.1.0",
            "notes",
            ReleasePackage::new(
                "https://example.test/package",
                "trusted:package",
                "darwin-aarch64",
                InstallKind::MacosAppArchive,
            )
            .unwrap(),
        )))
    }
}

/// 读取固定内容的确定性 minisign 端口。
struct FixedVerifier;

impl MinisignVerifier for FixedVerifier {
    /// 仅接受测试包和签名的精确绑定。
    fn verify(&self, package: &Path, signature: &str) -> Result<(), String> {
        if fs::read(package).map_err(|error| error.to_string())? == b"package"
            && signature == "trusted:package"
        {
            Ok(())
        } else {
            Err("untrusted".into())
        }
    }
}

/// 验证安装器只收到复验句柄和窄退出意图的注入式端口。
struct FixedInstaller;

impl UpdateInstaller for FixedInstaller {
    /// 确认 Update Restart 意图和已复验版本。
    fn install(
        &self,
        package: &crate::application_update::VerifiedStagedPackage,
        intent: InstallIntent,
    ) -> Result<InstallOutcome, InstallError> {
        if package.version() == "2.1.0" && intent == InstallIntent::UpdateRestart {
            Ok(InstallOutcome::Installed)
        } else {
            Err(InstallError::new("unexpected install request"))
        }
    }
}

/// 网络元数据、验证器和安装器均可由框架外测试替换。
#[tokio::test]
async fn external_ports_are_injected_and_deterministic() {
    let release = FixedProvider.check_stable().await.unwrap().unwrap();
    assert_eq!(release.version(), "2.1.0");
    assert_eq!(
        release.package().download_url(),
        "https://example.test/package"
    );
    assert_eq!(release.package().signature(), "trusted:package");
    assert_eq!(release.package().target(), "darwin-aarch64");
    assert_eq!(
        release.package().install_kind(),
        InstallKind::MacosAppArchive
    );

    let cache = tempfile::tempdir().unwrap();
    let context =
        StagingContext::new("2.0.15", "darwin-aarch64", InstallKind::MacosAppArchive).unwrap();
    let mut staging = StagingRepository::open(cache.path(), context, FixedVerifier).unwrap();
    staging
        .stage(
            StageCandidate::new(
                release.version(),
                release.package().signature(),
                release.package().target(),
                release.package().install_kind(),
                release.release_notes(),
            )
            .unwrap(),
            "package".as_bytes(),
        )
        .unwrap();
    assert_eq!(
        staging.install_with(&FixedInstaller, InstallIntent::UpdateRestart),
        Ok(InstallOutcome::Installed)
    );
}

/// 安装错误和不可重试验证类别保持脱敏、稳定。
#[test]
fn port_errors_are_stable_and_display_safe() {
    let error = InstallError::new("installer rejected package");
    assert_eq!(error.message(), "installer rejected package");
    assert_eq!(error.to_string(), "installer rejected package");
    let failure = UpdateFailure::new(UpdateFailureCategory::Installer, error.to_string());
    assert!(!failure.retryable());
    assert_eq!(failure.category(), UpdateFailureCategory::Installer);
    assert_eq!(InstallIntent::NormalExit, InstallIntent::NormalExit);
    assert_eq!(InstallOutcome::HandedOff, InstallOutcome::HandedOff);
}
