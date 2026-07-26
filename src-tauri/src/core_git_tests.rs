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

#[test]
fn review_workspace_is_a_detached_linked_worktree_at_the_snapshot_head() {
    let fixture = repository();
    Command::new("git")
        .args(["config", "user.email", "test@narrant.invalid"])
        .current_dir(fixture.path())
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Narrant Test"])
        .current_dir(fixture.path())
        .status()
        .unwrap();
    std::fs::write(fixture.path().join("README.md"), "fixture").unwrap();
    Command::new("git")
        .args(["add", "README.md"])
        .current_dir(fixture.path())
        .status()
        .unwrap();
    Command::new("git")
        .args(["commit", "-q", "-m", "initial"])
        .current_dir(fixture.path())
        .status()
        .unwrap();
    let head = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(fixture.path())
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned();
    let managed = tempfile::TempDir::new().unwrap();
    let workspace = managed.path().join("reviews/review-1/repo");
    let adapter = GitProcessAdapter::new(fixture.path());

    adapter
        .create_detached_worktree(&workspace, &head, &CancellationToken::default())
        .unwrap();

    let workspace_head = String::from_utf8(
        Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&workspace)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let symbolic = Command::new("git")
        .args(["symbolic-ref", "-q", "HEAD"])
        .current_dir(&workspace)
        .status()
        .unwrap();
    assert_eq!(workspace_head.trim(), head);
    assert!(!symbolic.success());
}

#[test]
fn removes_only_an_exact_registered_owned_worktree_even_when_dirty() {
    let fixture = repository_with_commit();
    let head = git_text(fixture.path(), &["rev-parse", "HEAD"]);
    let common_directory = PathBuf::from(git_text(
        fixture.path(),
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ));
    let managed = tempfile::TempDir::new().unwrap();
    let workspace = managed.path().join("reviews/review-1/repo");
    let user_worktree = managed.path().join("user-worktree");
    let adapter = GitProcessAdapter::new(fixture.path());
    adapter
        .create_detached_worktree(&workspace, &head, &CancellationToken::default())
        .unwrap();
    Command::new("git")
        .args(["worktree", "add", "--detach", "--"])
        .arg(&user_worktree)
        .arg(&head)
        .current_dir(fixture.path())
        .status()
        .unwrap();
    std::fs::write(workspace.join("README.md"), "dirty").unwrap();
    std::fs::write(workspace.join("untracked.txt"), "untracked").unwrap();
    std::fs::write(workspace.join(".gitignore"), "ignored.txt\n").unwrap();
    std::fs::write(workspace.join("ignored.txt"), "ignored").unwrap();

    adapter
        .remove_owned_worktree(&workspace, &common_directory, &CancellationToken::default())
        .unwrap();

    assert!(!workspace.exists());
    assert!(user_worktree.exists());
    let error = adapter
        .remove_owned_worktree(
            &managed.path().join("not-registered"),
            &common_directory,
            &CancellationToken::default(),
        )
        .unwrap_err();
    assert!(error.message().contains("所有权验证失败"));
    assert!(user_worktree.exists());
}

/// 创建含一个提交的真实仓库，供 destructive worktree 后置条件测试使用。
fn repository_with_commit() -> tempfile::TempDir {
    let fixture = repository();
    Command::new("git")
        .args(["config", "user.email", "test@narrant.invalid"])
        .current_dir(fixture.path())
        .status()
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Narrant Test"])
        .current_dir(fixture.path())
        .status()
        .unwrap();
    std::fs::write(fixture.path().join("README.md"), "fixture").unwrap();
    Command::new("git")
        .args(["add", "README.md"])
        .current_dir(fixture.path())
        .status()
        .unwrap();
    Command::new("git")
        .args(["commit", "-q", "-m", "initial"])
        .current_dir(fixture.path())
        .status()
        .unwrap();
    fixture
}

/// 执行测试专用 Git 查询并返回去除换行的 UTF-8 文本。
fn git_text(repository: &Path, arguments: &[&str]) -> String {
    String::from_utf8(
        Command::new("git")
            .args(arguments)
            .current_dir(repository)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned()
}
