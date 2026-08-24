use super::*;
use crate::application_settings::{
    ApplicationSettingsApplication, BabysitProvider, BabysitProviderSettings, ThemePreference,
};
use crate::babysit::{BabysitErrorCode, BabysitErrorPayload};
use std::ffi::OsString;
use std::sync::Arc;

/// 返回可执行 shell 与 runtime fixture，避免依赖真实 Claude Code 或付费请求。
fn fixture_candidate(
    scenario: &str,
    events: &std::path::Path,
) -> (std::path::PathBuf, Vec<OsString>) {
    let shell = if cfg!(windows) {
        std::path::PathBuf::from(r"C:\Program Files\Git\bin\sh.exe")
    } else {
        std::path::PathBuf::from("sh")
    };
    let fixture = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/babysit/tests/fixtures/fake_claude_runtime.sh");
    (
        shell,
        vec![
            fixture.into_os_string(),
            scenario.into(),
            events.as_os_str().to_owned(),
        ],
    )
}

/// 构造带独立 settings 与事件日志的 Claude runtime。
fn runtime(scenarios: &[&str]) -> (ClaudeRuntime, tempfile::TempDir, std::path::PathBuf) {
    let directory = tempfile::tempdir().expect("create runtime fixture directory");
    let events = directory.path().join("events.log");
    let candidates = scenarios
        .iter()
        .map(|scenario| fixture_candidate(scenario, &events))
        .collect();
    let provider = Arc::new(ClaudeCodeProvider::new("unused-claude"));
    let runtime = ClaudeRuntime::with_candidates(
        provider,
        directory.path().join("settings.json"),
        candidates,
    );
    (runtime, directory, events)
}

#[tokio::test]
async fn failed_candidate_does_not_shadow_later_authenticated_candidate() {
    let (runtime, _directory, events) = runtime(&["invalid", "ready"]);

    runtime
        .ensure_ready(std::path::Path::new("."))
        .await
        .expect("later candidate should become ready");

    assert_eq!(runtime.provider_id(), "claude-code");
    assert_eq!(runtime.status(), BabysitProviderRuntimeStatus::Ready);
    let invocations = std::fs::read_to_string(events).expect("read probe events");
    assert!(invocations.contains("invalid:--version"));
    assert!(invocations.contains("ready:--help"));
    assert!(invocations.contains("ready:auth status --json"));
    assert!(!invocations.contains(" -p "));
}

#[tokio::test]
async fn successful_auth_command_without_logged_in_true_is_signed_out() {
    let (runtime, _directory, events) = runtime(&["signed-out"]);

    let error = runtime
        .ensure_ready(std::path::Path::new("."))
        .await
        .expect_err("explicit loggedIn false must fail");

    assert!(
        matches!(error, BabysitError::ProviderSetup { provider_id, failure: BabysitProviderSetupFailure::SignedOut } if provider_id == "claude-code")
    );
    assert_eq!(runtime.status(), BabysitProviderRuntimeStatus::SignedOut);
    let invocations = std::fs::read_to_string(events).expect("read probe events");
    assert!(!invocations.contains(" -p "));
}

#[tokio::test]
async fn any_missing_required_help_flag_is_incompatible() {
    let (runtime, _directory, events) = runtime(&["missing-flag"]);

    let error = runtime
        .ensure_ready(std::path::Path::new("."))
        .await
        .expect_err("all six flags are required");

    assert!(matches!(
        error,
        BabysitError::ProviderSetup {
            failure: BabysitProviderSetupFailure::Incompatible,
            ..
        }
    ));
    assert_eq!(runtime.status(), BabysitProviderRuntimeStatus::Incompatible);
    assert!(!std::fs::read_to_string(events)
        .expect("read events")
        .contains("auth status"));
}

#[tokio::test]
async fn malformed_auth_status_is_redacted_initialization_failure() {
    let (runtime, _directory, _events) = runtime(&["malformed-auth"]);

    let error = runtime
        .ensure_ready(std::path::Path::new("C:/private/repository"))
        .await
        .expect_err("missing loggedIn field must fail closed");
    let payload = BabysitErrorPayload::from(error);

    assert_eq!(payload.code, BabysitErrorCode::ProviderInitializationFailed);
    assert_eq!(payload.provider_id.as_deref(), Some("claude-code"));
    assert!(!payload.detail.contains("private"));
    assert!(!payload.detail.contains("oauth"));
}

