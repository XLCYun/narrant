use super::*;

/// 创建最小真实仓库，供 adapter 限制测试执行机器格式命令。
fn repository() -> tempfile::TempDir {
    let fixture = tempfile::TempDir::new().unwrap();
    let status = Command::new("git")
        .args(["init", "-q"])
        .current_dir(fixture.path())
        .status()
        .unwrap();
    assert!(status.success());
    fixture
}

#[test]
fn cancellation_stops_a_core_git_operation() {
    let fixture = repository();
    let adapter = GitProcessAdapter::new(fixture.path());
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    let error = adapter
        .run(["rev-parse", "--git-dir"], &cancellation)
        .unwrap_err();
    assert_eq!(error.message(), "Core Git 操作已取消");
}

#[test]
#[cfg(unix)]
fn cancellation_stops_an_already_running_core_git_process() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = repository();
    std::fs::set_permissions(fixture.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let adapter = GitProcessAdapter::new(fixture.path());
    let cancellation = CancellationToken::default();
    let cancellation_request = cancellation.clone();
    let socket = fixture.path().join("credential-cache.socket");
    let canceller = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        cancellation_request.cancel();
    });
    let error = adapter
        .run(
            ["credential-cache--daemon", socket.to_str().unwrap()],
            &cancellation,
        )
        .unwrap_err();
    canceller.join().unwrap();
    assert_eq!(error.message(), "Core Git 操作已取消");
}

#[test]
fn timeout_stops_a_core_git_operation() {
    let fixture = repository();
    let mut adapter = GitProcessAdapter::new(fixture.path());
    adapter.timeout = Duration::ZERO;
    let error = adapter
        .run(["rev-parse", "--git-dir"], &CancellationToken::default())
        .unwrap_err();
    assert_eq!(error.message(), "Core Git 操作超时");
}

#[test]
fn output_limit_rejects_oversized_machine_output() {
    let fixture = repository();
    let mut adapter = GitProcessAdapter::new(fixture.path());
    adapter.output_limit = 1;
    let error = adapter
        .run(["rev-parse", "--git-dir"], &CancellationToken::default())
        .unwrap_err();
    assert_eq!(error.message(), "Core Git 输出超过安全上限");
}
