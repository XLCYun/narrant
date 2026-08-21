use super::*;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

/// 为设置测试构造确定性的可执行文件。
fn version_fixture(directory: &std::path::Path) -> std::path::PathBuf {
    let path = directory.join("codex-selected");
    std::fs::write(&path, "#!/bin/sh\nprintf 'codex-test 0.0.0\\n'\n").unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&path, permissions).unwrap();
    path
}

#[test]
fn file_settings_round_trip_only_the_executable_path() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("babysit-settings.json");
    let executable = version_fixture(directory.path());
    let settings = FileCodexSettings { path: path.clone() };

    settings.save(executable.clone()).unwrap();

    assert_eq!(settings.load(), Some(executable));
    let serialized = std::fs::read_to_string(path).unwrap();
    assert!(!serialized.contains("version"));
}

#[test]
fn malformed_or_relative_settings_are_ignored() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("babysit-settings.json");
    std::fs::write(&path, r#"{"executable":"relative/codex"}"#).unwrap();

    assert!(FileCodexSettings { path }.load().is_none());
}

#[test]
fn runtime_construction_does_not_probe_or_create_settings() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("babysit-settings.json");
    let runtime = CodexRuntime::new(
        Arc::new(CodexAppServerProvider::new("codex")),
        settings_path.clone(),
    );

    assert_eq!(runtime.status(), CodexRuntimeStatus::Unknown);
    assert!(!settings_path.exists());
}

#[tokio::test]
async fn picker_validates_and_persists_the_canonical_executable() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("babysit-settings.json");
    let executable = version_fixture(directory.path());
    let runtime = CodexRuntime::new(
        Arc::new(CodexAppServerProvider::new("codex")),
        settings_path,
    );

    runtime.select_executable(&executable).await.unwrap();

    assert_eq!(
        runtime.settings.load(),
        Some(executable.canonicalize().unwrap())
    );
    assert_eq!(runtime.status(), CodexRuntimeStatus::Unknown);
}

#[test]
fn runtime_maps_setup_failures_to_typed_states() {
    assert_eq!(
        status_for_error(&BabysitError::CodexNotFound),
        CodexRuntimeStatus::Missing
    );
    assert_eq!(
        status_for_error(&BabysitError::CodexSignedOut),
        CodexRuntimeStatus::SignedOut
    );
    assert_eq!(
        status_for_error(&BabysitError::CodexIncompatible),
        CodexRuntimeStatus::Incompatible
    );
    assert_eq!(
        status_for_error(&BabysitError::Provider {
            detail: "transport".to_owned(),
        }),
        CodexRuntimeStatus::InitFailed
    );
}