#[tokio::test]
async fn unavailable_candidates_are_classified_as_missing_without_leaking_paths() {
    let (runtime, _directory, _events) = runtime(&["invalid"]);

    let error = runtime
        .ensure_ready(std::path::Path::new("C:/private/repository"))
        .await
        .expect_err("an unavailable executable must be reported as missing");
    let payload = BabysitErrorPayload::from(error);

    assert_eq!(payload.code, BabysitErrorCode::ProviderNotFound);
    assert_eq!(payload.provider_id.as_deref(), Some("claude-code"));
    assert!(!payload.detail.contains("private"));
}

#[tokio::test]
async fn every_probe_step_has_a_bounded_timeout() {
    for scenario in ["hang-version", "hang-help", "hang-auth"] {
        let (runtime, _directory, _events) = runtime(&[scenario]);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            runtime.ensure_ready(std::path::Path::new(".")),
        )
        .await;
        assert!(result.is_ok(), "{scenario} probe exceeded its step timeout");
        assert!(result.unwrap().is_err());
    }
}

#[tokio::test]
async fn actionable_candidate_error_is_not_shadowed_by_later_missing_candidate() {
    for (scenarios, expected) in [
        (
            vec!["signed-out", "invalid"],
            BabysitProviderSetupFailure::SignedOut,
        ),
        (
            vec!["incompatible", "invalid"],
            BabysitProviderSetupFailure::Incompatible,
        ),
        (
            vec!["initialization-failed", "invalid"],
            BabysitProviderSetupFailure::InitializationFailed,
        ),
    ] {
        let (runtime, _directory, _events) = runtime(&scenarios);
        let error = runtime
            .ensure_ready(std::path::Path::new("."))
            .await
            .unwrap_err();
        assert!(
            matches!(error, BabysitError::ProviderSetup { failure, .. } if failure == expected)
        );
    }
}

#[tokio::test]
async fn signed_out_precedes_incompatible_and_initialization_failure() {
    let (runtime, _directory, _events) =
        runtime(&["initialization-failed", "incompatible", "signed-out"]);
    let error = runtime
        .ensure_ready(std::path::Path::new("."))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        BabysitError::ProviderSetup {
            failure: BabysitProviderSetupFailure::SignedOut,
            ..
        }
    ));
}

#[tokio::test]
async fn incompatible_precedes_initialization_failure() {
    let (runtime, _directory, _events) = runtime(&["initialization-failed", "incompatible"]);
    let error = runtime
        .ensure_ready(std::path::Path::new("."))
        .await
        .unwrap_err();
    assert!(matches!(
        error,
        BabysitError::ProviderSetup {
            failure: BabysitProviderSetupFailure::Incompatible,
            ..
        }
    ));
}

#[test]
fn unified_settings_supply_the_authoritative_claude_executable() {
    let directory = tempfile::tempdir().expect("create settings directory");
    let settings_path = directory.path().join("settings.json");
    let settings = ApplicationSettingsApplication::new(settings_path.clone());
    settings
        .update_theme(ThemePreference::Dark)
        .expect("persist complete unified settings");
    settings
        .update_provider(BabysitProvider::ClaudeCode)
        .expect("persist selected provider");
    settings
        .update_provider_settings(
            BabysitProvider::ClaudeCode,
            BabysitProviderSettings {
                executable: Some("selected-claude".into()),
                model: Some("sonnet".to_owned()),
                reasoning_effort: Some("high".to_owned()),
            },
        )
        .expect("persist Claude preference");
    let runtime = ClaudeRuntime::new(
        Arc::new(ClaudeCodeProvider::new("unused-claude")),
        settings_path,
    );

    let candidates = runtime.candidates();
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates[0].program,
        std::path::PathBuf::from("selected-claude")
    );
}
