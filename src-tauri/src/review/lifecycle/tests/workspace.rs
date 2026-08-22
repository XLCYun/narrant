/// 验证 shutdown 等待正在物化的 Review 收敛，并拒绝随后到达的新 present。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn shutdown_waits_for_in_flight_materialization_before_workspace_cleanup() {
    let fixture = LifecycleFixture::new();
    let saved = fixture.save_current_review().await;
    fixture.store.activate_initial_snapshot(&saved).unwrap();
    let lifecycle = Arc::new(ReviewLifecycle::new(fixture.store.clone()));
    let barrier = Arc::new(LifecycleOperationGateBarrier::new());
    lifecycle
        .install_operation_gate_barrier(barrier.clone())
        .unwrap();
    let workspace = fixture
        .store
        .review_workspace_path(&saved.review_id)
        .unwrap();
    let present_lifecycle = lifecycle.clone();
    let present_review_id = saved.review_id.clone();
    let present_task = tokio::spawn(async move {
        present_lifecycle
            .present(
                &OperationContext::background(),
                PresentReview::Saved {
                    review_id: present_review_id,
                },
            )
            .await
    });
    barrier.wait_until_operation_paused().await;

    let shutdown_lifecycle = lifecycle.clone();
    let shutdown_task = tokio::spawn(async move { shutdown_lifecycle.shutdown().await });
    while !lifecycle.is_shutting_down() {
        tokio::task::yield_now().await;
    }
    assert!(
        !shutdown_task.is_finished(),
        "present 持有 read gate 时 shutdown 不得越过 write gate"
    );
    let rejected = match lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: saved.review_id.clone(),
            },
        )
        .await
    {
        Ok(_) => panic!("shutdown 开始后不应接受新的 present"),
        Err(error) => error,
    };
    assert!(matches!(rejected, ReviewLifecycleError::ShuttingDown));
    barrier.release_operation().await;
    let opened = present_task.await.unwrap().unwrap();
    let warnings = shutdown_task.await.unwrap().unwrap();

    assert_eq!(opened.snapshot_id, saved.snapshot_identity());
    assert!(warnings.is_empty());
    assert!(!workspace.exists());
    assert!(lifecycle
        .open_review_runtime(&saved.review_id)
        .unwrap()
        .is_none());
}

