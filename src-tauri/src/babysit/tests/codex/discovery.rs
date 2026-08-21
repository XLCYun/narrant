use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// 写入一个自带 PID marker 的 shell executable，供 kill/reap 行为测试使用。
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

/// 等待版本探测失败后被显式 reap 的 PID 消失，避免只断言错误类别。
async fn assert_process_reaped(marker: &Path) {
    for _ in 0..100 {
        if let Ok(pid) = std::fs::read_to_string(marker)
            .and_then(|value| value.parse::<libc::pid_t>().map_err(std::io::Error::other))
        {
            // `kill(pid, 0)` 只探测存在性，不给 fixture 进程发送信号。
            if unsafe { libc::kill(pid, 0) } == -1
                && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
            {
                return;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("version probe child was not reaped");
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

#[tokio::test]
async fn oversized_version_output_is_bounded_and_child_is_reaped() {
    let directory = tempfile::tempdir().unwrap();
    let (executable, marker) = process_fixture(
        directory.path(),
        "i=0\nwhile [ \"$i\" -lt 5000 ]; do printf x; i=$((i + 1)); done\nwhile :; do :; done",
    );
    let error = CodexExecutableDiscovery
        .validate_with_timeout(executable, true, std::time::Duration::from_secs(1))
        .await
        .unwrap_err();

    assert!(matches!(error, BabysitError::CodexExecutableInvalid));
    assert_process_reaped(&marker).await;
}

#[tokio::test]
async fn hanging_version_process_is_killed_and_reaped_on_timeout() {
    let directory = tempfile::tempdir().unwrap();
    let (executable, marker) = process_fixture(directory.path(), "while :; do :; done");
    let error = CodexExecutableDiscovery
        .validate_with_timeout(executable, true, std::time::Duration::from_secs(3))
        .await
        .unwrap_err();

    assert!(matches!(error, BabysitError::CodexExecutableInvalid));
    assert_process_reaped(&marker).await;
}
