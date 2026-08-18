use super::*;
use crate::operation::{OperationContext, TestCancellation};

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

/// 创建带用户配置和首个提交的真实仓库。
fn committed_repository() -> tempfile::TempDir {
    let fixture = repository();
    run_git(
        fixture.path(),
        ["config", "user.email", "test@narrant.invalid"],
    );
    run_git(fixture.path(), ["config", "user.name", "Narrant Test"]);
    std::fs::write(fixture.path().join("README.md"), "fixture").unwrap();
    run_git(fixture.path(), ["add", "README.md"]);
    run_git(fixture.path(), ["commit", "-q", "-m", "initial"]);
    fixture
}

/// 执行预期成功且无需读取输出的测试 Git 命令。
fn run_git<const N: usize>(path: &Path, arguments: [&str; N]) {
    assert!(Command::new("git")
        .args(arguments)
        .current_dir(path)
        .status()
        .unwrap()
        .success());
}

/// 读取预期成功的单行测试 Git 输出。
fn read_git<const N: usize>(path: &Path, arguments: [&str; N]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[tokio::test]
async fn cancellation_stops_a_core_git_operation() {
    let fixture = repository();
    let process = CoreGitProcess::default();
    let cancellation = TestCancellation::default();
    cancellation.cancel();
    let error = process
        .execute(
            GitInvocation::new(
                fixture.path(),
                ["rev-parse", "--git-dir"],
                GitOperationClass::Query,
            ),
            &cancellation,
        )
        .await
        .unwrap_err();
    assert_eq!(error.kind(), CoreGitFailureKind::Cancelled);
}

#[tokio::test]
async fn core_git_process_returns_machine_output_through_its_public_seam() {
    let fixture = repository();
    let process = CoreGitProcess::default();
    let invocation = GitInvocation::new(
        fixture.path(),
        ["rev-parse", "--git-dir"],
        GitOperationClass::Query,
    );

    let output = process
        .execute(invocation, OperationContext::background().cancellation())
        .await
        .unwrap();

    assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), ".git");
}

#[tokio::test]
async fn core_git_process_preserves_failure_kind_when_context_is_added() {
    let fixture = repository();
    let process = CoreGitProcess::with_limits(Duration::ZERO, DEFAULT_OUTPUT_LIMIT);
    let invocation = GitInvocation::new(
        fixture.path(),
        ["rev-parse", "--git-dir"],
        GitOperationClass::Query,
    );

    let error = process
        .execute(invocation, OperationContext::background().cancellation())
        .await
        .unwrap_err()
        .with_context("解析仓库");

    assert_eq!(error.kind(), CoreGitFailureKind::TimedOut);
    assert!(error.message().contains("解析仓库"));
}

#[tokio::test]
async fn core_git_process_classifies_the_stream_that_crosses_its_limit() {
    let fixture = repository();
    let process = CoreGitProcess::with_limits(DEFAULT_TIMEOUT, 1);
    let invocation = GitInvocation::new(
        fixture.path(),
        ["rev-parse", "--git-dir"],
        GitOperationClass::Query,
    );

    let error = process
        .execute(invocation, OperationContext::background().cancellation())
        .await
        .unwrap_err();

    assert_eq!(
        error.kind(),
        CoreGitFailureKind::OutputLimitExceeded {
            stream: GitOutputStream::Stdout,
            limit: 1,
        },
        "{error:?}"
    );
}

#[tokio::test]
#[cfg(unix)]
async fn cancellation_stops_an_already_running_core_git_process() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = repository();
    std::fs::set_permissions(fixture.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let process = CoreGitProcess::default();
    let cancellation = TestCancellation::default();
    let cancellation_request = cancellation.clone();
    let socket = fixture.path().join("credential-cache.socket");
    let canceller = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        cancellation_request.cancel();
    });
    let error = process
        .execute(
            GitInvocation::new(
                fixture.path(),
                ["credential-cache--daemon", socket.to_str().unwrap()],
                GitOperationClass::Query,
            ),
            &cancellation,
        )
        .await
        .unwrap_err();
    canceller.join().unwrap();
    assert_eq!(error.kind(), CoreGitFailureKind::Cancelled);
}

#[tokio::test]
#[cfg(unix)]
async fn cancellation_reaps_descendants_before_returning() {
    let fixture = repository();
    let process = CoreGitProcess::default();
    let cancellation = TestCancellation::default();
    let cancellation_request = cancellation.clone();
    let canceller = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        cancellation_request.cancel();
    });
    let started = Instant::now();

    let error = process
        .execute(
            GitInvocation::new(
                fixture.path(),
                ["-c", "alias.block=!sleep 30", "block"],
                GitOperationClass::Query,
            ),
            &cancellation,
        )
        .await
        .unwrap_err();
    canceller.join().unwrap();

    assert_eq!(error.kind(), CoreGitFailureKind::Cancelled);
    assert!(started.elapsed() < Duration::from_secs(1));
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
        .create_detached_worktree(
            &workspace,
            &head,
            OperationContext::background().cancellation(),
        )
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
fn cancelled_workspace_creation_removes_its_partial_worktree() {
    let fixture = committed_repository();
    for index in 0..1_000 {
        std::fs::write(
            fixture.path().join(format!("workspace-{index}.txt")),
            format!("{index}\n"),
        )
        .unwrap();
    }
    run_git(fixture.path(), ["add", "."]);
    run_git(fixture.path(), ["commit", "-q", "-m", "large workspace"]);
    let head = read_git(fixture.path(), ["rev-parse", "HEAD"]);
    let managed = tempfile::TempDir::new().unwrap();
    let workspace = managed.path().join("reviews/review-1/repo");
    let cancellation = TestCancellation::default();
    let cancellation_request = cancellation.clone();
    let observed_workspace = workspace.clone();
    let observer = thread::spawn(move || {
        for _ in 0..5_000 {
            if observed_workspace.exists() {
                cancellation_request.cancel();
                return true;
            }
            thread::sleep(Duration::from_millis(1));
        }
        false
    });

    let error = GitProcessAdapter::new(fixture.path())
        .create_detached_worktree(&workspace, &head, &cancellation)
        .unwrap_err();

    assert!(
        observer.join().unwrap(),
        "未观察到 Review Workspace 开始创建"
    );
    assert!(error.is_cancelled(), "{error}");
    let worktree_list = read_git(fixture.path(), ["worktree", "list", "--porcelain"]);
    assert!(!workspace.exists(), "{error}; worktrees: {worktree_list}");
    assert!(!worktree_list.contains(&workspace.to_string_lossy().to_string()));
}

