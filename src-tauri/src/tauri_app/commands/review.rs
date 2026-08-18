use crate::github::GitHubRepository;
use crate::operation;
use crate::review;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
/// Review Snapshot 列表中的单个 File Change IPC 摘要。
struct FileChangeSummaryResponse {
    path: String,
    old_path: Option<String>,
    status: FileChangeStatusResponse,
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
/// WebView 读取一个 File Change 时接收的完整 IPC 内容。
pub(crate) struct FileChangeContentResponse {
    path: String,
    old_path: Option<String>,
    status: FileChangeStatusResponse,
    content_kind: FileContentKindResponse,
    before_size: Option<usize>,
    after_size: Option<usize>,
    before: Option<String>,
    after: Option<String>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
/// File Change 状态的稳定 IPC 表示。
enum FileChangeStatusResponse {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
/// File Change 内容类别的稳定 IPC 表示。
enum FileContentKindResponse {
    Text,
    Binary,
    Submodule,
    Symlink,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
/// 当前 Review Snapshot 的完整 IPC 表示。
pub(crate) struct ReviewSnapshotResponse {
    review_id: String,
    snapshot_id: String,
    base_ref: String,
    head_ref: String,
    base_revision: String,
    comparison_base: String,
    head_revision: String,
    file_changes: Vec<FileChangeSummaryResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pull_request: Option<review::PullRequestMetadata>,
}

/// 检查 moving refs 后返回的版本差异，不携带或切换候选 File Changes。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchReviewUpdateResponse {
    changed: bool,
    current_base_revision: String,
    current_head_revision: String,
    latest_base_revision: String,
    latest_head_revision: String,
}

/// 用户基于检查结果明确确认的一组 current/latest revisions。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfirmedBranchUpdate {
    current_base_revision: String,
    current_head_revision: String,
    latest_base_revision: String,
    latest_head_revision: String,
}

/// Pull Request 更新确认仅固定用户刚刚看到的 current/available revisions。
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ConfirmedPullRequestUpdate {
    current_base_revision: String,
    current_head_revision: String,
    available_base_revision: String,
    available_head_revision: String,
}

/// 检查 Pull Request 当前 revisions 后返回，不改变 current Snapshot 或 workspace。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ReviewUpdateResponse {
    update_available: bool,
    current_base_revision: String,
    current_head_revision: String,
    available_base_revision: String,
    available_head_revision: String,
}

/// 将领域 File Change 状态映射到稳定 IPC 枚举。
fn map_status(status: review::FileChangeStatus) -> FileChangeStatusResponse {
    match status {
        review::FileChangeStatus::Added => FileChangeStatusResponse::Added,
        review::FileChangeStatus::Modified => FileChangeStatusResponse::Modified,
        review::FileChangeStatus::Deleted => FileChangeStatusResponse::Deleted,
        review::FileChangeStatus::Renamed => FileChangeStatusResponse::Renamed,
    }
}

/// 将领域内容类别映射到稳定 IPC 枚举。
fn map_content_kind(kind: review::FileContentKind) -> FileContentKindResponse {
    match kind {
        review::FileContentKind::Text => FileContentKindResponse::Text,
        review::FileContentKind::Binary => FileContentKindResponse::Binary,
        review::FileContentKind::Submodule => FileContentKindResponse::Submodule,
        review::FileContentKind::Symlink => FileContentKindResponse::Symlink,
    }
}

/// 将固定 Snapshot 中读取的 File Change 映射到 IPC 内容。
pub(crate) fn map_file_content(change: review::FileChangeContent) -> FileChangeContentResponse {
    let content_kind = change.content_kind();
    let before_size = change.before.as_ref().and_then(review::FileContent::size);
    let after_size = change.after.as_ref().and_then(review::FileContent::size);
    FileChangeContentResponse {
        path: change.path,
        old_path: change.old_path,
        status: map_status(change.status),
        content_kind: map_content_kind(content_kind),
        before_size,
        after_size,
        before: change
            .before
            .as_ref()
            .and_then(review::FileContent::text)
            .map(str::to_owned),
        after: change
            .after
            .as_ref()
            .and_then(review::FileContent::text)
            .map(str::to_owned),
    }
}

/// 将内部固定 Snapshot 映射为不区分来源的前端 IPC 契约。
pub(crate) fn map_snapshot(
    review_id: String,
    snapshot_id: String,
    snapshot: &review::ChangeComparison,
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
                before_mode: change.before.as_ref().map(|content| content.mode),
                after_mode: change.after.as_ref().map(|content| content.mode),
                before_object_id: change
                    .before
                    .as_ref()
                    .map(|content| content.object_id.clone()),
                after_object_id: change
                    .after
                    .as_ref()
                    .map(|content| content.object_id.clone()),
            })
            .collect(),
        pull_request: None,
    }
}

