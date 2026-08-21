use super::*;
use std::os::unix::fs::PermissionsExt;

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
    let discovery = CodexExecutableDiscovery;

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
    let discovery = CodexExecutableDiscovery;

    let resolved = discovery
        .resolve_candidates(
            Some(&directory.path().join("missing-codex")),
            [executable.clone()],
        )
        .await;

    let resolved = resolved.unwrap();
    assert_eq!(resolved.path, executable.canonicalize().unwrap());
}

#[tokio::test]
async fn invalid_selected_executable_returns_reselection_error_without_raw_output() {
    let directory = tempfile::tempdir().unwrap();
    let executable = version_fixture(directory.path(), false);
    let error = CodexExecutableDiscovery
        .validate_selected(executable)
        .await
        .unwrap_err();

    assert!(matches!(error, BabysitError::CodexExecutableInvalid));
    assert!(!error.to_string().contains("codex-bad"));
}
