pub mod branch_selection;
pub mod cancellable_operation;
pub mod change_content;
pub mod core_git;
pub mod github_auth;
pub mod github_discovery;
pub mod pull_request_selection;
pub mod repository_catalog;
pub mod review_lifecycle;

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_window_state::StateFlags;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileChangeSummaryResponse {
    path: String,
    old_path: Option<String>,
    status: ChangeStatusResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    before_mode: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after_mode: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    before_object_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    after_object_id: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileChangeContentResponse {
    path: String,
    old_path: Option<String>,
    status: ChangeStatusResponse,
    content_kind: ContentKindResponse,
    before_size: Option<usize>,
    after_size: Option<usize>,
    before: Option<String>,
    after: Option<String>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum ChangeStatusResponse {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum ContentKindResponse {
    Text,
    Binary,
    Submodule,
    Symlink,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewSnapshotResponse {
    review_id: String,
    snapshot_id: String,
    base_ref: String,
    head_ref: String,
    base_revision: String,
    comparison_base: String,
    head_revision: String,
    file_changes: Vec<FileChangeSummaryResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pull_request: Option<repository_catalog::PullRequestMetadata>,
}

/// 检查 moving refs 后返回的版本差异，不携带或切换候选 File Changes。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BranchReviewUpdateResponse {
    changed: bool,
    current_base_revision: String,
    current_head_revision: String,
    latest_base_revision: String,
    latest_head_revision: String,
}

/// 用户基于检查结果明确确认的一组 current/latest revisions。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfirmedBranchUpdate {
    current_base_revision: String,
    current_head_revision: String,
    latest_base_revision: String,
    latest_head_revision: String,
}

/// Pull Request 更新确认仅固定用户刚刚看到的 current/available revisions。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConfirmedPullRequestUpdate {
    current_base_revision: String,
    current_head_revision: String,
    available_base_revision: String,
    available_head_revision: String,
}

/// 检查 Pull Request 当前 revisions 后返回，不改变 current Snapshot 或 workspace。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ReviewUpdateResponse {
    update_available: bool,
    current_base_revision: String,
    current_head_revision: String,
    available_base_revision: String,
    available_head_revision: String,
}

/// Repository Entry 页面展示的 durable Review 摘要。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SavedReviewSummaryResponse {
    review_id: String,
    base_revision: String,
    head_revision: String,
    pull_request: Option<repository_catalog::PullRequestMetadata>,
}

/// Repository Entry 本地分支标签所需的 canonical refs 与显式默认选择。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BranchRefsResponse {
    refs: Vec<String>,
    default_ref: String,
    current_ref: String,
}

fn map_status(status: change_content::ChangeStatus) -> ChangeStatusResponse {
    match status {
        change_content::ChangeStatus::Added => ChangeStatusResponse::Added,
        change_content::ChangeStatus::Modified => ChangeStatusResponse::Modified,
        change_content::ChangeStatus::Deleted => ChangeStatusResponse::Deleted,
        change_content::ChangeStatus::Renamed => ChangeStatusResponse::Renamed,
    }
}

fn map_content_kind(kind: change_content::ContentKind) -> ContentKindResponse {
    match kind {
        change_content::ContentKind::Text => ContentKindResponse::Text,
        change_content::ContentKind::Binary => ContentKindResponse::Binary,
        change_content::ContentKind::Submodule => ContentKindResponse::Submodule,
        change_content::ContentKind::Symlink => ContentKindResponse::Symlink,
    }
}

fn map_file_content(change: change_content::LoadedFileChange) -> FileChangeContentResponse {
    FileChangeContentResponse {
        path: change.path,
        old_path: change.old_path,
        status: map_status(change.status),
        content_kind: map_content_kind(change.content_kind),
        before_size: change.before_size,
        after_size: change.after_size,
        before: change.before,
        after: change.after,
    }
}

/// 将内部固定 Snapshot 映射为不区分来源的前端 IPC 契约。
fn map_snapshot(
    review_id: String,
    snapshot_id: String,
    snapshot: &change_content::local::LocalSnapshot,
) -> ReviewSnapshotResponse {
    ReviewSnapshotResponse {
        review_id,
        snapshot_id,
        base_ref: snapshot.base_ref.clone(),
        head_ref: snapshot.head_ref.clone(),
        base_revision: snapshot.base_revision.clone(),
        comparison_base: snapshot.comparison_base.clone(),
        head_revision: snapshot.head_revision.clone(),
        file_changes: snapshot
            .file_changes
            .iter()
            .map(|change| FileChangeSummaryResponse {
                path: change.path.clone(),
                old_path: change.old_path.clone(),
                status: map_status(change.status),
                before_mode: change.before_mode,
                after_mode: change.after_mode,
                before_object_id: change.before_object_id.clone(),
                after_object_id: change.after_object_id.clone(),
            })
            .collect(),
        pull_request: None,
    }
}