/// 将生命周期输出映射为既有 Tauri IPC 契约。
fn map_presentation(presentation: review::ReviewPresentation) -> ReviewSnapshotResponse {
    let mut response = map_snapshot(
        presentation.review_id,
        presentation.snapshot_id,
        &presentation.snapshot,
    );
    response.pull_request = presentation.pull_request;
    response
}

/// 将 Tauri 的字符串 operation identity 与错误契约适配到可取消操作 seam。
pub(super) async fn run_cancellable_command<T, F, Fut>(
    operations: &operation::CancellableOperations,
    operation_id: String,
    operation: F,
) -> Result<T, String>
where
    F: FnOnce(operation::OperationContext) -> Fut,
    Fut: std::future::Future<Output = Result<T, String>>,
{
    let id = operation::OperationId::new(operation_id).map_err(|error| error.to_string())?;
    operations
        .run(id, operation)
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
/// 从 Branch Selection 创建或重开 durable Review。
pub(crate) async fn compare_local_refs(
    lifecycle: tauri::State<'_, review::ReviewLifecycle>,
    operations: tauri::State<'_, operation::CancellableOperations>,
    operation_id: String,
    repository_id: i64,
    base_ref: String,
    head_ref: String,
) -> Result<ReviewSnapshotResponse, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .present(
                &operation,
                review::PresentReview::Branch {
                    repository_id,
                    base_ref,
                    head_ref,
                },
            )
            .await
            .map(map_presentation)
            .map_err(|error| error.to_string())
    })
    .await
}

/// 重新获取所选 remote refs 并解析最新版本，但保持 current Snapshot 与 Workspace 不变。
#[tauri::command]
pub(crate) async fn check_branch_review_updates(
    lifecycle: tauri::State<'_, review::ReviewLifecycle>,
    operations: tauri::State<'_, operation::CancellableOperations>,
    operation_id: String,
    review_id: String,
) -> Result<BranchReviewUpdateResponse, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .check_update(&operation, &review_id)
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
pub(crate) async fn refresh_branch_review(
    lifecycle: tauri::State<'_, review::ReviewLifecycle>,
    operations: tauri::State<'_, operation::CancellableOperations>,
    operation_id: String,
    review_id: String,
    confirmation: ConfirmedBranchUpdate,
) -> Result<ReviewSnapshotResponse, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .refresh(
                &operation,
                &review_id,
                review::ConfirmedReviewUpdate {
                    current_base_revision: confirmation.current_base_revision,
                    current_head_revision: confirmation.current_head_revision,
                    available_base_revision: confirmation.latest_base_revision,
                    available_head_revision: confirmation.latest_head_revision,
                },
            )
            .await
            .map(map_presentation)
            .map_err(|error| error.to_string())
    })
    .await
}

