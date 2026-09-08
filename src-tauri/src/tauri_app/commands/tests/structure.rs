use super::*;
use crate::operation::{OperationContext, OperationId};
use crate::persistence::{PersistedReviewState, ReviewStore};
use crate::repository::verify_repository_location;
use crate::review::{BranchSelection, BranchSelectionResolver, PresentReview};
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use tauri::Manager;

/// 创建含真实变更和未修改上下文的固定 Review，供 Tauri IPC 全链路测试。
async fn structure_command_fixture() -> (
    tempfile::TempDir,
    tempfile::TempDir,
    ReviewLifecycle,
    PersistedReviewState,
) {
    let data = tempfile::tempdir().unwrap();
    let repository = tempfile::tempdir().unwrap();
    for arguments in [
        vec!["init", "-q"],
        vec!["config", "user.email", "test@narrant.invalid"],
        vec!["config", "user.name", "Narrant Test"],
    ] {
        command_git(repository.path(), &arguments);
    }
    std::fs::write(
        repository.path().join("context.ts"),
        "export const stable = 7;\n",
    )
    .unwrap();
    std::fs::write(
        repository.path().join("example.ts"),
        "export function greet() { return 1; }\n",
    )
    .unwrap();
    command_git(repository.path(), &["add", "."]);
    command_git(repository.path(), &["commit", "-q", "-m", "base"]);
    command_git(repository.path(), &["branch", "base"]);
    std::fs::write(
        repository.path().join("example.ts"),
        "export function greet() { return 2; }\n",
    )
    .unwrap();
    command_git(repository.path(), &["commit", "-q", "-am", "head"]);
    command_git(repository.path(), &["branch", "topic"]);
    let store = Arc::new(ReviewStore::open(&data.path().join("store.sqlite3")).unwrap());
    let registered = store
        .upsert_repository(&verify_repository_location(repository.path()).unwrap())
        .unwrap();
    store.insert_workspace_trust(registered.id).unwrap();
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
        .save_branch_review(registered.id, &resolved.comparison)
        .unwrap();
    store.activate_initial_snapshot(&saved).unwrap();
    let lifecycle = ReviewLifecycle::new(store);
    lifecycle
        .present(
            &OperationContext::background(),
            PresentReview::Saved {
                review_id: saved.review_id.clone(),
            },
        )
        .await
        .unwrap();
    (data, repository, lifecycle, saved)
}

/// 在临时仓库执行真实 Git fixture 命令，失败时报告具体参数。
fn command_git(repository: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .status()
        .unwrap();
    assert!(status.success(), "git {arguments:?} failed");
}

/// 通过真实 Tauri command 分发和 JSON 解码路径调用结构能力。
fn invoke_structure_command(
    webview: &tauri::WebviewWindow<tauri::test::MockRuntime>,
    name: &str,
    payload: serde_json::Value,
) -> Result<serde_json::Value, serde_json::Value> {
    tauri::test::get_ipc_response(
        webview,
        tauri::webview::InvokeRequest {
            cmd: name.to_owned(),
            callback: tauri::ipc::CallbackFn(1),
            error: tauri::ipc::CallbackFn(2),
            url: "tauri://localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::Json(payload),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_owned(),
        },
    )
    .map(|response| response.deserialize().expect("结构 IPC 必须返回稳定 JSON"))
}

/// 验证 IPC 可在分析前打开未改全文，并返回绑定同一 Snapshot 的 owned 结构结果。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structure_commands_read_context_before_analysis_and_preserve_snapshot_identity() {
    let (_data, _repository, lifecycle, saved) = structure_command_fixture().await;
    let app = tauri::test::mock_builder()
        .manage(lifecycle)
        .manage(operation::CancellableOperations::default())
        .invoke_handler(tauri::generate_handler![
            analyze_review_structure,
            read_review_structure_source
        ])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let source = invoke_structure_command(
        &webview,
        "read_review_structure_source",
        serde_json::json!({
            "operationId": "structure-source", "reviewId": saved.review_id,
            "snapshotId": saved.snapshot_identity(), "side": "after", "path": "context.ts",
        }),
    )
    .unwrap();
    assert_eq!(source["reviewId"], saved.review_id);
    assert_eq!(source["snapshotId"], saved.snapshot_identity());
    assert_eq!(source["source"]["classification"]["kind"], "unchanged");
    assert_eq!(source["before"]["state"], "available");
    assert_eq!(source["after"]["text"], "export const stable = 7;\n");
    assert_eq!(source["before"]["text"], source["after"]["text"]);
    let result = invoke_structure_command(
        &webview,
        "analyze_review_structure",
        serde_json::json!({
            "operationId": "structure-analysis", "reviewId": saved.review_id,
            "snapshotId": saved.snapshot_identity(), "progress": "__CHANNEL__:0",
        }),
    )
    .unwrap();
    assert_eq!(result["reviewId"], saved.review_id);
    assert_eq!(result["snapshotId"], saved.snapshot_identity());
    assert_eq!(result["status"], "completed");
    assert!(result["nodes"]
        .as_array()
        .is_some_and(|nodes| !nodes.is_empty()));
    assert!(result["coverage"]["files"].is_array());
    assert!(result["initialFocus"]["nodeIds"].is_array());
    app.state::<ReviewLifecycle>().shutdown().await.unwrap();
}

/// 两个结构 command 都遵守提前取消的操作注册语义，失效 Snapshot 也通过既有字符串错误返回。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn structure_commands_use_cancellation_registry_and_reject_stale_snapshot() {
    let (_data, _repository, lifecycle, saved) = structure_command_fixture().await;
    let operations = operation::CancellableOperations::default();
    for name in ["analysis-cancelled", "source-cancelled"] {
        operations.cancel(&OperationId::new(name).unwrap()).unwrap();
    }
    let app = tauri::test::mock_builder()
        .manage(lifecycle)
        .manage(operations)
        .invoke_handler(tauri::generate_handler![
            analyze_review_structure,
            read_review_structure_source
        ])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .unwrap();
    let webview = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    for (name, operation_id) in [
        ("analyze_review_structure", "analysis-cancelled"),
        ("read_review_structure_source", "source-cancelled"),
    ] {
        let error = invoke_structure_command(
            &webview,
            name,
            serde_json::json!({
                "operationId": operation_id, "reviewId": saved.review_id,
                "snapshotId": saved.snapshot_identity(), "progress": "__CHANNEL__:0",
                "side": "after", "path": "context.ts",
            }),
        )
        .unwrap_err();
        assert_eq!(error, "操作已取消。");
    }
    let error = invoke_structure_command(
        &webview,
        "read_review_structure_source",
        serde_json::json!({
            "operationId": "stale-source", "reviewId": saved.review_id,
            "snapshotId": "snapshot-old", "side": "after", "path": "context.ts",
        }),
    )
    .unwrap_err();
    assert!(error
        .as_str()
        .is_some_and(|message| message.contains("Snapshot identity")));
    app.state::<ReviewLifecycle>().shutdown().await.unwrap();
}