/// 将生命周期输出映射为既有 Tauri IPC 契约。
fn map_presentation(presentation: review_lifecycle::ReviewPresentation) -> ReviewSnapshotResponse {
    let mut response = map_snapshot(
        presentation.review_id,
        presentation.snapshot_id,
        &presentation.snapshot,
    );
    response.pull_request = presentation.pull_request;
    response
}

/// 将 Tauri 的字符串 operation identity 与错误契约适配到可取消操作 seam。
async fn run_cancellable_command<T, F, Fut>(
    operations: &cancellable_operation::CancellableOperations,
    operation_id: String,
    operation: F,
) -> Result<T, String>
where
    F: FnOnce(cancellable_operation::OperationContext) -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let id =
        cancellable_operation::OperationId::new(operation_id).map_err(|error| error.to_string())?;
    operations
        .run(id, operation)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
async fn compare_local_refs(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    operations: tauri::State<'_, cancellable_operation::CancellableOperations>,
    operation_id: String,
    repository_entry_id: i64,
    base_ref: String,
    head_ref: String,
) -> Result<ReviewSnapshotResponse, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .present(
                review_lifecycle::PresentReview::Branch {
                    repository_entry_id,
                    base_ref,
                    head_ref,
                },
                operation.cancellation(),
            )
            .await
            .map(map_presentation)
            .map_err(|error| error.to_string())
    })
    .await
}

/// 重新获取所选 remote refs 并解析最新版本，但保持 current Snapshot 与 Workspace 不变。
#[tauri::command]
async fn check_branch_review_updates(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    operations: tauri::State<'_, cancellable_operation::CancellableOperations>,
    operation_id: String,
    review_id: String,
) -> Result<BranchReviewUpdateResponse, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .check_update(&review_id, operation.cancellation())
            .await
            .map(|update| BranchReviewUpdateResponse {
                changed: update.update_available,
                current_base_revision: update.current_base_revision,
                current_head_revision: update.current_head_revision,
                latest_base_revision: update.available_base_revision,
                latest_head_revision: update.available_head_revision,
            })
            .map_err(|error| error.to_string())
    })
    .await
}

/// 在用户明确确认后创建 Snapshot、重建 Workspace，并以 CAS 推进 current 指针。
#[tauri::command]
async fn refresh_branch_review(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    operations: tauri::State<'_, cancellable_operation::CancellableOperations>,
    operation_id: String,
    review_id: String,
    confirmation: ConfirmedBranchUpdate,
) -> Result<ReviewSnapshotResponse, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .refresh(
                &review_id,
                review_lifecycle::ConfirmedReviewUpdate {
                    current_base_revision: confirmation.current_base_revision,
                    current_head_revision: confirmation.current_head_revision,
                    available_base_revision: confirmation.latest_base_revision,
                    available_head_revision: confirmation.latest_head_revision,
                },
                operation.cancellation(),
            )
            .await
            .map(map_presentation)
            .map_err(|error| error.to_string())
    })
    .await
}

/// 原子承接 Branch Selection：重开 durable Review，并使用既有确认刷新。
#[tauri::command]
async fn reopen_and_refresh_branch_review(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    operations: tauri::State<'_, cancellable_operation::CancellableOperations>,
    operation_id: String,
    review_id: String,
    confirmation: ConfirmedBranchUpdate,
) -> Result<ReviewSnapshotResponse, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .reopen_and_refresh(
                &review_id,
                review_lifecycle::ConfirmedReviewUpdate {
                    current_base_revision: confirmation.current_base_revision,
                    current_head_revision: confirmation.current_head_revision,
                    available_base_revision: confirmation.latest_base_revision,
                    available_head_revision: confirmation.latest_head_revision,
                },
                operation.cancellation(),
            )
            .await
            .map(map_presentation)
            .map_err(|error| error.to_string())
    })
    .await
}

/// 从稳定 Selection identity 创建或重开 durable Pull Request Review。
#[tauri::command]
async fn open_pull_request_review(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    operations: tauri::State<'_, cancellable_operation::CancellableOperations>,
    operation_id: String,
    repository_entry_id: i64,
    repository: github_discovery::GitHubRepository,
    number: u64,
) -> Result<ReviewSnapshotResponse, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .present(
                review_lifecycle::PresentReview::PullRequest {
                    repository_entry_id,
                    selection: pull_request_selection::PullRequestSelection { repository, number },
                },
                operation.cancellation(),
            )
            .await
            .map(map_presentation)
            .map_err(|error| error.to_string())
    })
    .await
}

