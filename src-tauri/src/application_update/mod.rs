mod ports;
mod preference;
mod staging;
mod state;

pub(crate) use ports::{
    Clock, InstallError, InstallIntent, InstallOutcome, StableReleaseProvider, UpdateInstaller,
};
pub(crate) use preference::{FilePreferenceStore, PreferenceError, PreferenceStore};
pub(crate) use staging::{
    InstallKind, MinisignVerifier, StageCandidate, StagingContext, StagingRepository,
    VerifiedStagedPackage,
};
#[allow(
    unused_imports,
    reason = "Task 3 的 adapter 将消费暂存结果与稳定错误分类"
)]
pub(crate) use staging::{StagedUpdate, StagingError};
#[cfg(test)]
pub(crate) use staging::{StagingCommitPoint, StagingFaultInjector};
pub(crate) use state::{
    DownloadProgress, ReleasePackage, StableRelease, UpdateController, UpdateEffect, UpdateFailure,
    UpdateFailureCategory, UpdateInput,
};
#[allow(
    unused_imports,
    reason = "Task 3 的 runtime 与 typed command 将消费这些纯数据契约"
)]
pub(crate) use state::{
    UpdateCandidate, UpdateDomainError, UpdateFailureSnapshot, UpdateOperation, UpdateOperationId,
    UpdateOutput, UpdateSnapshot, UpdateState,
};

const MAX_RELEASE_NOTES_BYTES: usize = 32 * 1024;

/// 将 Release Notes 收敛到可持久化和跨边界投影的 UTF-8 字节上限。
fn bounded_release_notes(notes: &str) -> String {
    if notes.len() <= MAX_RELEASE_NOTES_BYTES {
        return notes.to_owned();
    }
    let mut boundary = MAX_RELEASE_NOTES_BYTES;
    while !notes.is_char_boundary(boundary) {
        boundary -= 1;
    }
    notes[..boundary].to_owned()
}

/// 在 Task 3 接线前仅类型检查生产组合将消费的公开入口，不执行任何运行时工作。
#[cfg(not(test))]
#[allow(dead_code, reason = "Task 3 接线边界的唯一临时锚点；接线后应整体删除")]
fn task3_wiring_contract<C, P, V, R, I>(
    cache_root: &std::path::Path,
    clock: C,
    preferences: P,
    verifier: V,
    provider: &R,
    installer: &I,
) where
    C: Clock,
    P: PreferenceStore,
    V: MinisignVerifier,
    R: StableReleaseProvider,
    I: UpdateInstaller,
{
    let _ = clock.now();
    let _ = provider.check_stable();

    let mut file_preferences =
        FilePreferenceStore::new(cache_root.join("application-update-preference.json"));
    let _ = file_preferences.load_automatic_download();
    let _ = file_preferences.save_automatic_download(true);

    let package = ReleasePackage::new(
        "https://example.invalid/package",
        "signature",
        "darwin-aarch64",
        InstallKind::MacosAppArchive,
    )
    .expect("静态 Task 3 接线示例必须满足领域约束");
    let release = StableRelease::new("2.0.0", "notes", package);
    let _ = (
        release.version(),
        release.release_notes(),
        release.candidate(),
        release.package().download_url(),
        release.package().signature(),
        release.package().target(),
        release.package().install_kind(),
    );

    if let Ok(mut controller) = UpdateController::new("1.0.0", clock, preferences) {
        let _ = controller.snapshot();
        if let Ok(output) = controller.handle(UpdateInput::Ready) {
            for effect in output.effects {
                if let UpdateEffect::CheckStable { operation_id } = effect {
                    let _ = operation_id.value();
                    let failure =
                        UpdateFailure::new(UpdateFailureCategory::Network, "private diagnostic");
                    let _ = vec![
                        UpdateInput::RecoverStaged {
                            version: "2.0.0".into(),
                            release_notes: "notes".into(),
                        },
                        UpdateInput::PeriodicCheckDue,
                        UpdateInput::ManualCheck,
                        UpdateInput::CheckSucceeded {
                            operation_id,
                            release: Some(release.clone()),
                        },
                        UpdateInput::CheckFailed {
                            operation_id,
                            failure: failure.clone(),
                        },
                        UpdateInput::SetAutomaticDownload(false),
                        UpdateInput::DownloadStarted { operation_id },
                        UpdateInput::DownloadAdvanced {
                            operation_id,
                            progress: DownloadProgress::KnownTotal {
                                downloaded_bytes: 1,
                                total_bytes: 2,
                            },
                        },
                        UpdateInput::DownloadSucceeded { operation_id },
                        UpdateInput::DownloadFailed {
                            operation_id,
                            failure,
                        },
                        UpdateInput::UserRetry,
                        UpdateInput::RetryDue { token: 1 },
                        UpdateInput::RequestUpdateRestart,
                    ];
                }
            }
        }
    }

    let _ = [InstallIntent::NormalExit, InstallIntent::UpdateRestart];
    let _ = [InstallOutcome::Installed, InstallOutcome::HandedOff];
    let install_error = InstallError::new("safe message");
    let _ = install_error.message();
    let _ = [
        UpdateFailureCategory::Network,
        UpdateFailureCategory::RequestTimeout,
        UpdateFailureCategory::RateLimited,
        UpdateFailureCategory::Server,
        UpdateFailureCategory::Manifest,
        UpdateFailureCategory::Verification,
        UpdateFailureCategory::Storage,
        UpdateFailureCategory::Installer,
    ];
    let diagnostic = UpdateFailure::from_http_status(503, "private diagnostic");
    let _ = (
        diagnostic.category(),
        diagnostic.diagnostic(),
        diagnostic.retryable(),
    );

    let context = StagingContext::new("1.0.0", "darwin-aarch64", InstallKind::MacosAppArchive)
        .expect("静态 Task 3 接线示例必须满足 staging 约束");
    let candidate = StageCandidate::new(
        "2.0.0",
        "signature",
        "darwin-aarch64",
        InstallKind::MacosAppArchive,
        "notes",
    )
    .expect("静态 Task 3 接线示例必须满足 staging 约束");
    if let Ok(mut repository) = StagingRepository::open(cache_root, context, verifier) {
        let _ = repository.snapshot();
        let _ = repository.recovery_failure();
        if let Ok(staged) = repository.stage(candidate, std::io::empty()) {
            let _ = (staged.version(), staged.release_notes());
        }
        let _ = repository.update_release_notes("2.0.0", "notes");
        if let Ok(verified) = repository.verify_for_install() {
            let _ = (
                verified.version(),
                verified.path(),
                verified.target(),
                verified.install_kind(),
            );
            let _ = installer.install(&verified, InstallIntent::UpdateRestart);
            let _ = repository.complete_install(&verified);
        }
        let _ = repository.reconcile_current_version("2.0.0");
    }
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