/// 原子承接 Branch Selection：重开 durable Review，并使用既有确认刷新。
#[tauri::command]
pub(crate) async fn reopen_and_refresh_branch_review(
    lifecycle: tauri::State<'_, review::ReviewLifecycle>,
    operations: tauri::State<'_, operation::CancellableOperations>,
    operation_id: String,
    review_id: String,
    confirmation: ConfirmedBranchUpdate,
) -> Result<ReviewSnapshotResponse, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .reopen_and_refresh(
                &operation,
                &review_id,
                review::ConfirmedReviewUpdate {
                    current_base_revision: confirmation.current_base_revision,
                    current_head_revision: confirmation.current_head_revision,
                    available_base_revision: confirmation.latest_base_revision,
                    available_head_revision: confirmation.latest_head_revision,
                },
            )
            .await
            .map(map_presentation)
            .map_err(|error| error.to_string())
    })
    .await
}

/// 从稳定 Selection identity 创建或重开 durable Pull Request Review。
#[tauri::command]
pub(crate) async fn open_pull_request_review(
    lifecycle: tauri::State<'_, review::ReviewLifecycle>,
    operations: tauri::State<'_, operation::CancellableOperations>,
    operation_id: String,
    repository_id: i64,
    repository: GitHubRepository,
    number: u64,
) -> Result<ReviewSnapshotResponse, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .present(
                &operation,
                review::PresentReview::PullRequest {
                    repository_id,
                    selection: review::PullRequestSelection { repository, number },
                },
            )
            .await
            .map(map_presentation)
            .map_err(|error| error.to_string())
    })
    .await
}

/// 检查 Pull Request 当前 revisions；只获取对象并计算候选 Snapshot。
#[tauri::command]
pub(crate) async fn check_pull_request_review_update(
    lifecycle: tauri::State<'_, review::ReviewLifecycle>,
    operations: tauri::State<'_, operation::CancellableOperations>,
    operation_id: String,
    review_id: String,
) -> Result<ReviewUpdateResponse, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .check_update(&operation, &review_id)
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
pub(crate) async fn refresh_pull_request_review(
    lifecycle: tauri::State<'_, review::ReviewLifecycle>,
    operations: tauri::State<'_, operation::CancellableOperations>,
    operation_id: String,
    review_id: String,
    confirmation: ConfirmedPullRequestUpdate,
) -> Result<ReviewSnapshotResponse, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .refresh(
                &operation,
                &review_id,
                review::ConfirmedReviewUpdate {
                    current_base_revision: confirmation.current_base_revision,
                    current_head_revision: confirmation.current_head_revision,
                    available_base_revision: confirmation.available_base_revision,
                    available_head_revision: confirmation.available_head_revision,
                },
            )
            .await
            .map(map_presentation)
            .map_err(|error| error.to_string())
    })
    .await
}

/// 通过 durable reviewId 从 current Snapshot 读取本地 File Change。
#[tauri::command]
pub(crate) async fn read_review_file_change(
    lifecycle: tauri::State<'_, review::ReviewLifecycle>,
    operations: tauri::State<'_, operation::CancellableOperations>,
    operation_id: String,
    review_id: String,
    snapshot_id: String,
    path: String,
) -> Result<FileChangeContentResponse, String> {
    run_cancellable_command(&operations, operation_id, |operation| async move {
        lifecycle
            .read_file_change(&operation, &review_id, &snapshot_id, &path)
            .await
            .map(map_file_content)
            .map_err(|error| error.to_string())
    })
    .await
}

/// 关闭 durable Review 并精确移除它的 disposable linked worktree。
#[tauri::command]
pub(crate) fn close_review(
    lifecycle: tauri::State<'_, review::ReviewLifecycle>,
    review_id: String,
) -> Result<(), String> {
    lifecycle
        .close(&review_id)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// 使用 current Snapshot 按 durable reviewId 重新物化 Review。
#[tauri::command]
pub(crate) async fn reopen_review(
    lifecycle: tauri::State<'_, review::ReviewLifecycle>,
    review_id: String,
) -> Result<ReviewSnapshotResponse, String> {
    lifecycle
        .present(
            &operation::OperationContext::background(),
            review::PresentReview::Saved { review_id },
        )
        .await
        .map(map_presentation)
        .map_err(|error| error.to_string())
}