/// 检查 Pull Request 当前 revisions；只获取对象并计算候选 Snapshot。
#[tauri::command]
async fn check_pull_request_review_update(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    operations: tauri::State<'_, cancellable_operation::CancellableOperations>,
    operation_id: String,
    review_id: String,
) -> Result<ReviewUpdateResponse, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .check_update(&review_id, operation.cancellation())
            .await
            .map(|update| ReviewUpdateResponse {
                update_available: update.update_available,
                current_base_revision: update.current_base_revision,
                current_head_revision: update.current_head_revision,
                available_base_revision: update.available_base_revision,
                available_head_revision: update.available_head_revision,
            })
            .map_err(|error| error.to_string())
    })
    .await
}

/// 显式刷新 Pull Request Review，并在 workspace 对齐后推进 current Snapshot。
#[tauri::command]
async fn refresh_pull_request_review(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    operations: tauri::State<'_, cancellable_operation::CancellableOperations>,
    operation_id: String,
    review_id: String,
    confirmation: ConfirmedPullRequestUpdate,
) -> Result<ReviewSnapshotResponse, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .refresh(
                &review_id,
                review_lifecycle::ConfirmedReviewUpdate {
                    current_base_revision: confirmation.current_base_revision,
                    current_head_revision: confirmation.current_head_revision,
                    available_base_revision: confirmation.available_base_revision,
                    available_head_revision: confirmation.available_head_revision,
                },
                operation.cancellation(),
            )
            .await
            .map(map_presentation)
            .map_err(|error| error.to_string())
    })
    .await
}

/// 列出 Repository Entry 保存的 Reviews；不访问 GitHub，也不创建 workspace。
#[tauri::command]
fn list_saved_reviews(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    repository_entry_id: i64,
) -> Result<Vec<SavedReviewSummaryResponse>, String> {
    require_available_repository(lifecycle.catalog(), repository_entry_id)?;
    lifecycle
        .catalog()
        .list_saved_review_snapshots(repository_entry_id)
        .map_err(|error| error.to_string())
        .map(|reviews| {
            reviews
                .into_iter()
                .map(|review| SavedReviewSummaryResponse {
                    review_id: review.review_id,
                    base_revision: review.base_revision,
                    head_revision: review.head_revision,
                    pull_request: review.pull_request,
                })
                .collect()
        })
}

/// 仅用 cached current Snapshot 重开 durable Review，不依赖 GitHub 登录或 discovery。
#[tauri::command]
async fn open_saved_review(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    review_id: String,
) -> Result<ReviewSnapshotResponse, String> {
    lifecycle
        .present(
            review_lifecycle::PresentReview::Saved { review_id },
            &core_git::CancellationToken::default(),
        )
        .await
        .map(map_presentation)
        .map_err(|error| error.to_string())
}

#[tauri::command]
async fn list_local_refs(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    operations: tauri::State<'_, cancellable_operation::CancellableOperations>,
    operation_id: String,
    repository_entry_id: i64,
) -> Result<BranchRefsResponse, String> {
    let repository_path = require_available_repository(lifecycle.catalog(), repository_entry_id)?;
    run_cancellable_command(&operations, operation_id, |operation| async move {
        branch_selection::BranchRefCatalog
            .list(&repository_path, &operation)
            .await
            .map(|branches| BranchRefsResponse {
                refs: branches.refs,
                default_ref: branches.default_ref,
                current_ref: branches.current_ref,
            })
            .map_err(|error| error.to_string())
    })
    .await
}

#[tauri::command]
fn cancel_operation(
    operations: tauri::State<'_, cancellable_operation::CancellableOperations>,
    operation_id: String,
) -> Result<(), String> {
    let id =
        cancellable_operation::OperationId::new(operation_id).map_err(|error| error.to_string())?;
    operations.cancel(&id).map_err(|error| error.to_string())
}

fn github_api_base_url() -> String {
    #[cfg(feature = "wdio")]
    {
        std::env::var("NARRANT_GITHUB_API_URL")
            .unwrap_or_else(|_| "https://api.github.com".to_owned())
    }
    #[cfg(not(feature = "wdio"))]
    {
        "https://api.github.com".to_owned()
    }
}

