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

/// 读取 fixture 记录的子进程 pid，供候选切换的回收断言使用。
fn fixture_pid(events: &std::path::Path) -> u32 {
    std::fs::read_to_string(events)
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("process-pid:"))
        .unwrap()
        .parse()
        .unwrap()
}

/// 查询进程是否仍存在；已 wait 的候选必须返回 false。
fn process_exists(pid: u32) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[test]
fn file_settings_round_trip_preserves_unified_application_settings() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    let executable = version_fixture(directory.path());
    let settings = ApplicationCodexSettings {
        application: ApplicationSettingsApplication::new(
            path.clone(),
            directory.path().join("legacy.json"),
        ),
    };
    std::fs::write(
        &path,
        r#"{"version":1,"theme":"dark","codexExecutable":null,"babysitModel":{"mode":"automatic"}}"#,
    )
    .unwrap();

    settings.save(executable.clone()).unwrap();

    assert_eq!(settings.load(), Some(executable));
    let serialized = std::fs::read_to_string(path).unwrap();
    assert!(serialized.contains(r#""version":1"#));
    assert!(serialized.contains(r#""theme":"dark""#));
}

#[test]
fn malformed_or_relative_settings_are_ignored() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    std::fs::write(&path, r#"{"codexExecutable":"relative/codex"}"#).unwrap();

    assert!(ApplicationCodexSettings {
        application: ApplicationSettingsApplication::new(
            path,
            directory.path().join("legacy.json"),
        ),
    }
    .load()
    .is_none());
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

    assert!(std::fs::read_to_string(&rejected_events)
        .unwrap()
        .lines()
        .any(|line| line == "turn/start:sentinel"));
    assert!(!process_exists(fixture_pid(&rejected_events)));
    assert!(std::fs::read_to_string(&accepted_events)
        .unwrap()
        .lines()
        .any(|line| line == "thread/delete"));
    runtime.shutdown().await;
}

#[tokio::test]
async fn signed_out_environment_candidate_does_not_shadow_an_authenticated_candidate() {
    let directory = tempfile::tempdir().unwrap();
    let signed_out_events = directory.path().join("signed-out.log");
    let accepted_events = directory.path().join("accepted.log");
    let signed_out = app_server_candidate(
        directory.path(),
        "codex-signed-out",
        "auth-fail",
        &signed_out_events,
    );
    let accepted = app_server_candidate(
        directory.path(),
        "codex-authenticated",
        "complete",
        &accepted_events,
    );
    let runtime = CodexRuntime::new_with_discovery(
        Arc::new(CodexAppServerProvider::new("codex")),
        directory.path().join("babysit-settings.json"),
        CodexExecutableDiscovery::with_candidates(vec![signed_out, accepted]),
    );

    runtime
        .ensure_ready(std::path::Path::new("/tmp/narrant-review-workspace"))
        .await
        .unwrap();

    assert!(std::fs::read_to_string(signed_out_events)
        .unwrap()
        .lines()
        .any(|line| line == "account/read"));
    assert!(std::fs::read_to_string(accepted_events)
        .unwrap()
        .lines()
        .any(|line| line == "thread/delete"));
    runtime.shutdown().await;
}
