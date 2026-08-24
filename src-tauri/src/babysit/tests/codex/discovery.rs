use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// 写入一个带启动 marker 的 shell executable，供直接子进程回收测试使用。
fn process_fixture(directory: &Path, body: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let executable = directory.join("codex-process");
    let marker = directory.join("codex-process.pid");
    let script = format!(
        "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\n{}\n",
        marker.display(),
        body
    );
    std::fs::write(&executable, script).unwrap();
    let mut permissions = std::fs::metadata(&executable).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&executable, permissions).unwrap();
    (executable, marker)
}

/// 构造返回固定版本的可执行文件，不触碰真实 Codex 安装。
fn version_fixture(directory: &std::path::Path, success: bool) -> std::path::PathBuf {
    let path = directory.join(if success { "codex-ok" } else { "codex-bad" });
    let body = if success {
        "#!/bin/sh\nprintf 'codex-test 0.0.0\\n'\n"
    } else {
        "#!/bin/sh\nexit 17\n"
    };
    std::fs::write(&path, body).unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&path, permissions).unwrap();
    path
}

#[tokio::test]
async fn selected_executable_is_version_checked_without_persisting_from_discovery() {
    let directory = tempfile::tempdir().unwrap();
    let executable = version_fixture(directory.path(), true);
    let discovery = CodexExecutableDiscovery::default();

    let resolved = discovery
        .validate_selected(executable.clone())
        .await
        .unwrap();

    assert_eq!(resolved.path, executable.canonicalize().unwrap());
    assert_eq!(resolved.version, "codex-test 0.0.0");
}

#[tokio::test]
async fn persisted_invalid_path_falls_back_to_process_candidates() {
    let directory = tempfile::tempdir().unwrap();
    let executable = version_fixture(directory.path(), true);
    let discovery = CodexExecutableDiscovery::with_candidates(vec![executable.clone()]);

    let resolved = discovery
        .resolve_all(Some(&directory.path().join("missing-codex")))
        .await;

    let resolved = resolved.unwrap().remove(0);
    assert_eq!(resolved.path, executable.canonicalize().unwrap());
}

#[tokio::test]
async fn process_environment_candidate_precedes_a_valid_persisted_path() {
    let directory = tempfile::tempdir().unwrap();
    let environment = version_fixture(directory.path(), true);
    let persisted_directory = tempfile::tempdir().unwrap();
    let persisted = version_fixture(persisted_directory.path(), true);

    let resolved = CodexExecutableDiscovery::with_candidates(vec![environment.clone()])
        .resolve_all(Some(&persisted))
        .await
        .unwrap()
        .remove(0);

    assert_eq!(resolved.path, environment.canonicalize().unwrap());
}

#[test]
fn bare_codex_command_is_the_first_environment_candidate() {
    let candidates = CodexExecutableDiscovery::default().candidates();

    assert_eq!(candidates.first(), Some(&std::path::PathBuf::from("codex")));
}

#[tokio::test]
async fn invalid_selected_executable_returns_reselection_error_without_raw_output() {
    let directory = tempfile::tempdir().unwrap();
    let executable = version_fixture(directory.path(), false);
    let error = CodexExecutableDiscovery::default()
        .validate_selected(executable)
        .await
        .unwrap_err();

    assert!(matches!(
        &error,
        BabysitError::ProviderSetup { provider_id, failure: BabysitProviderSetupFailure::ExecutableInvalid }
            if provider_id == "codex-app-server"
    ));
    assert!(!error.to_string().contains("codex-bad"));
}

#[tokio::test]
async fn oversized_version_output_is_bounded_and_child_is_reaped() {
    let directory = tempfile::tempdir().unwrap();
    let output = "x".repeat(5000);
    let body = format!("printf '%s' '{}'\nsleep 30", output);
    let (executable, marker) = process_fixture(directory.path(), &body);
    let (result, reaped) = CodexExecutableDiscovery::default()
        .validate_with_timeout_observed(executable, true, std::time::Duration::from_secs(5))
        .await;

    assert!(matches!(
        result,
        Err(BabysitError::ProviderSetup { provider_id, failure: BabysitProviderSetupFailure::ExecutableInvalid })
            if provider_id == "codex-app-server"
    ));
    assert!(marker.exists(), "version fixture did not start");
    assert!(reaped, "direct version child was not reaped");
}

#[tokio::test]
async fn hanging_version_process_is_killed_and_reaped_on_timeout() {
    let directory = tempfile::tempdir().unwrap();
    let (executable, marker) = process_fixture(directory.path(), "sleep 30");
    let (result, reaped) = CodexExecutableDiscovery::default()
        .validate_with_timeout_observed(executable, true, std::time::Duration::from_secs(3))
        .await;

    assert!(matches!(
        result,
        Err(BabysitError::ProviderSetup { provider_id, failure: BabysitProviderSetupFailure::ExecutableInvalid })
            if provider_id == "codex-app-server"
    ));
    assert!(marker.exists(), "version fixture did not start");
    assert!(reaped, "direct version child was not reaped");
}