/// 为 Repository Entry 的全部 GitHub remotes 独立发现 Pull Requests。
#[tauri::command]
async fn discover_github_pull_requests(
    auth: tauri::State<'_, Arc<github_auth::GitHubAuth>>,
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    repository_entry_id: i64,
) -> Result<Vec<github_discovery::PullRequestGroup>, String> {
    let repository_path = require_available_repository(lifecycle.catalog(), repository_entry_id)?;
    let access_token = auth.access_token().await?;
    let api_base_url = github_api_base_url();
    github_discovery::discover_pull_requests(&repository_path, &api_base_url, access_token).await
}

#[tauri::command]
async fn github_auth_status(
    auth: tauri::State<'_, Arc<github_auth::GitHubAuth>>,
) -> Result<github_auth::AuthorizationStatus, String> {
    auth.status().await
}

/// 在不改变登录状态的情况下刷新 GitHub 公开身份。
#[tauri::command]
async fn refresh_github_identity(
    auth: tauri::State<'_, Arc<github_auth::GitHubAuth>>,
) -> Result<github_auth::GitHubIdentity, String> {
    auth.refresh_identity().await
}

#[tauri::command]
async fn start_github_authorization(
    auth: tauri::State<'_, Arc<github_auth::GitHubAuth>>,
) -> Result<github_auth::DeviceAuthorization, String> {
    auth.start_device_authorization().await
}

#[tauri::command]
async fn complete_github_authorization(
    auth: tauri::State<'_, Arc<github_auth::GitHubAuth>>,
    authorization_id: String,
) -> Result<(), String> {
    auth.complete_device_authorization(&authorization_id).await
}

#[tauri::command]
async fn sign_out_github(
    auth: tauri::State<'_, Arc<github_auth::GitHubAuth>>,
) -> Result<(), String> {
    auth.sign_out().await
}

/// 通过 durable reviewId 从 current Snapshot 读取本地 File Change。
#[tauri::command]
async fn read_review_file_change(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    operations: tauri::State<'_, cancellable_operation::CancellableOperations>,
    operation_id: String,
    review_id: String,
    snapshot_id: String,
    path: String,
) -> Result<FileChangeContentResponse, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .read_file_change(&review_id, &snapshot_id, &path, operation.cancellation())
            .await
            .map(map_file_content)
            .map_err(|error| error.to_string())
    })
    .await
}

