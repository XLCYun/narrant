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

#[tokio::test]
async fn file_settings_round_trip_preserves_unified_application_settings() {
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

    settings.save(Some(executable.clone())).await.unwrap();

    assert_eq!(settings.load().unwrap(), Some(executable));
    let serialized = std::fs::read_to_string(path).unwrap();
    assert!(serialized.contains(r#""version":1"#));
    assert!(serialized.contains(r#""theme":"dark""#));
}

#[test]
fn malformed_or_relative_settings_fail_instead_of_falling_back() {
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
    .is_err());
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
async fn model_catalog_uses_explicit_executable_before_automatic_candidates() {
    let directory = tempfile::tempdir().unwrap();
    let selected_events = directory.path().join("selected-events");
    let fallback_events = directory.path().join("fallback-events");
    let selected = app_server_candidate(
        directory.path(),
        "selected-codex",
        "complete",
        &selected_events,
    );
    let fallback = app_server_candidate(
        directory.path(),
        "fallback-codex",
        "complete",
        &fallback_events,
    );
    let settings_path = directory.path().join("settings.json");
    let provider = Arc::new(CodexAppServerProvider::new("codex"));
    let runtime = CodexRuntime::new_with_discovery(
        provider,
        settings_path,
        CodexExecutableDiscovery::with_candidates(vec![fallback]),
    );
    runtime.select_executable(&selected).await.unwrap();

    let catalog = runtime.model_catalog().await.unwrap();

    assert_eq!(catalog.models[0].id, "fixture-model");
    assert!(std::fs::read_to_string(selected_events)
        .unwrap()
        .contains("model/list"));
    assert!(!fallback_events.exists());
}

#[tokio::test]
async fn model_catalog_process_is_reused_when_babysit_mode_starts() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let executable = app_server_candidate(directory.path(), "catalog-codex", "complete", &events);
    let runtime = CodexRuntime::new_with_discovery(
        Arc::new(CodexAppServerProvider::new("codex")),
        directory.path().join("settings.json"),
        CodexExecutableDiscovery::with_candidates(vec![executable]),
    );

    runtime.model_catalog().await.unwrap();
    runtime
        .ensure_ready(std::path::Path::new("/tmp/narrant-review-workspace"))
        .await
        .unwrap();

    let observed = std::fs::read_to_string(events).unwrap();
    assert!(observed.contains("model/list"));
    assert!(observed.contains("thread/list"));
    runtime.shutdown().await;
}

#[tokio::test]
async fn babysit_process_is_reused_when_model_catalog_opens_after_analysis() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let executable = app_server_candidate(directory.path(), "analysis-codex", "complete", &events);
    let runtime = CodexRuntime::new_with_discovery(
        Arc::new(CodexAppServerProvider::new("codex")),
        directory.path().join("settings.json"),
        CodexExecutableDiscovery::with_candidates(vec![executable]),
    );

    runtime
        .ensure_ready(std::path::Path::new("/tmp/narrant-review-workspace"))
        .await
        .unwrap();
    runtime.model_catalog().await.unwrap();

    let observed = std::fs::read_to_string(events).unwrap();
    assert!(observed.contains("thread/list"));
    assert!(observed.contains("model/list"));
    runtime.shutdown().await;
}

#[tokio::test]
async fn capability_resolves_saved_model_through_the_same_app_server() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let executable = app_server_candidate(directory.path(), "model-codex", "complete", &events);
    let runtime = CodexRuntime::new_with_discovery(
        Arc::new(CodexAppServerProvider::new("codex")),
        directory.path().join("settings.json"),
        CodexExecutableDiscovery::with_candidates(vec![executable]),
    );
    runtime
        .settings
        .application
        .update_babysit_model(
            crate::application_settings::BabysitModelPreference::Explicit {
                model: "fixture-model".to_owned(),
                reasoning_effort: Some("low".to_owned()),
            },
        )
        .unwrap();

    let (resolved, analysis) = runtime
        .prepare_analysis(std::path::Path::new("/tmp/narrant-review-workspace"))
        .await
        .unwrap();

    assert_eq!(resolved.model, "gpt-5.6-luna");
    assert_eq!(resolved.reasoning_effort, "low");
    drop(analysis);
    runtime.shutdown().await;
}

#[tokio::test]
async fn restarted_process_is_probed_before_the_next_analysis() {
    let directory = tempfile::tempdir().unwrap();
    let events = directory.path().join("events");
    let executable = app_server_candidate(directory.path(), "restart-codex", "complete", &events);
    let provider = Arc::new(CodexAppServerProvider::new("codex"));
    let runtime = CodexRuntime::new_with_discovery(
        provider.clone(),
        directory.path().join("settings.json"),
        CodexExecutableDiscovery::with_candidates(vec![executable]),
    );

    let (_, first_analysis) = runtime
        .prepare_analysis(std::path::Path::new("/tmp/narrant-review-workspace"))
        .await
        .unwrap();
    drop(first_analysis);
    let first_generation = provider.process_generation();
    provider.shutdown().await;
    let (_, second_analysis) = runtime
        .prepare_analysis(std::path::Path::new("/tmp/narrant-review-workspace"))
        .await
        .unwrap();
    drop(second_analysis);

    assert_ne!(provider.process_generation(), first_generation);
    assert_eq!(
        std::fs::read_to_string(events)
            .unwrap()
            .lines()
            .filter(|event| *event == "thread/list")
            .count(),
        2
    );
    runtime.shutdown().await;
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
        runtime.settings.load().unwrap(),
        Some(executable.canonicalize().unwrap())
    );
    assert_eq!(runtime.status(), CodexRuntimeStatus::Unknown);
}

