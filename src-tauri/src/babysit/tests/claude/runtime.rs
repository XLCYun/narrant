use super::*;
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

    assert_eq!(runtime.provider_id(), "claude_code");
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
        matches!(error, BabysitError::ProviderSetup { provider_id, failure: BabysitProviderSetupFailure::SignedOut } if provider_id == "claude_code")
    );
    assert_eq!(runtime.status(), BabysitProviderRuntimeStatus::SignedOut);
    let invocations = std::fs::read_to_string(events).expect("read probe events");
    assert!(!invocations.contains(" -p "));
}

#[tokio::test]
async fn any_missing_required_help_flag_is_incompatible() {
    let (runtime, _directory, events) = runtime(&["missing-flag"]);

    let error = runtime.ensure_ready(std::path::Path::new(".")).await.expect_err("all six flags are required");

    assert!(matches!(error, BabysitError::ProviderSetup { failure: BabysitProviderSetupFailure::Incompatible, .. }));
    assert_eq!(runtime.status(), BabysitProviderRuntimeStatus::Incompatible);
    assert!(!std::fs::read_to_string(events).expect("read events").contains("auth status"));
}

#[tokio::test]
async fn malformed_auth_status_is_redacted_initialization_failure() {
    let (runtime, _directory, _events) = runtime(&["malformed-auth"]);

    let error = runtime.ensure_ready(std::path::Path::new("C:/private/repository")).await.expect_err("missing loggedIn field must fail closed");
    let payload = BabysitErrorPayload::from(error);

    assert_eq!(payload.code, BabysitErrorCode::ProviderInitializationFailed);
    assert_eq!(payload.provider_id.as_deref(), Some("claude_code"));
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
    assert_eq!(payload.provider_id.as_deref(), Some("claude_code"));
    assert!(!payload.detail.contains("private"));
}

#[test]
fn bare_path_is_preferred_before_selected_and_environment_fallbacks() {
    assert_eq!(
        candidate_paths(Some("selected-claude".into()), Some("environment-claude".into())),
        vec![std::path::PathBuf::from("claude"), std::path::PathBuf::from("selected-claude"), std::path::PathBuf::from("environment-claude")],
    );
}

#[test]
fn duplicate_fallbacks_are_not_probed_twice() {
    assert_eq!(
        candidate_paths(Some("claude".into()), Some("selected-claude".into())),
        vec![std::path::PathBuf::from("claude"), std::path::PathBuf::from("selected-claude")],
    );
}
