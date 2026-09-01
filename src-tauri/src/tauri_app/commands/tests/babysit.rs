use super::*;
use crate::application_settings::{
    ApplicationLanguage, ApplicationSettingsApplication, BabysitProvider,
};
use crate::babysit::{FakeBabysitProvider, TestBabysitApplication};
use crate::operation::OperationContext;
use crate::persistence::{PersistedReviewState, ReviewStore};
use crate::repository::{verify_repository_location, RepositoryApplication};
use crate::review::{
    BranchSelection, BranchSelectionResolver, PresentReview, PullRequestSelectionResolver,
    ReviewLifecycle,
};
use crate::review::{FileChangeContent, FileChangeStatus, FileContent};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tauri::Manager;

/// 构造一个不应触发 Codex runtime 的文档 File Change。
fn markdown_change() -> FileChangeContent {
    FileChangeContent {
        path: "README.md".to_owned(),
        old_path: None,
        status: FileChangeStatus::Modified,
        before: Some(FileContent::Text {
            size: 6,
            text: "# old\n".to_owned(),
        }),
        after: Some(FileContent::Text {
            size: 6,
            text: "# new\n".to_owned(),
        }),
    }
}

/// 创建一个带 TypeScript File Change 的已授权 Review，供 command seam 真实读取 Workspace。
async fn command_review_fixture() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    Arc<ReviewStore>,
    PersistedReviewState,
) {
    let application_data = tempfile::tempdir().unwrap();
    let repository = tempfile::tempdir().unwrap();
    run_git(repository.path(), &["init", "-q"]);
    run_git(
        repository.path(),
        &["config", "user.email", "test@narrant.invalid"],
    );
    run_git(repository.path(), &["config", "user.name", "Narrant Test"]);
    std::fs::create_dir_all(repository.path().join("src")).unwrap();
    std::fs::write(
        repository.path().join("src/example.ts"),
        "const value = 1;\n",
    )
    .unwrap();
    run_git(repository.path(), &["add", "src/example.ts"]);
    run_git(repository.path(), &["commit", "-q", "-m", "base"]);
    run_git(repository.path(), &["branch", "base"]);
    std::fs::write(
        repository.path().join("src/example.ts"),
        "const value = 2;\n",
    )
    .unwrap();
    run_git(repository.path(), &["commit", "-q", "-am", "head"]);
    run_git(repository.path(), &["branch", "topic"]);

    let store =
        Arc::new(ReviewStore::open(&application_data.path().join("store.sqlite3")).unwrap());
    let entry = store
        .upsert_repository(&verify_repository_location(repository.path()).unwrap())
        .unwrap();
    store.insert_workspace_trust(entry.id).unwrap();
    let resolved = BranchSelectionResolver
        .resolve(
            &OperationContext::background(),
            repository.path(),
            &BranchSelection {
                base_ref: "refs/heads/base".to_owned(),
                head_ref: "refs/heads/topic".to_owned(),
            },
        )
        .await
        .unwrap();
    let saved = store
        .save_branch_review(entry.id, &resolved.comparison)
        .unwrap();
    store.activate_initial_snapshot(&saved).unwrap();
    (application_data, repository, store, saved)
}

/// 执行 command seam 所需的真实 Git 命令；失败时立即让 fixture 暴露原因。
fn run_git(repository: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .status()
        .unwrap();
    assert!(status.success(), "git {arguments:?} failed");
}

/// 通过 Tauri mock IPC 调用 explain command，故意不携带 locale 字段。
fn invoke_explain(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    saved: &PersistedReviewState,
    operation_id: &str,
) -> crate::babysit::BabysitFileExplanations {
    let payload = serde_json::json!({
        "operationId": operation_id,
        "reviewId": saved.review_id,
        "snapshotId": saved.snapshot_identity(),
        "path": "src/example.ts",
        "detectedLanguage": "typescript",
        "generationPolicy": "reuse_cached",
        "progress": "__CHANNEL__:0",
    });
    let response = tauri::test::get_ipc_response(
        webview,
        tauri::webview::InvokeRequest {
            cmd: "explain_review_file_change".into(),
            callback: tauri::ipc::CallbackFn(1),
            error: tauri::ipc::CallbackFn(2),
            url: "tauri://localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(payload),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_owned(),
        },
    )
    .expect("explain_review_file_change command should resolve");
    response
        .deserialize()
        .expect("command response should match provider-independent explanations")
}

#[test]
fn explain_gate_rejects_ineligible_file_before_runtime_io() {
    let directory = tempfile::tempdir().unwrap();
    let settings = directory.path().join("babysit-settings.json");
    let babysit = BabysitApplication::with_installed_provider_registry_at(&settings);

    let error =
        validate_explain_input(&babysit, "README.md", &markdown_change(), None).unwrap_err();

    assert_eq!(
        error.code,
        crate::babysit::BabysitErrorCode::UnsupportedFile
    );
    assert!(!settings.exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn explain_command_uses_application_language_without_a_locale_argument() {
    let (application_data, _repository, store, saved) = command_review_fixture().await;
    let settings_path = application_data.path().join("settings.json");
    let settings =
        ApplicationSettingsApplication::with_locale_resolver(settings_path.clone(), || {
            "en-US".to_owned()
        });
    settings
        .update_language(ApplicationLanguage::SimplifiedChinese)
        .unwrap();
    settings.update_provider(BabysitProvider::Codex).unwrap();
    let provider = Arc::new(FakeBabysitProvider::default());
    let babysit = TestBabysitApplication::with_providers_at(
        settings_path,
        [(BabysitProvider::Codex, provider.clone())],
    );
    let lifecycle = ReviewLifecycle::with_repository_application(
        store.clone(),
        Arc::new(RepositoryApplication::new(store)),
        Arc::new(PullRequestSelectionResolver::anonymous(
            "https://api.github.com",
        )),
    );
    lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: saved.review_id.clone(),
            },
        )
        .await
        .unwrap();
    let operations = crate::operation::CancellableOperations::default();
    let app = tauri::test::mock_builder()
        .manage(lifecycle)
        .manage(babysit)
        .manage(operations)
        .invoke_handler(tauri::generate_handler![explain_review_file_change])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();

    let chinese = invoke_explain(&webview, &saved, "language-command-zh");
    assert!(chinese.after["1"].starts_with("解释"));

    settings
        .update_language(ApplicationLanguage::English)
        .unwrap();
    let english = invoke_explain(&webview, &saved, "language-command-en");
    assert!(english.after["1"].starts_with("Explains"));
    assert_eq!(provider.generate_calls(), 2);

    app.state::<ReviewLifecycle>().shutdown().await.unwrap();
}
