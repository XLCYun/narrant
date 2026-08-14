#[tokio::test]
async fn failed_target_materialization_keeps_current_review_readable() {
    let application_data = tempfile::TempDir::new().unwrap();
    let first_repository = test_repository_with_commit();
    let target_repository = test_repository_with_commit();
    let catalog = Arc::new(
        RepositoryCatalog::open(&application_data.path().join("catalog.sqlite3")).unwrap(),
    );
    let first_entry = catalog.register(first_repository.path()).unwrap();
    let target_entry = catalog.register(target_repository.path()).unwrap();
    catalog.grant_workspace_trust(first_entry.id).unwrap();
    catalog.grant_workspace_trust(target_entry.id).unwrap();
    let first_saved = save_current_review(&catalog, first_entry.id, first_repository.path()).await;
    let target_saved =
        save_current_review(&catalog, target_entry.id, target_repository.path()).await;
    catalog.activate_snapshot(&first_saved).unwrap();
    catalog.activate_snapshot(&target_saved).unwrap();
    let lifecycle = ReviewLifecycle::new(catalog.clone());
    let first = lifecycle
        .present(
            PresentReview::Saved {
                review_id: first_saved.review_id.clone(),
            },
            &CancellationToken::default(),
        )
        .await
        .unwrap();
    std::fs::remove_dir_all(target_repository.path()).unwrap();

    let error = match lifecycle
        .present(
            PresentReview::Saved {
                review_id: target_saved.review_id,
            },
            &CancellationToken::default(),
        )
        .await
    {
        Ok(_) => panic!("目标 Review materialization 应失败"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        ReviewLifecycleError::RepositoryUnavailable(_)
    ));
    let content = lifecycle
        .read_file_change(
            &first_saved.review_id,
            &first.snapshot_id,
            "README.md",
            &CancellationToken::default(),
        )
        .await
        .unwrap();
    assert_eq!(content.path, "README.md");
}

#[tokio::test]
async fn pull_request_lifecycle_resolves_durable_selection_without_caller_observation() {
    let fixture = LifecycleFixture::new();
    let (base_revision, head_revision) = fixture.configure_pull_request_remote();
    let server = MockServer::start();
    let observation = server.mock(|when, then| {
        when.method(GET).path("/repos/octo-org/octo-repo/pulls/42");
        then.status(200).json_body(serde_json::json!({
            "number": 42,
            "title": "Lifecycle-owned resolution",
            "body": "",
            "user": { "login": "octocat" },
            "state": "open",
            "merged_at": null,
            "draft": false,
            "html_url": "https://github.com/octo-org/octo-repo/pull/42",
            "updated_at": "2026-08-06T00:00:00Z",
            "base": { "ref": "base", "sha": base_revision },
            "head": { "ref": "topic", "sha": head_revision }
        }));
    });
    let lifecycle = ReviewLifecycle::with_pull_request_resolver(
        fixture.catalog.clone(),
        Arc::new(PullRequestSelectionResolver::anonymous(server.base_url())),
    );

    let opened = lifecycle
        .present(
            PresentReview::PullRequest {
                repository_entry_id: fixture.repository_entry_id,
                selection: PullRequestSelection {
                    repository: crate::github::GitHubRepository {
                        owner: "octo-org".to_owned(),
                        name: "octo-repo".to_owned(),
                    },
                    number: 42,
                },
            },
            &CancellationToken::default(),
        )
        .await
        .unwrap();
    let update = lifecycle
        .check_update(&opened.review_id, &CancellationToken::default())
        .await
        .unwrap();
    let error = match lifecycle
        .refresh(
            &opened.review_id,
            ConfirmedReviewUpdate {
                current_base_revision: update.current_base_revision,
                current_head_revision: update.current_head_revision,
                available_base_revision: "changed-after-confirmation".to_owned(),
                available_head_revision: update.available_head_revision,
            },
            &CancellationToken::default(),
        )
        .await
    {
        Ok(_) => panic!("确认后的 Pull Request 移动应拒绝刷新"),
        Err(error) => error,
    };

    assert_eq!(
        opened.pull_request.unwrap().title,
        "Lifecycle-owned resolution"
    );
    assert!(!update.update_available);
    assert!(matches!(error, ReviewLifecycleError::SelectionMismatch(_)));
    assert_eq!(observation.hits(), 3);
}

