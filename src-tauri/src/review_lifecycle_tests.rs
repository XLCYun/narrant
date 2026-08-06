use super::*;
use std::process::Command;

#[test]
fn same_review_cannot_hold_two_refresh_leases() {
    let fixture = LifecycleFixture::new();
    let lifecycle = ReviewLifecycle::new(fixture.catalog.clone());

    lifecycle.begin_refresh("review-1").unwrap();
    let error = lifecycle.begin_refresh("review-1").unwrap_err();
    lifecycle.begin_refresh("review-2").unwrap();

    assert!(matches!(error, ReviewLifecycleError::Busy));
    lifecycle.finish_refresh("review-1");
    lifecycle.begin_refresh("review-1").unwrap();
}

#[test]
fn close_is_logical_success_when_workspace_cleanup_must_be_deferred() {
    let fixture = LifecycleFixture::new();
    let lifecycle = ReviewLifecycle::new(fixture.catalog.clone());
    let saved = fixture.save_current_review();
    fixture.catalog.activate_snapshot(&saved).unwrap();
    let presentation = lifecycle
        .present(
            PresentReview::Saved {
                review_id: saved.review_id.clone(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let workspace = fixture
        .catalog
        .review_workspace_path(&saved.review_id)
        .unwrap();
    Command::new("git")
        .args(["worktree", "remove", "--force", "--"])
        .arg(&workspace)
        .current_dir(fixture.repository.path())
        .status()
        .unwrap();
    std::fs::create_dir_all(&workspace).unwrap();

    let outcome = lifecycle.close(&saved.review_id).unwrap();

    assert!(matches!(
        outcome.workspace_cleanup,
        WorkspaceCleanup::Deferred(_)
    ));
    let error = lifecycle
        .read_file_change(
            &saved.review_id,
            &presentation.snapshot_id,
            "README.md",
            &CancellationToken::default(),
        )
        .unwrap_err();
    assert_eq!(error.to_string(), "Review 已关闭或失效，请重新打开。");
}

#[test]
fn recovery_abandons_unactivated_candidate_without_adding_history() {
    let fixture = LifecycleFixture::new();
    let saved = fixture.save_current_review();
    fixture.catalog.activate_snapshot(&saved).unwrap();
    let candidate = fixture
        .catalog
        .save_refresh_snapshot(&saved.review_id, &saved.snapshot())
        .unwrap();
    let lifecycle = ReviewLifecycle::new(fixture.catalog.clone());

    let report = lifecycle.recover().unwrap();

    assert_eq!(
        report.abandoned_snapshot_ids,
        vec![candidate.snapshot_identity()]
    );
    assert_eq!(
        fixture
            .catalog
            .list_review_snapshot_identities(&saved.review_id)
            .unwrap(),
        vec![saved.snapshot_identity()]
    );
}

#[test]
fn branch_refresh_converges_catalog_workspace_and_runtime_reader() {
    let fixture = LifecycleFixture::new();
    let saved = fixture.save_current_review();
    fixture.catalog.activate_snapshot(&saved).unwrap();
    let lifecycle = ReviewLifecycle::new(fixture.catalog.clone());
    let current = lifecycle
        .present(
            PresentReview::Saved {
                review_id: saved.review_id.clone(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    run_git(fixture.repository.path(), &["checkout", "-q", "topic"]);
    std::fs::write(fixture.repository.path().join("README.md"), "refreshed\n").unwrap();
    run_git(
        fixture.repository.path(),
        &["commit", "-q", "-am", "refresh"],
    );
    let update = lifecycle
        .check_update(
            &saved.review_id,
            UpdateSource::Branch,
            &CancellationToken::default(),
        )
        .unwrap();

    let refreshed = lifecycle
        .refresh(
            &saved.review_id,
            RefreshEvidence::Branch {
                current_base_revision: update.current_base_revision,
                current_head_revision: update.current_head_revision,
                latest_base_revision: update.available_base_revision,
                latest_head_revision: update.available_head_revision,
            },
            &CancellationToken::default(),
        )
        .unwrap();

    assert_ne!(refreshed.snapshot_id, current.snapshot_id);
    assert_eq!(
        fixture
            .catalog
            .open_review(&saved.review_id)
            .unwrap()
            .snapshot_identity(),
        refreshed.snapshot_id
    );
    let workspace = fixture
        .catalog
        .review_workspace_path(&saved.review_id)
        .unwrap();
    assert_eq!(
        git_text(&workspace, &["rev-parse", "HEAD"]),
        refreshed.snapshot.head_revision
    );
    let stale_error = lifecycle
        .read_file_change(
            &saved.review_id,
            &current.snapshot_id,
            "README.md",
            &CancellationToken::default(),
        )
        .unwrap_err();
    assert!(matches!(stale_error, ReviewLifecycleError::StaleSnapshot));
    let content = lifecycle
        .read_file_change(
            &saved.review_id,
            &refreshed.snapshot_id,
            "README.md",
            &CancellationToken::default(),
        )
        .unwrap();
    assert_eq!(content.after.as_deref(), Some("refreshed\n"));
}

#[test]
fn cancelled_update_check_keeps_current_snapshot_and_workspace() {
    let fixture = LifecycleFixture::new();
    let saved = fixture.save_current_review();
    fixture.catalog.activate_snapshot(&saved).unwrap();
    let lifecycle = ReviewLifecycle::new(fixture.catalog.clone());
    let current = lifecycle
        .present(
            PresentReview::Saved {
                review_id: saved.review_id.clone(),
            },
            &CancellationToken::default(),
        )
        .unwrap();
    let cancellation = CancellationToken::default();
    cancellation.cancel();

    let error = lifecycle
        .check_update(&saved.review_id, UpdateSource::Branch, &cancellation)
        .unwrap_err();

    assert!(matches!(error, ReviewLifecycleError::Cancelled(_)));
    assert_eq!(
        fixture
            .catalog
            .open_review(&saved.review_id)
            .unwrap()
            .snapshot_identity(),
        current.snapshot_id
    );
    let workspace = fixture
        .catalog
        .review_workspace_path(&saved.review_id)
        .unwrap();
    assert_eq!(
        git_text(&workspace, &["rev-parse", "HEAD"]),
        current.snapshot.head_revision
    );
}

#[test]
fn failed_target_materialization_keeps_current_review_readable() {
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
    let first_saved = save_current_review(&catalog, first_entry.id, first_repository.path());
    let target_saved = save_current_review(&catalog, target_entry.id, target_repository.path());
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
        .unwrap();
    std::fs::remove_dir_all(target_repository.path()).unwrap();

    let error = match lifecycle.present(
        PresentReview::Saved {
            review_id: target_saved.review_id,
        },
        &CancellationToken::default(),
    ) {
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
        .unwrap();
    assert_eq!(content.path, "README.md");
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
    fn save_current_review(&self) -> SavedBranchReview {
        save_current_review(
            &self.catalog,
            self.repository_entry_id,
            self.repository.path(),
        )
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
fn save_current_review(
    catalog: &RepositoryCatalog,
    repository_entry_id: i64,
    repository_path: &Path,
) -> SavedBranchReview {
    let opened =
        LocalOpenedReview::open(repository_path, "refs/heads/base", "refs/heads/topic").unwrap();
    catalog
        .save_branch_review(repository_entry_id, opened.snapshot())
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
