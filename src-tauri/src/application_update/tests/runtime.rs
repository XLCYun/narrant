use super::*;
use base64::Engine;
use std::io::Cursor;

const PUBLIC_KEY: &str = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
const SIGNATURE: &str = "untrusted comment: signature from minisign secret key\nRUQf6LRCGA9i559r3g7V1qNyJDApGip8MfqcadIgT9CuhV3EMhHoN1mGTkUidF/z7SrlQgXdy8ofjb7bNJJylDOocrCo8KLzZwo=\ntrusted comment: timestamp:1556193335\tfile:test\ny/rUw2y8/hOUYjZU71eHp/Wo1KZ40fGy2VJEDl34XMJM+TX48Ss/17u3IvIfbVR1FkZZSNCisQbuQY+bHwhEBg==";

fn encode(value: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(value)
}

fn trusted_config() -> Result<TrustedUpdaterConfig, super::super::config::UpdaterConfigError> {
    TrustedUpdaterConfig::parse(
        Some(super::super::config::PRODUCTION_UPDATE_ENDPOINT),
        Some(&encode(PUBLIC_KEY)),
    )
}

#[test]
fn updater_registry_identity_is_namespaced_and_bounded() {
    let _ = managed_operation_id("download", 42);
}

#[test]
fn startup_distinguishes_recoverable_discard_from_unavailable_repository() {
    let temporary = tempfile::tempdir().expect("cache");
    let target = super::super::platform::MACOS_TARGET;
    let context =
        StagingContext::new("1.0.0", target, super::super::InstallKind::MacosAppArchive).unwrap();
    let verifier = DiskMinisignVerifier::new(&encode(PUBLIC_KEY)).unwrap();
    let mut repository = StagingRepository::open(temporary.path(), context, verifier).unwrap();
    let candidate = StageCandidate::new(
        "2.0.0",
        encode(SIGNATURE),
        target,
        super::super::InstallKind::MacosAppArchive,
        "notes",
    )
    .unwrap();
    repository.stage(candidate, Cursor::new(b"test")).unwrap();
    drop(repository);
    let staging_root = temporary.path().join("application-update");
    let generation = std::fs::read_to_string(staging_root.join("current")).unwrap();
    std::fs::write(
        staging_root.join(generation.trim()).join("package.bin"),
        b"tampered",
    )
    .unwrap();

    let (available, recovery_failure) =
        initialize_staging(temporary.path(), "1.0.0", &trusted_config());
    assert!(available.is_some());
    assert_eq!(
        recovery_failure.unwrap().category(),
        UpdateFailureCategory::Verification
    );

    let unavailable_root = temporary.path().join("not-a-directory");
    std::fs::write(&unavailable_root, b"file").unwrap();
    let (available, recovery_failure) =
        initialize_staging(&unavailable_root, "1.0.0", &trusted_config());
    assert!(available.is_none());
    assert_eq!(
        recovery_failure.unwrap().category(),
        UpdateFailureCategory::Storage
    );
}