/// 生命周期测试所需的应用目录、Repository Catalog 与 Git repository。
struct LifecycleFixture {
    _application_data: tempfile::TempDir,
    repository: tempfile::TempDir,
    catalog: Arc<RepositoryCatalog>,
    repository_entry_id: i64,
}

impl LifecycleFixture {
    /// 创建一个已注册且已授予 Workspace Trust 的测试环境。
    fn new() -> Self {
        let application_data = tempfile::TempDir::new().unwrap();
        let repository = test_repository_with_commit();
        let catalog = Arc::new(
            RepositoryCatalog::open(&application_data.path().join("catalog.sqlite3")).unwrap(),
        );
        let entry = catalog.register(repository.path()).unwrap();
        catalog.grant_workspace_trust(entry.id).unwrap();
        Self {
            _application_data: application_data,
            repository,
            catalog,
            repository_entry_id: entry.id,
        }
    }

    /// 保存当前分支相对其父提交的测试 Review。
    async fn save_current_review(&self) -> SavedBranchReview {
        save_current_review(
            &self.catalog,
            self.repository_entry_id,
            self.repository.path(),
        )
        .await
    }

    /// 把测试 repository 配成可通过 GitHub identity 匹配的 PR base remote。
    fn configure_pull_request_remote(&self) -> (String, String) {
        let base_revision = git_text(self.repository.path(), &["rev-parse", "refs/heads/base"]);
        let head_revision = git_text(self.repository.path(), &["rev-parse", "refs/heads/topic"]);
        run_git(
            self.repository.path(),
            &["update-ref", "refs/pull/42/head", &head_revision],
        );
        run_git(
            self.repository.path(),
            &[
                "remote",
                "add",
                "origin",
                "https://github.com/octo-org/octo-repo.git",
            ],
        );
        run_git(
            self.repository.path(),
            &[
                "config",
                &format!("url.{}.insteadOf", self.repository.path().display()),
                "https://github.com/octo-org/octo-repo.git",
            ],
        );
        (base_revision, head_revision)
    }
}

/// 创建两个提交和 base/head 分支，保证测试 Review 至少包含一个 File Change。
fn test_repository_with_commit() -> tempfile::TempDir {
    let repository = tempfile::TempDir::new().unwrap();
    run_git(repository.path(), &["init", "-q"]);
    run_git(
        repository.path(),
        &["config", "user.email", "test@narrant.invalid"],
    );
    run_git(repository.path(), &["config", "user.name", "Narrant Test"]);
    std::fs::write(repository.path().join("README.md"), "before\n").unwrap();
    run_git(repository.path(), &["add", "README.md"]);
    run_git(repository.path(), &["commit", "-q", "-m", "base"]);
    run_git(repository.path(), &["branch", "base"]);
    std::fs::write(repository.path().join("README.md"), "after\n").unwrap();
    run_git(repository.path(), &["commit", "-q", "-am", "head"]);
    run_git(repository.path(), &["branch", "topic"]);
    repository
}

/// 保存 base/head 分支组成的测试 Review。
async fn save_current_review(
    catalog: &RepositoryCatalog,
    repository_entry_id: i64,
    repository_path: &Path,
) -> SavedBranchReview {
    let resolved = BranchSelectionResolver
        .resolve(
            repository_path,
            &BranchSelection {
                base_ref: "refs/heads/base".to_owned(),
                head_ref: "refs/heads/topic".to_owned(),
            },
            &OperationContext::non_cancellable(),
        )
        .await
        .unwrap();
    catalog
        .save_branch_review(repository_entry_id, &resolved.comparison)
        .unwrap()
}

/// 在指定测试 repository 中执行必须成功的 Core Git 命令。
fn run_git(repository: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .status()
        .unwrap();
    assert!(status.success(), "git {arguments:?} failed");
}

/// 返回测试断言所需的单行 Core Git 输出。
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