#[tokio::test]
async fn explicit_executable_is_the_only_provider_candidate() {
    let directory = tempfile::tempdir().unwrap();
    let selected_events = directory.path().join("selected.log");
    let fallback_events = directory.path().join("fallback.log");
    let selected = app_server_candidate(
        directory.path(),
        "codex-selected",
        "complete",
        &selected_events,
    );
    let fallback = app_server_candidate(
        directory.path(),
        "codex-fallback",
        "complete",
        &fallback_events,
    );
    let runtime = CodexRuntime::new_with_discovery(
        Arc::new(CodexAppServerProvider::new("codex")),
        directory.path().join("settings.json"),
        CodexExecutableDiscovery::with_candidates(vec![fallback]),
    );
    runtime.select_executable(&selected).await.unwrap();

    runtime
        .ensure_ready(std::path::Path::new("/tmp/narrant-review-workspace"))
        .await
        .unwrap();

    assert!(selected_events.exists());
    assert!(!fallback_events.exists());
    runtime.shutdown().await;
}

#[tokio::test]
async fn invalid_explicit_executable_does_not_fall_back() {
    let directory = tempfile::tempdir().unwrap();
    let fallback_events = directory.path().join("fallback.log");
    let fallback = app_server_candidate(
        directory.path(),
        "codex-fallback",
        "complete",
        &fallback_events,
    );
    let settings_path = directory.path().join("settings.json");
    let missing = directory.path().join("codex-missing");
    std::fs::write(
        &settings_path,
        serde_json::json!({
            "version": 1,
            "theme": "system",
            "codexExecutable": missing,
            "babysitModel": { "mode": "automatic" }
        })
        .to_string(),
    )
    .unwrap();
    let runtime = CodexRuntime::new_with_discovery(
        Arc::new(CodexAppServerProvider::new("codex")),
        settings_path,
        CodexExecutableDiscovery::with_candidates(vec![fallback]),
    );

    let result = runtime
        .ensure_ready(std::path::Path::new("/tmp/narrant-review-workspace"))
        .await;

    assert!(matches!(
        result,
        Err(BabysitError::ProviderSetup { provider_id, failure: BabysitProviderSetupFailure::ExecutableInvalid })
            if provider_id == "codex-app-server"
    ));
    assert!(!fallback_events.exists());
}

#[tokio::test]
async fn clearing_explicit_executable_restores_automatic_discovery() {
    let directory = tempfile::tempdir().unwrap();
    let selected_events = directory.path().join("selected.log");
    let automatic_events = directory.path().join("automatic.log");
    let selected = app_server_candidate(
        directory.path(),
        "codex-selected",
        "complete",
        &selected_events,
    );
    let automatic = app_server_candidate(
        directory.path(),
        "codex-automatic",
        "complete",
        &automatic_events,
    );
    let runtime = CodexRuntime::new_with_discovery(
        Arc::new(CodexAppServerProvider::new("codex")),
        directory.path().join("settings.json"),
        CodexExecutableDiscovery::with_candidates(vec![automatic]),
    );
    runtime.select_executable(&selected).await.unwrap();
    runtime.clear_executable().await.unwrap();

    runtime
        .ensure_ready(std::path::Path::new("/tmp/narrant-review-workspace"))
        .await
        .unwrap();

    assert!(!selected_events.exists());
    assert!(automatic_events.exists());
    runtime.shutdown().await;
}

#[test]
fn runtime_maps_setup_failures_to_typed_states() {
    assert_eq!(
        status_for_error(&BabysitError::provider_setup(
            "codex-app-server",
            BabysitProviderSetupFailure::NotFound,
        )),
        CodexRuntimeStatus::Missing
    );
    assert_eq!(
        status_for_error(&BabysitError::provider_setup(
            "codex-app-server",
            BabysitProviderSetupFailure::SignedOut,
        )),
        CodexRuntimeStatus::SignedOut
    );
    assert_eq!(
        status_for_error(&BabysitError::provider_setup(
            "codex-app-server",
            BabysitProviderSetupFailure::Incompatible,
        )),
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