/// 关闭 durable Review 并精确移除它的 disposable linked worktree。
#[tauri::command]
fn close_review(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    review_id: String,
) -> Result<(), String> {
    lifecycle
        .close(&review_id)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// 查询一个 Repository Entry 的持久化 Workspace Trust。
#[tauri::command]
fn repository_workspace_trust(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    repository_entry_id: i64,
) -> Result<bool, String> {
    lifecycle
        .catalog()
        .is_workspace_trusted(repository_entry_id)
        .map_err(|error| error.to_string())
}

/// 在用户确认风险说明后授予 Repository Entry 级 Workspace Trust。
#[tauri::command]
fn grant_repository_workspace_trust(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    repository_entry_id: i64,
) -> Result<(), String> {
    lifecycle
        .catalog()
        .grant_workspace_trust(repository_entry_id)
        .map_err(|error| error.to_string())
}

/// 注册一个经 Core Git 验证的 Repository Entry。
#[tauri::command]
fn register_repository_entry(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    path: String,
) -> Result<repository_catalog::RepositoryEntry, String> {
    lifecycle
        .catalog()
        .register(std::path::Path::new(&path))
        .map_err(|error| error.to_string())
}

/// 列出应用数据库中的全部 Repository Entries。
#[tauri::command]
fn list_repository_entries(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
) -> Result<Vec<repository_catalog::RepositoryEntry>, String> {
    lifecycle
        .catalog()
        .list()
        .map_err(|error| error.to_string())
}

/// 列出一个 Repository Entry 的 Reviews；空 identity 表示全局最近 Reviews。
#[tauri::command]
fn list_reviews(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    repository_entry_id: Option<i64>,
) -> Result<Vec<repository_catalog::ReviewSummary>, String> {
    lifecycle
        .catalog()
        .list_reviews(repository_entry_id)
        .map_err(|error| error.to_string())
}

/// 使用 current Snapshot 按 durable reviewId 重新物化 Review。
#[tauri::command]
async fn reopen_review(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    review_id: String,
) -> Result<ReviewSnapshotResponse, String> {
    lifecycle
        .present(
            review_lifecycle::PresentReview::Saved { review_id },
            &core_git::CancellationToken::default(),
        )
        .await
        .map(map_presentation)
        .map_err(|error| error.to_string())
}

/// 打开 Repository Entry 并更新其最后打开时间。
#[tauri::command]
fn open_repository_entry(
    lifecycle: tauri::State<'_, review_lifecycle::ReviewLifecycle>,
    id: i64,
) -> Result<repository_catalog::RepositoryEntry, String> {
    lifecycle
        .catalog()
        .open_entry(id)
        .map_err(|error| error.to_string())
}

/// 将 durable Repository Entry identity 解析为当前仍可用的保存路径。
fn require_available_repository(
    catalog: &repository_catalog::RepositoryCatalog,
    id: i64,
) -> Result<std::path::PathBuf, String> {
    let entry = catalog.open_entry(id).map_err(|error| error.to_string())?;
    if entry.available {
        Ok(entry.canonical_path)
    } else {
        Err(entry
            .unavailable_reason
            .unwrap_or_else(|| "Repository Entry 当前不可用。".to_owned()))
    }
}

pub fn run() {
    #[cfg(feature = "wdio")]
    let oauth_base_url = std::env::var("NARRANT_GITHUB_OAUTH_URL")
        .unwrap_or_else(|_| "https://github.com".to_owned());
    #[cfg(not(feature = "wdio"))]
    let oauth_base_url = "https://github.com".to_owned();
    let builder = tauri::Builder::default()
        .manage(cancellable_operation::CancellableOperations::default())
        .setup(move |app| {
            let application_data = app.path().app_data_dir()?;
            #[cfg(feature = "wdio")]
            let database_path = std::env::var_os("NARRANT_DATABASE_PATH")
                .map(std::path::PathBuf::from)
                .unwrap_or(application_data.join("narrant.sqlite3"));
            #[cfg(not(feature = "wdio"))]
            let database_path = application_data.join("narrant.sqlite3");
            let oauth_client = tauri::async_runtime::block_on(async {
                Arc::new(
                    github_auth::OctocrabOAuthClient::new(&oauth_base_url)
                        .await
                        .expect("build GitHub OAuth client"),
                )
            });
            let identity_provider = Arc::new(github_auth::FileGitHubIdentityProvider::new(
                github_api_base_url(),
                application_data.join("github-identity"),
            ));
            let auth = Arc::new(github_auth::GitHubAuth::with_identity_provider(
                option_env!("NARRANT_GITHUB_CLIENT_ID").unwrap_or_default(),
                option_env!("NARRANT_GITHUB_APP_SLUG").unwrap_or_default(),
                oauth_client,
                Arc::new(github_auth::SystemCredentialStore),
                identity_provider,
            ));
            let catalog = Arc::new(repository_catalog::RepositoryCatalog::open(&database_path)?);
            let pull_requests = Arc::new(
                pull_request_selection::PullRequestSelectionResolver::authenticated(
                    github_api_base_url(),
                    auth.clone(),
                ),
            );
            let lifecycle = review_lifecycle::ReviewLifecycle::with_pull_request_resolver(
                catalog,
                pull_requests,
            );
            let _ = lifecycle.recover();
            app.manage(lifecycle);
            app.manage(auth);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            register_repository_entry,
            list_repository_entries,
            list_reviews,
            open_repository_entry,
            repository_workspace_trust,
            grant_repository_workspace_trust,
            compare_local_refs,
            check_branch_review_updates,
            reopen_and_refresh_branch_review,
            refresh_branch_review,
            reopen_review,
            list_local_refs,
            cancel_operation,
            discover_github_pull_requests,
            open_pull_request_review,
            check_pull_request_review_update,
            refresh_pull_request_review,
            list_saved_reviews,
            open_saved_review,
            github_auth_status,
            refresh_github_identity,
            start_github_authorization,
            complete_github_authorization,
            sign_out_github,
            read_review_file_change,
            close_review
        ])
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        // 仅恢复用户可见的桌面窗口状态，避免意外恢复全屏或其他窗口属性。
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    StateFlags::SIZE
                        | StateFlags::POSITION
                        | StateFlags::MAXIMIZED
                        | StateFlags::VISIBLE,
                )
                .build(),
        );
    #[cfg(feature = "wdio")]
    let builder = builder
        .plugin(tauri_plugin_wdio::init())
        .plugin(tauri_plugin_wdio_webdriver::init());
    let application = builder
        .build(tauri::generate_context!())
        .expect("build Narrant");
    application.run(|app, event| {
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            let lifecycle = app.state::<review_lifecycle::ReviewLifecycle>();
            let _ = lifecycle.shutdown();
        }
    });
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