/// 验证真实 Core Git Workspace 操作阻塞时，全局 Open Review Set 仍可读取。
#[cfg(unix)]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn core_git_workspace_block_does_not_hold_open_review_set_lock() {
    let fixture = LifecycleFixture::new();
    let first = fixture.save_current_review().await;
    let second = fixture
        .save_review_with_refs("refs/heads/topic", "refs/heads/base")
        .await;
    fixture.store.activate_initial_snapshot(&first).unwrap();
    fixture.store.activate_initial_snapshot(&second).unwrap();
    let lifecycle = Arc::new(ReviewLifecycle::new(fixture.store.clone()));
    lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: first.review_id.clone(),
            },
        )
        .await
        .unwrap();
    let first_reader = lifecycle
        .open_review_runtime(&first.review_id)
        .unwrap()
        .unwrap()
        .reader;
    let hook_entered = fixture._application_data.path().join("hook-entered");
    let hook_released = fixture._application_data.path().join("hook-released");
    install_blocking_post_checkout_hook(
        fixture.repository.path(),
        &hook_entered,
        &hook_released,
    );

    let present_lifecycle = lifecycle.clone();
    let second_review_id = second.review_id.clone();
    let present_task = tokio::spawn(async move {
        present_lifecycle
            .present(
                &OperationContext::background(),
                PresentReview::Saved {
                    review_id: second_review_id,
                },
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), async {
        while !hook_entered.exists() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("post-checkout hook 未进入阻塞状态");

    let access_lifecycle = lifecycle.clone();
    let first_review_id = first.review_id.clone();
    let runtime_access = tokio::time::timeout(
        Duration::from_secs(1),
        tokio::task::spawn_blocking(move || {
            access_lifecycle.open_review_runtime(&first_review_id)
        }),
    )
    .await;
    std::fs::write(&hook_released, "release\n").unwrap();
    let second_presentation = present_task.await.unwrap().unwrap();
    let runtime_access = runtime_access
        .expect("Core Git 阻塞期间全局 Open Review Set 锁不可获取")
        .unwrap()
        .unwrap()
        .unwrap();

    assert!(Arc::ptr_eq(&first_reader, &runtime_access.reader));
    assert_eq!(second_presentation.review_id, second.review_id);
    for review in [&first, &second] {
        assert!(fixture
            .store
            .review_workspace_path(&review.review_id)
            .unwrap()
            .exists());
        assert!(lifecycle
            .open_review_runtime(&review.review_id)
            .unwrap()
            .is_some());
    }
}

/// 验证正常退出释放全部 Open Review Reader 与受管 Workspace。
#[tokio::test]
async fn shutdown_releases_all_retained_review_workspaces_and_readers() {
    let fixture = LifecycleFixture::new();
    let first = fixture.save_current_review().await;
    let second = fixture
        .save_review_with_refs("refs/heads/topic", "refs/heads/base")
        .await;
    fixture.store.activate_initial_snapshot(&first).unwrap();
    fixture.store.activate_initial_snapshot(&second).unwrap();
    let lifecycle = ReviewLifecycle::new(fixture.store.clone());
    for review in [&first, &second] {
        lifecycle
            .present(
                &OperationContext::background(),
                PresentReview::Saved {
                    review_id: review.review_id.clone(),
                },
            )
            .await
            .unwrap();
    }

    let warnings = lifecycle.shutdown().await.unwrap();

    assert!(warnings.is_empty());
    for review in [&first, &second] {
        let workspace = fixture
            .store
            .review_workspace_path(&review.review_id)
            .unwrap();
        assert!(!workspace.exists());
        assert!(lifecycle
            .open_review_runtime(&review.review_id)
            .unwrap()
            .is_none());
        let error = lifecycle
            .read_file_change(
                &OperationContext::background(),
                &review.review_id,
                &review.snapshot_identity(),
                "README.md",
            )
            .await
            .unwrap_err();
        assert!(matches!(error, ReviewLifecycleError::ReviewUnavailable(_)));
    }
}

#[tokio::test]
async fn recovery_removes_registration_for_an_unactivated_review_without_a_workspace() {
    let fixture = LifecycleFixture::new();
    let candidate = fixture.save_current_review().await;
    let workspace = fixture
        .store
        .review_workspace_path(&candidate.review_id)
        .unwrap();
    GitProcessAdapter::new(fixture.repository.path())
        .create_detached_worktree(
            &workspace,
            &candidate.head_revision,
            OperationContext::background().cancellation(),
            OperationContext::background().cancellation(),
        )
        .unwrap();
    std::fs::remove_dir_all(&workspace).unwrap();
    assert!(git_text(
        fixture.repository.path(),
        &["worktree", "list", "--porcelain"]
    )
    .contains(&workspace.to_string_lossy().to_string()));
    let lifecycle = ReviewLifecycle::new(fixture.store.clone());

    let report = lifecycle.recover().unwrap();

    assert_eq!(
        report.abandoned_snapshot_ids,
        vec![candidate.snapshot_identity()]
    );
    assert!(report.cleanup_warnings.is_empty());
    assert!(!workspace.exists());
    assert!(!git_text(
        fixture.repository.path(),
        &["worktree", "list", "--porcelain"]
    )
    .contains(&workspace.to_string_lossy().to_string()));
}

#[tokio::test]
async fn cancellation_after_materialization_starts_does_not_interrupt_convergence() {
    let fixture = LifecycleFixture::new();
    for index in 0..1_000 {
        std::fs::write(
            fixture.repository.path().join(format!("materialize-{index}.txt")),
            format!("{index}\n"),
        )
        .unwrap();
    }
    run_git(fixture.repository.path(), &["add", "."]);
    run_git(
        fixture.repository.path(),
        &["commit", "-q", "-m", "large materialization"],
    );
    run_git(fixture.repository.path(), &["branch", "-f", "topic", "HEAD"]);
    let saved = fixture.save_current_review().await;
    fixture.store.activate_initial_snapshot(&saved).unwrap();
    let workspace = fixture
        .store
        .review_workspace_path(&saved.review_id)
        .unwrap();
    let cancellation = crate::operation::TestCancellation::default();
    let operation = OperationContext::from_test_cancellation(&cancellation);
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
    let lifecycle = ReviewLifecycle::new(fixture.store.clone());

    let opened = lifecycle
        .present(
            &operation,
            PresentReview::Saved {
                review_id: saved.review_id.clone(),
            },
        )
        .await;

    assert!(observer.join().unwrap(), "未观察到 Review Workspace 开始物化");
    let opened = opened.unwrap();
    assert!(cancellation.is_cancelled());
    assert_eq!(
        git_text(&workspace, &["rev-parse", "HEAD"]),
        opened.snapshot.head_revision
    );
}

/// 验证 C 物化失败不会改变 Presented B，并保留已打开 A、B 的读取能力。
#[tokio::test]
async fn failed_target_materialization_preserves_presented_and_open_reviews() {
    let application_data = tempfile::TempDir::new().unwrap();
    let first_repository = test_repository_with_commit();
    let second_repository = test_repository_with_commit();
    let target_repository = test_repository_with_commit();
    let store = Arc::new(
        ReviewStore::open(&application_data.path().join("store.sqlite3")).unwrap(),
    );
    let repository_entries = [
        store
            .upsert_repository(&verify_repository_location(first_repository.path()).unwrap())
            .unwrap(),
        store
            .upsert_repository(&verify_repository_location(second_repository.path()).unwrap())
            .unwrap(),
        store
            .upsert_repository(&verify_repository_location(target_repository.path()).unwrap())
            .unwrap(),
    ];
    for entry in &repository_entries {
        store.insert_workspace_trust(entry.id).unwrap();
    }
    let first_entry = &repository_entries[0];
    let second_entry = &repository_entries[1];
    let target_entry = &repository_entries[2];
    let first_saved = save_current_review(&store, first_entry.id, first_repository.path()).await;
    let second_saved =
        save_current_review(&store, second_entry.id, second_repository.path()).await;
    let target_saved =
        save_current_review(&store, target_entry.id, target_repository.path()).await;
    for saved in [&first_saved, &second_saved, &target_saved] {
        store.activate_initial_snapshot(saved).unwrap();
    }
    let lifecycle = ReviewLifecycle::new(store.clone());
    let mut presentations = Vec::new();
    for saved in [&first_saved, &second_saved] {
        presentations.push(
            lifecycle
                .present(
                    &OperationContext::background(),
                    PresentReview::Saved {
                        review_id: saved.review_id.clone(),
                    },
                )
                .await
                .unwrap(),
        );
    }
    std::fs::remove_dir_all(target_repository.path()).unwrap();

    let error = match lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: target_saved.review_id.clone(),
            },
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
    assert_eq!(
        lifecycle
            .runtime
            .lock()
            .unwrap()
            .presented_review_id
            .as_deref(),
        Some(second_saved.review_id.as_str())
    );
    for (saved, presentation) in [&first_saved, &second_saved]
        .into_iter()
        .zip(&presentations)
    {
        let content = lifecycle
            .read_file_change(
                &OperationContext::background(),
                &saved.review_id,
                &presentation.snapshot_id,
                "README.md",
            )
            .await
            .unwrap();
        assert_eq!(content.path, "README.md");
        assert!(lifecycle
            .open_review_runtime(&saved.review_id)
            .unwrap()
            .is_some());
    }
    assert!(lifecycle
        .open_review_runtime(&target_saved.review_id)
        .unwrap()
        .is_none());
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
        fixture.store.clone(),
        Arc::new(PullRequestSelectionResolver::anonymous(server.base_url())),
    );

    let opened = lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::PullRequest {
                repository_id: fixture.repository_id,
                selection: PullRequestSelection {
                    repository: crate::github::GitHubRepository {
                        owner: "octo-org".to_owned(),
                        name: "octo-repo".to_owned(),
                    },
                    number: 42,
                },
            },
        )
        .await
        .unwrap();
    let update = lifecycle
        .check_update(&OperationContext::background(), &opened.review_id)
        .await
        .unwrap();
    let error = match lifecycle
        .refresh(
            &OperationContext::background(),
            &opened.review_id,
            ConfirmedReviewUpdate {
                current_base_revision: update.current_base_revision,
                current_head_revision: update.current_head_revision,
                available_base_revision: "changed-after-confirmation".to_owned(),
                available_head_revision: update.available_head_revision,
            },
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

/// 生命周期测试所需的应用目录、Persistence 与 Git repository。
struct LifecycleFixture {
    _application_data: tempfile::TempDir,
    repository: tempfile::TempDir,
    store: Arc<ReviewStore>,
    repository_id: i64,
}

impl LifecycleFixture {
    /// 创建一个已注册且已授予 Workspace Trust 的测试环境。
    fn new() -> Self {
        let application_data = tempfile::TempDir::new().unwrap();
        let repository = test_repository_with_commit();
        let store = Arc::new(
            ReviewStore::open(&application_data.path().join("store.sqlite3")).unwrap(),
        );
        let entry = store.upsert_repository(&verify_repository_location(repository.path()).unwrap()).unwrap();
        store.insert_workspace_trust(entry.id).unwrap();
        Self {
            _application_data: application_data,
            repository,
            store,
            repository_id: entry.id,
        }
    }

    /// 保存当前分支相对其父提交的测试 Review。
    async fn save_current_review(&self) -> PersistedReviewState {
        save_review_with_refs(
            &self.store,
            self.repository_id,
            self.repository.path(),
            "refs/heads/base",
            "refs/heads/topic",
        )
        .await
    }

    /// 保存指定 branch pair 的测试 Review，供多 Review 生命周期断言复用。
    async fn save_review_with_refs(
        &self,
        base_ref: &str,
        head_ref: &str,
    ) -> PersistedReviewState {
        save_review_with_refs(
            &self.store,
            self.repository_id,
            self.repository.path(),
            base_ref,
            head_ref,
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
    store: &ReviewStore,
    repository_id: i64,
    repository_path: &Path,
) -> PersistedReviewState {
    save_review_with_refs(
        store,
        repository_id,
        repository_path,
        "refs/heads/base",
        "refs/heads/topic",
    )
    .await
}

/// 保存指定 canonical branch pair 组成的测试 Review。
async fn save_review_with_refs(
    store: &ReviewStore,
    repository_id: i64,
    repository_path: &Path,
    base_ref: &str,
    head_ref: &str,
) -> PersistedReviewState {
    let resolved = BranchSelectionResolver
        .resolve(
            &OperationContext::background(),
            repository_path,
            &BranchSelection {
                base_ref: base_ref.to_owned(),
                head_ref: head_ref.to_owned(),
            },
        )
        .await
        .unwrap();
    store
        .save_branch_review(repository_id, &resolved.comparison)
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
