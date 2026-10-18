use super::*;

/// 已取消的分析不得启动 worktree 创建，也不能留下未注册的目录。
#[test]
fn cancelled_before_creation_does_not_touch_workspace() {
    let repository = committed_repository();
    let managed = tempfile::tempdir().unwrap();
    let workspace = managed.path().join("review/before");
    let head = read_git(repository.path(), ["rev-parse", "HEAD"]);
    let cancellation = CancellationToken::new();
    cancellation.cancel();

    let error = GitProcessAdapter::new(repository.path())
        .create_detached_worktree(
            &workspace,
            &head,
            &cancellation,
            OperationContext::background().cancellation(),
        )
        .unwrap_err();

    assert!(error.is_cancelled(), "{error}");
    assert!(!workspace.parent().unwrap().exists(), "{error}");
    let worktrees = read_git(repository.path(), ["worktree", "list", "--porcelain"]);
    assert_eq!(worktrees.matches("worktree ").count(), 1, "{worktrees}");
}

/// 在独立临时目录中创建真实 linked worktree，保留仓库直至校验结束。
struct WorkspaceFixture {
    repository: tempfile::TempDir,
    _managed: tempfile::TempDir,
    adapter: GitProcessAdapter,
    workspace: PathBuf,
    common_directory: PathBuf,
    head: String,
    cancellation: CancellationToken,
}

impl WorkspaceFixture {
    /// 带空格和非 ASCII 字符的路径用于验证 porcelain 零分隔字段与规范路径比较。
    fn new() -> Self {
        let repository = committed_repository();
        let managed = tempfile::tempdir().unwrap();
        let adapter = GitProcessAdapter::new(repository.path());
        let workspace = managed.path().join("review Ä workspace");
        let common_directory = PathBuf::from(read_git(
            repository.path(),
            ["rev-parse", "--path-format=absolute", "--git-common-dir"],
        ));
        let head = read_git(repository.path(), ["rev-parse", "HEAD"]);
        let cancellation = CancellationToken::new();
        adapter
            .create_detached_worktree(&workspace, &head, &cancellation, &cancellation)
            .unwrap();
        Self {
            repository,
            _managed: managed,
            adapter,
            workspace,
            common_directory,
            head,
            cancellation,
        }
    }
}

/// 注册路径可用等价规范路径访问；未跟踪文件不在源码一致性检查范围内。
#[test]
fn revision_validation_accepts_clean_registered_workspace() {
    let fixture = WorkspaceFixture::new();
    std::fs::write(fixture.workspace.join("untracked.rs"), "fn generated() {}").unwrap();
    fixture
        .adapter
        .validate_worktree_revision(
            &fixture.workspace.join("."),
            &fixture.common_directory.join("."),
            &fixture.head,
            &fixture.cancellation,
        )
        .unwrap();
}

/// 相同文件内容也不能代替仓库归属和精确 worktree 路径的注册证据。
#[test]
fn revision_validation_rejects_foreign_repository_and_unregistered_paths() {
    let fixture = WorkspaceFixture::new();
    let foreign = committed_repository();
    let error = fixture
        .adapter
        .validate_worktree_revision(
            &fixture.workspace,
            &foreign.path().join(".git"),
            &fixture.head,
            &fixture.cancellation,
        )
        .unwrap_err();
    assert_eq!(error.kind(), CoreGitFailureKind::InvalidData);
    assert!(error.message().contains("common directory 不匹配"));

    let nested = fixture.workspace.join("nested");
    std::fs::create_dir(&nested).unwrap();
    for path in [foreign.path(), nested.as_path()] {
        let error = fixture
            .adapter
            .validate_worktree_revision(
                path,
                &fixture.common_directory,
                &fixture.head,
                &fixture.cancellation,
            )
            .unwrap_err();
        assert_eq!(error.kind(), CoreGitFailureKind::InvalidData);
        assert!(error.message().contains("所有权验证失败"));
    }
}

/// 符号链接按 canonical 身份匹配，改指向外部仓库后不能沿用原 worktree 的注册。
#[cfg(unix)]
#[test]
fn revision_validation_resolves_symlinks_to_the_registered_path() {
    let fixture = WorkspaceFixture::new();
    let alias = fixture._managed.path().join("alias");
    std::os::unix::fs::symlink(&fixture.workspace, &alias).unwrap();
    fixture
        .adapter
        .validate_worktree_revision(
            &alias,
            &fixture.common_directory,
            &fixture.head,
            &fixture.cancellation,
        )
        .unwrap();

    let foreign = committed_repository();
    std::fs::remove_file(&alias).unwrap();
    std::os::unix::fs::symlink(foreign.path(), &alias).unwrap();
    let error = fixture
        .adapter
        .validate_worktree_revision(
            &alias,
            &fixture.common_directory,
            &fixture.head,
            &fixture.cancellation,
        )
        .unwrap_err();
    assert_eq!(error.kind(), CoreGitFailureKind::InvalidData);
    assert!(error.message().contains("所有权验证失败"));
    assert_eq!(
        std::fs::read_to_string(foreign.path().join("README.md")).unwrap(),
        "fixture"
    );
}

/// HEAD 必须来自目标 worktree 的同一条记录，不能借用清单中另一个工作目录的 HEAD。
#[test]
fn revision_validation_requires_head_from_the_matching_worktree() {
    let fixture = WorkspaceFixture::new();
    std::fs::write(fixture.repository.path().join("next.txt"), "next").unwrap();
    run_git(fixture.repository.path(), ["add", "next.txt"]);
    run_git(fixture.repository.path(), ["commit", "-q", "-m", "next"]);
    let different_head = read_git(fixture.repository.path(), ["rev-parse", "HEAD"]);
    let error = fixture
        .adapter
        .validate_worktree_revision(
            &fixture.workspace,
            &fixture.common_directory,
            &different_head,
            &fixture.cancellation,
        )
        .unwrap_err();
    assert_eq!(error.kind(), CoreGitFailureKind::InvalidData);
    assert!(error.message().contains("HEAD"));
    assert_eq!(
        read_git(&fixture.workspace, ["rev-parse", "HEAD"]),
        fixture.head
    );
}

/// 已跟踪源码相对 HEAD 的修改在暂存前后都会被拒绝，并保留 Git 失败类别。
#[test]
fn revision_validation_rejects_tracked_changes() {
    for staged in [false, true] {
        let fixture = WorkspaceFixture::new();
        std::fs::write(fixture.workspace.join("README.md"), "changed").unwrap();
        if staged {
            run_git(&fixture.workspace, ["add", "README.md"]);
        }
        let error = fixture
            .adapter
            .validate_worktree_revision(
                &fixture.workspace,
                &fixture.common_directory,
                &fixture.head,
                &fixture.cancellation,
            )
            .unwrap_err();
        assert_eq!(error.kind(), CoreGitFailureKind::Exited { code: Some(1) });
        assert!(error.message().contains("已跟踪源码"));
    }
}

/// 校验沿用调用方的取消 token，不能把取消改写成路径或版本校验失败。
#[test]
fn revision_validation_preserves_cancellation() {
    let fixture = WorkspaceFixture::new();
    fixture.cancellation.cancel();
    let error = fixture
        .adapter
        .validate_worktree_revision(
            &fixture.workspace,
            &fixture.common_directory,
            &fixture.head,
            &fixture.cancellation,
        )
        .unwrap_err();
    assert!(error.is_cancelled());
}