#[test]
fn removal_clears_a_registered_worktree_after_its_directory_disappears() {
    let fixture = committed_repository();
    let head = read_git(fixture.path(), ["rev-parse", "HEAD"]);
    let managed = tempfile::TempDir::new().unwrap();
    let workspace = managed.path().join("reviews/review-1/repo");
    let adapter = GitProcessAdapter::new(fixture.path());
    adapter
        .create_detached_worktree(
            &workspace,
            &head,
            OperationContext::background().cancellation(),
        )
        .unwrap();
    std::fs::remove_dir_all(&workspace).unwrap();

    adapter
        .remove_worktree(&workspace, OperationContext::background().cancellation())
        .unwrap();

    let worktree_list = read_git(fixture.path(), ["worktree", "list", "--porcelain"]);
    assert!(!worktree_list.contains(&workspace.to_string_lossy().to_string()));
}

#[test]
fn force_restore_review_workspace_discards_dirty_state_and_moves_detached_head() {
    let repository = committed_repository();
    let adapter = GitProcessAdapter::new(repository.path());
    let cancellation = TestCancellation::default();
    let first_head = read_git(repository.path(), ["rev-parse", "HEAD"]);
    let common_directory = PathBuf::from(read_git(
        repository.path(),
        ["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ));
    let workspace_root = tempfile::TempDir::new().unwrap();
    let workspace = workspace_root.path().join("repo");
    adapter
        .create_detached_worktree(&workspace, &first_head, &cancellation)
        .unwrap();
    std::fs::write(workspace.join("dirty.txt"), "discard me").unwrap();

    std::fs::write(repository.path().join("next.txt"), "next").unwrap();
    run_git(repository.path(), ["add", "next.txt"]);
    run_git(repository.path(), ["commit", "-q", "-m", "next"]);
    let next_head = read_git(repository.path(), ["rev-parse", "HEAD"]);

    adapter
        .transition_detached_worktree(
            &workspace,
            &common_directory,
            &first_head,
            &next_head,
            &cancellation,
        )
        .unwrap();

    assert_eq!(read_git(&workspace, ["rev-parse", "HEAD"]), next_head);
    assert!(!workspace.join("dirty.txt").exists());
    assert!(!Command::new("git")
        .args(["symbolic-ref", "-q", "HEAD"])
        .current_dir(&workspace)
        .status()
        .unwrap()
        .success());
}

#[test]
fn workspace_transition_rejects_an_unregistered_directory_before_deletion() {
    let repository = committed_repository();
    let adapter = GitProcessAdapter::new(repository.path());
    let head = read_git(repository.path(), ["rev-parse", "HEAD"]);
    let common_directory = PathBuf::from(read_git(
        repository.path(),
        ["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ));
    let unmanaged = tempfile::TempDir::new().unwrap();
    std::fs::write(unmanaged.path().join("keep.txt"), "owned elsewhere").unwrap();

    let error = adapter
        .transition_detached_worktree(
            unmanaged.path(),
            &common_directory,
            &head,
            &head,
            OperationContext::background().cancellation(),
        )
        .unwrap_err();

    assert!(error.to_string().contains("所有权验证失败"));
    assert_eq!(
        std::fs::read_to_string(unmanaged.path().join("keep.txt")).unwrap(),
        "owned elsewhere"
    );
}

#[test]
fn failed_workspace_transition_restores_the_previous_snapshot_head() {
    let repository = committed_repository();
    let adapter = GitProcessAdapter::new(repository.path());
    let cancellation = TestCancellation::default();
    let old_head = read_git(repository.path(), ["rev-parse", "HEAD"]);
    let common_directory = PathBuf::from(read_git(
        repository.path(),
        ["rev-parse", "--path-format=absolute", "--git-common-dir"],
    ));
    let workspace_root = tempfile::TempDir::new().unwrap();
    let workspace = workspace_root.path().join("repo");
    adapter
        .create_detached_worktree(&workspace, &old_head, &cancellation)
        .unwrap();

    let error = adapter
        .transition_detached_worktree(
            &workspace,
            &common_directory,
            &old_head,
            "0000000000000000000000000000000000000000",
            &cancellation,
        )
        .unwrap_err();

    assert!(error.to_string().contains("Review Workspace"));
    assert_eq!(read_git(&workspace, ["rev-parse", "HEAD"]), old_head);
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
        .create_detached_worktree(
            &workspace,
            &head,
            OperationContext::background().cancellation(),
        )
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
        .remove_owned_worktree(
            &workspace,
            &common_directory,
            OperationContext::background().cancellation(),
        )
        .unwrap();

    assert!(!workspace.exists());
    assert!(user_worktree.exists());
    let error = adapter
        .remove_owned_worktree(
            &managed.path().join("not-registered"),
            &common_directory,
            OperationContext::background().cancellation(),
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
