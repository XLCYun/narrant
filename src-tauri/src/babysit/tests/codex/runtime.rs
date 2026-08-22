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

/// 构造同时支持 `--version` 与 `app-server` 的候选 wrapper。
fn app_server_candidate(
    directory: &std::path::Path,
    name: &str,
    mode: &str,
    events: &std::path::Path,
) -> std::path::PathBuf {
    let path = directory.join(name);
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/babysit/tests/fixtures/fake_app_server.sh");
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = \"--version\" ]; then printf 'codex-test 0.0.0\\n'; exit 0; fi\nif [ \"$1\" = \"app-server\" ]; then exec sh '{}' '{}' '{}'; fi\nexit 2\n",
        fixture.display(),
        mode,
        events.display(),
    );
    std::fs::write(&path, script).unwrap();
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

#[tokio::test]
async fn incompatible_environment_candidate_falls_back_to_the_next_executable() {
    let directory = tempfile::tempdir().unwrap();
    let rejected_events = directory.path().join("rejected.log");
    let accepted_events = directory.path().join("accepted.log");
    let rejected = app_server_candidate(
        directory.path(),
        "codex-rejected",
        "rejected-output-schema",
        &rejected_events,
    );
    let accepted = app_server_candidate(
        directory.path(),
        "codex-accepted",
        "complete",
        &accepted_events,
    );
    let runtime = CodexRuntime::new_with_discovery(
        Arc::new(CodexAppServerProvider::new("codex")),
        directory.path().join("babysit-settings.json"),
        CodexExecutableDiscovery::with_candidates(vec![rejected, accepted]),
    );

    runtime
        .ensure_ready(std::path::Path::new("/tmp/narrant-review-workspace"))
        .await
        .unwrap();

    assert!(std::fs::read_to_string(rejected_events)
        .unwrap()
        .lines()
        .any(|line| line == "turn/start:sentinel"));
    assert!(std::fs::read_to_string(accepted_events)
        .unwrap()
        .lines()
        .any(|line| line == "thread/delete"));
    runtime.shutdown().await;
}
