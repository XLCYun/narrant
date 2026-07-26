pub mod change_content;
pub mod core_git;
pub mod github_auth;
pub mod github_discovery;
pub mod repository_catalog;

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::Manager;

enum OpenedReview {
    Local(Box<change_content::local::LocalOpenedReview>),
}

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

/// 保存当前打开 Review 的运行时读取句柄，并串行化同一 Review 的刷新操作。
#[derive(Default)]
struct OpenReviewStore {
    reviews: Mutex<HashMap<String, Arc<Mutex<OpenedReview>>>>,
    active_refreshes: Mutex<HashSet<String>>,
}

/// 跟踪可由另一个 Tauri 调用取消的在途 Core Git 操作。
#[derive(Default)]
struct GitOperationStore {
    operations: Mutex<HashMap<String, core_git::CancellationToken>>,
}

impl GitOperationStore {
    /// 注册一次 Core Git 操作，并返回其共享取消标记。
    fn start(&self, id: String) -> Result<core_git::CancellationToken, String> {
        let token = core_git::CancellationToken::default();
        self.operations
            .lock()
            .map_err(|_| "Core Git 操作状态不可用。".to_owned())?
            .insert(id, token.clone());
        Ok(token)
    }

    /// 移除已经完成的 Core Git 操作。
    fn finish(&self, id: &str) {
        if let Ok(mut operations) = self.operations.lock() {
            operations.remove(id);
        }
    }

    /// 请求取消指定的在途 Core Git 操作；操作已经完成时保持幂等。
    fn cancel(&self, id: &str) -> Result<(), String> {
        if let Some(token) = self
            .operations
            .lock()
            .map_err(|_| "Core Git 操作状态不可用。".to_owned())?
            .get(id)
        {
            token.cancel();
        }
        Ok(())
    }
}

impl OpenReviewStore {
    /// 用 durable reviewId 注册运行时读取句柄。
    fn insert_with_id(&self, id: String, review: OpenedReview) -> Result<(), String> {
        self.reviews
            .lock()
            .map_err(|_| "Review 状态不可用。".to_owned())?
            .insert(id, Arc::new(Mutex::new(review)));
        Ok(())
    }

    /// 按 durable reviewId 获取仍处于打开状态的运行时读取句柄。
    fn get(&self, id: &str) -> Result<Arc<Mutex<OpenedReview>>, String> {
        self.reviews
            .lock()
            .map_err(|_| "Review 状态不可用。".to_owned())?
            .get(id)
            .cloned()
            .ok_or_else(|| "Review 已关闭或失效，请重新打开。".to_owned())
    }

    /// 幂等移除 durable Review 的运行时读取句柄。
    fn close(&self, id: &str) -> Result<(), String> {
        self.reviews
            .lock()
            .map_err(|_| "Review 状态不可用。".to_owned())?
            .remove(id);
        Ok(())
    }
    /// 仅保留即将打开的同一 Review 句柄，确保运行时最多存在一个 Review。
    fn retain_only(&self, id: Option<&str>) -> Result<(), String> {
        self.reviews
            .lock()
            .map_err(|_| "Review 状态不可用。".to_owned())?
            .retain(|candidate, _| id.is_some_and(|id| candidate == id));
        Ok(())
    }

    /// 为一个 durable Review 获取独占刷新权，防止并发转换同一 Workspace。
    fn begin_refresh(&self, review_id: &str) -> Result<(), String> {
        let mut active = self
            .active_refreshes
            .lock()
            .map_err(|_| "Review 刷新状态不可用。".to_owned())?;
        if !active.insert(review_id.to_owned()) {
            return Err("此 Review 正在刷新；请等待当前操作完成。".to_owned());
        }
        Ok(())
    }

    /// 释放 durable Review 的独占刷新权。
    fn finish_refresh(&self, review_id: &str) {
        if let Ok(mut active) = self.active_refreshes.lock() {
            active.remove(review_id);
        }
    }
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
    snapshot: &change_content::local::LocalSnapshot,
) -> ReviewSnapshotResponse {
    ReviewSnapshotResponse {
        review_id,
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

#[tauri::command]
fn compare_local_refs(
    state: tauri::State<'_, OpenReviewStore>,
    operations: tauri::State<'_, GitOperationStore>,
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    operation_id: String,
    repository_entry_id: i64,
    base_ref: String,
    head_ref: String,
) -> Result<ReviewSnapshotResponse, String> {
    let repository_path = require_available_repository(&catalog, repository_entry_id)?;
    if !catalog
        .is_workspace_trusted(repository_entry_id)
        .map_err(|error| error.to_string())?
    {
        return Err("尚未授予 Workspace Trust；请先阅读风险说明并确认授权。".to_owned());
    }
    if let Some(saved) = catalog
        .find_branch_review(repository_entry_id, &base_ref, &head_ref)
        .map_err(|error| error.to_string())?
    {
        return open_saved_branch_review(&state, &catalog, repository_path, saved);
    }
    let cancellation = operations.start(operation_id.clone())?;
    let fetched = change_content::local::fetch_selected_remote_refs(
        &repository_path,
        &base_ref,
        &head_ref,
        &cancellation,
    )
    .map_err(|error| error.to_string());
    if let Err(error) = fetched {
        operations.finish(&operation_id);
        return Err(error);
    }
    let review = change_content::local::LocalOpenedReview::open_with_cancellation(
        &repository_path,
        &base_ref,
        &head_ref,
        &cancellation,
    )
    .map_err(|error| error.to_string());
    operations.finish(&operation_id);
    let calculated_review = review?;
    let saved = catalog
        .save_branch_review(repository_entry_id, calculated_review.snapshot())
        .map_err(|error| error.to_string())?;
    open_saved_branch_review(&state, &catalog, repository_path, saved)
}

/// 重新获取所选 remote refs 并解析最新版本，但保持 current Snapshot 与 Workspace 不变。
#[tauri::command]
fn check_branch_review_updates(
    operations: tauri::State<'_, GitOperationStore>,
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    operation_id: String,
    review_id: String,
) -> Result<BranchReviewUpdateResponse, String> {
    let current = catalog
        .open_review(&review_id)
        .map_err(|error| error.to_string())?;
    let repository = require_available_repository(&catalog, current.repository_entry_id)?;
    let cancellation = operations.start(operation_id.clone())?;
    let result = (|| {
        change_content::local::fetch_selected_remote_refs(
            &repository,
            &current.base_ref,
            &current.head_ref,
            &cancellation,
        )?;
        let latest = change_content::local::LocalOpenedReview::open_with_cancellation(
            &repository,
            &current.base_ref,
            &current.head_ref,
            &cancellation,
        )?;
        Ok::<_, core_git::GitCommandError>(BranchReviewUpdateResponse {
            changed: current.base_revision != latest.snapshot().base_revision
                || current.head_revision != latest.snapshot().head_revision,
            current_base_revision: current.base_revision,
            current_head_revision: current.head_revision,
            latest_base_revision: latest.snapshot().base_revision.clone(),
            latest_head_revision: latest.snapshot().head_revision.clone(),
        })
    })()
    .map_err(|error| error.to_string());
    operations.finish(&operation_id);
    result
}

/// 在用户明确确认后创建 Snapshot、重建 Workspace，并以 CAS 推进 current 指针。
#[tauri::command]
fn refresh_branch_review(
    state: tauri::State<'_, OpenReviewStore>,
    operations: tauri::State<'_, GitOperationStore>,
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    operation_id: String,
    review_id: String,
    confirmation: ConfirmedBranchUpdate,
) -> Result<ReviewSnapshotResponse, String> {
    state.begin_refresh(&review_id)?;
    let current = catalog
        .open_review(&review_id)
        .map_err(|error| error.to_string());
    let result = current.and_then(|current| {
        if current.base_revision != confirmation.current_base_revision
            || current.head_revision != confirmation.current_head_revision
        {
            return Err("current Snapshot 已改变；请重新检查更新。".to_owned());
        }
        let repository = require_available_repository(&catalog, current.repository_entry_id)?;
        let entry = catalog
            .open_entry(current.repository_entry_id)
            .map_err(|error| error.to_string())?;
        let cancellation = operations.start(operation_id.clone())?;
        let refreshed = (|| {
            change_content::local::fetch_selected_remote_refs(
                &repository,
                &current.base_ref,
                &current.head_ref,
                &cancellation,
            )?;
            let latest = change_content::local::LocalOpenedReview::open_with_cancellation(
                &repository,
                &current.base_ref,
                &current.head_ref,
                &cancellation,
            )?;
            if latest.snapshot().base_revision != confirmation.latest_base_revision
                || latest.snapshot().head_revision != confirmation.latest_head_revision
            {
                return Err(core_git::GitCommandError::new(
                    "Branch Selection 在确认后再次变化；请重新检查并确认新版本。",
                ));
            }
            if current.base_revision == latest.snapshot().base_revision
                && current.head_revision == latest.snapshot().head_revision
            {
                return Ok(map_snapshot(review_id.clone(), &current.snapshot()));
            }
            let candidate = catalog
                .save_refresh_snapshot(&review_id, latest.snapshot())
                .map_err(|error| core_git::GitCommandError::new(error.to_string()))?;
            let workspace = catalog
                .review_workspace_path(&review_id)
                .map_err(|error| core_git::GitCommandError::new(error.to_string()))?;
            let adapter = core_git::GitProcessAdapter::new(&repository);
            let mut reviews = state
                .reviews
                .lock()
                .map_err(|_| core_git::GitCommandError::new("Review 状态不可用。"))?;
            adapter.transition_detached_worktree(
                &workspace,
                &entry.common_directory,
                &current.head_revision,
                &candidate.head_revision,
                &cancellation,
            )?;
            let snapshot = candidate.snapshot();
            if let Err(error) = catalog.activate_refresh_snapshot(&candidate, &current) {
                let rollback = adapter.transition_detached_worktree(
                    &workspace,
                    &entry.common_directory,
                    &candidate.head_revision,
                    &current.head_revision,
                    &core_git::CancellationToken::default(),
                );
                return Err(core_git::GitCommandError::new(match rollback {
                    Ok(()) => error.to_string(),
                    Err(rollback_error) => {
                        format!("{error}；恢复旧 Review Workspace 失败：{rollback_error}")
                    }
                }));
            }
            reviews.insert(
                review_id.clone(),
                Arc::new(Mutex::new(OpenedReview::Local(Box::new(
                    change_content::local::LocalOpenedReview::restore(
                        &repository,
                        snapshot.clone(),
                    ),
                )))),
            );
            Ok(map_snapshot(review_id.clone(), &snapshot))
        })()
        .map_err(|error: core_git::GitCommandError| error.to_string());
        operations.finish(&operation_id);
        refreshed
    });
    state.finish_refresh(&review_id);
    result
}

/// materialize saved Snapshot，校验 workspace 后再推进 current 指针并注册运行时句柄。
fn open_saved_branch_review(
    state: &OpenReviewStore,
    catalog: &repository_catalog::RepositoryCatalog,
    repository_path: std::path::PathBuf,
    saved: repository_catalog::SavedBranchReview,
) -> Result<ReviewSnapshotResponse, String> {
    cleanup_review_workspaces(catalog, Some(&saved.review_id))?;
    state.retain_only(Some(&saved.review_id))?;
    let workspace = catalog
        .review_workspace_path(&saved.review_id)
        .map_err(|error| error.to_string())?;
    if workspace.exists() {
        let entry = catalog
            .open_entry(saved.repository_entry_id)
            .map_err(|error| error.to_string())?;
        core_git::GitProcessAdapter::new(&repository_path)
            .validate_owned_worktree(
                &workspace,
                &entry.common_directory,
                &core_git::CancellationToken::default(),
            )
            .map_err(|error| error.to_string())?;
    }
    if let Err(error) = core_git::GitProcessAdapter::new(&repository_path).create_detached_worktree(
        &workspace,
        &saved.head_revision,
        &core_git::CancellationToken::default(),
    ) {
        return Err(error.to_string());
    }
    if saved.created {
        catalog
            .activate_snapshot(&saved)
            .map_err(|error| error.to_string())?;
    }
    let snapshot = saved.snapshot();
    let review =
        change_content::local::LocalOpenedReview::restore(&repository_path, snapshot.clone());
    state.insert_with_id(
        saved.review_id.clone(),
        OpenedReview::Local(Box::new(review)),
    )?;
    catalog
        .mark_review_opened(&saved.review_id)
        .map_err(|error| error.to_string())?;
    let mut response = map_snapshot(saved.review_id, &snapshot);
    response.pull_request = saved.pull_request;
    Ok(response)
}

/// 从 discovery metadata 创建或重开 durable Pull Request Review。
#[tauri::command]
fn open_pull_request_review(
    state: tauri::State<'_, OpenReviewStore>,
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    repository_entry_id: i64,
    repository: github_discovery::GitHubRepository,
    pull_request: github_discovery::DiscoveredPullRequest,
) -> Result<ReviewSnapshotResponse, String> {
    let repository_path = require_available_repository(&catalog, repository_entry_id)?;
    if !catalog
        .is_workspace_trusted(repository_entry_id)
        .map_err(|error| error.to_string())?
    {
        return Err("尚未授予 Workspace Trust；请先阅读风险说明并确认授权。".to_owned());
    }
    if let Some(saved) = catalog
        .find_pull_request_review(
            repository_entry_id,
            &repository.owner,
            &repository.name,
            pull_request.number,
        )
        .map_err(|error| error.to_string())?
    {
        return open_saved_branch_review(&state, &catalog, repository_path, saved);
    }
    let (calculated, metadata) =
        calculate_pull_request_review(&repository_path, repository, pull_request)?;
    let saved = catalog
        .save_pull_request_review(repository_entry_id, &metadata, calculated.snapshot())
        .map_err(|error| error.to_string())?;
    open_saved_branch_review(&state, &catalog, repository_path, saved)
}

/// 检查 Pull Request 当前 revisions；只获取对象并计算候选 Snapshot。
#[tauri::command]
fn check_pull_request_review_update(
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    review_id: String,
    repository: github_discovery::GitHubRepository,
    pull_request: github_discovery::DiscoveredPullRequest,
) -> Result<ReviewUpdateResponse, String> {
    let current = catalog
        .open_review(&review_id)
        .map_err(|error| error.to_string())?;
    require_pull_request_identity(&current, &repository, &pull_request)?;
    let repository_path = require_available_repository(&catalog, current.repository_entry_id)?;
    let (available, _) = calculate_pull_request_review(&repository_path, repository, pull_request)?;
    Ok(ReviewUpdateResponse {
        update_available: current.base_revision != available.snapshot().base_revision
            || current.head_revision != available.snapshot().head_revision,
        current_base_revision: current.base_revision,
        current_head_revision: current.head_revision,
        available_base_revision: available.snapshot().base_revision.clone(),
        available_head_revision: available.snapshot().head_revision.clone(),
    })
}

/// 显式刷新 Pull Request Review，并在 workspace 对齐后推进 current Snapshot。
#[tauri::command]
fn refresh_pull_request_review(
    state: tauri::State<'_, OpenReviewStore>,
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    review_id: String,
    repository: github_discovery::GitHubRepository,
    pull_request: github_discovery::DiscoveredPullRequest,
) -> Result<ReviewSnapshotResponse, String> {
    let current = catalog
        .open_review(&review_id)
        .map_err(|error| error.to_string())?;
    require_pull_request_identity(&current, &repository, &pull_request)?;
    let repository_path = require_available_repository(&catalog, current.repository_entry_id)?;
    let (calculated, metadata) =
        calculate_pull_request_review(&repository_path, repository, pull_request)?;
    if current.base_revision == calculated.snapshot().base_revision
        && current.head_revision == calculated.snapshot().head_revision
    {
        return open_saved_branch_review(&state, &catalog, repository_path, current);
    }
    let staged = catalog
        .stage_pull_request_snapshot(&current, &metadata, calculated.snapshot())
        .map_err(|error| error.to_string())?;
    let workspace = catalog
        .review_workspace_path(&review_id)
        .map_err(|error| error.to_string())?;
    let adapter = core_git::GitProcessAdapter::new(&repository_path);
    adapter
        .remove_worktree(&workspace, &core_git::CancellationToken::default())
        .and_then(|_| {
            adapter.create_detached_worktree(
                &workspace,
                &staged.head_revision,
                &core_git::CancellationToken::default(),
            )
        })
        .map_err(|error| format!("Git transport：{error}"))?;
    open_saved_branch_review(&state, &catalog, repository_path, staged)
}

/// 确保更新请求不会改变 durable Review 的 immutable Selection identity。
fn require_pull_request_identity(
    current: &repository_catalog::SavedBranchReview,
    repository: &github_discovery::GitHubRepository,
    pull_request: &github_discovery::DiscoveredPullRequest,
) -> Result<(), String> {
    let saved = current
        .pull_request
        .as_ref()
        .ok_or_else(|| "该 Review 不是 Pull Request Selection。".to_owned())?;
    if !saved.owner.eq_ignore_ascii_case(&repository.owner)
        || !saved.repository.eq_ignore_ascii_case(&repository.name)
        || saved.number != pull_request.number
    {
        return Err("Pull Request Selection identity 与 durable Review 不一致。".to_owned());
    }
    Ok(())
}

/// 列出 Repository Entry 保存的 Reviews；不访问 GitHub，也不创建 workspace。
#[tauri::command]
fn list_saved_reviews(
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    repository_entry_id: i64,
) -> Result<Vec<SavedReviewSummaryResponse>, String> {
    require_available_repository(&catalog, repository_entry_id)?;
    catalog
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
fn open_saved_review(
    state: tauri::State<'_, OpenReviewStore>,
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    review_id: String,
) -> Result<ReviewSnapshotResponse, String> {
    let saved = catalog
        .open_review(&review_id)
        .map_err(|error| error.to_string())?;
    let repository_path = require_available_repository(&catalog, saved.repository_entry_id)?;
    open_saved_branch_review(&state, &catalog, repository_path, saved)
}

/// 将 GitHub metadata 与用户 Core Git transport 收敛为一个本地候选 Snapshot。
fn calculate_pull_request_review(
    repository_path: &std::path::Path,
    repository: github_discovery::GitHubRepository,
    pull_request: github_discovery::DiscoveredPullRequest,
) -> Result<
    (
        change_content::local::LocalOpenedReview,
        repository_catalog::PullRequestMetadata,
    ),
    String,
> {
    let remote = github_discovery::remote_names_for_repository(repository_path, &repository)?
        .into_iter()
        .next()
        .ok_or_else(|| {
            format!(
                "Git transport：找不到代表 base repository {}/{} 的 Git remote。",
                repository.owner, repository.name
            )
        })?;
    let calculated = change_content::local::fetch_pull_request_objects(
        repository_path,
        &remote,
        pull_request.number,
        &pull_request.base.reference,
        &pull_request.base.sha,
        &pull_request.head.sha,
    )
    .map_err(|error| format!("Git transport：{error}"))?;
    let metadata = repository_catalog::PullRequestMetadata {
        owner: repository.owner,
        repository: repository.name,
        number: pull_request.number,
        title: pull_request.title,
        description: pull_request.description,
        author: pull_request.author,
        state: pull_request.state,
        draft: pull_request.draft,
        url: pull_request.url,
        base_ref: pull_request.base.reference,
        head_ref: pull_request.head.reference,
        synchronized_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "系统时间早于 Unix epoch，无法保存 GitHub metadata。".to_owned())?
            .as_millis()
            .try_into()
            .map_err(|_| "系统时间超出可保存范围。".to_owned())?,
    };
    Ok((calculated, metadata))
}

#[tauri::command]
fn list_local_refs(
    operations: tauri::State<'_, GitOperationStore>,
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    operation_id: String,
    repository_entry_id: i64,
) -> Result<Vec<String>, String> {
    let repository_path = require_available_repository(&catalog, repository_entry_id)?;
    let cancellation = operations.start(operation_id.clone())?;
    let result =
        change_content::local::list_refs_with_cancellation(&repository_path, &cancellation)
            .map_err(|error| error.to_string());
    operations.finish(&operation_id);
    result
}

#[tauri::command]
fn cancel_git_operation(
    operations: tauri::State<'_, GitOperationStore>,
    operation_id: String,
) -> Result<(), String> {
    operations.cancel(&operation_id)
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
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    repository_entry_id: i64,
) -> Result<Vec<github_discovery::PullRequestGroup>, String> {
    let repository_path = require_available_repository(&catalog, repository_entry_id)?;
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
fn read_review_file_change(
    state: tauri::State<'_, OpenReviewStore>,
    operations: tauri::State<'_, GitOperationStore>,
    operation_id: String,
    review_id: String,
    path: String,
) -> Result<FileChangeContentResponse, String> {
    let review = state.get(&review_id)?;
    let review = review
        .lock()
        .map_err(|_| "Review 状态不可用。".to_owned())?;
    let OpenedReview::Local(review) = &*review;
    let cancellation = operations.start(operation_id.clone())?;
    let result = review
        .read_file_change_with_cancellation(&path, &cancellation)
        .map(map_file_content)
        .map_err(|error| error.to_string());
    operations.finish(&operation_id);
    result
}

/// 关闭 durable Review 并精确移除它的 disposable linked worktree。
#[tauri::command]
fn close_review(
    state: tauri::State<'_, OpenReviewStore>,
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    review_id: String,
) -> Result<(), String> {
    state.close(&review_id)?;
    let saved = catalog
        .open_review(&review_id)
        .map_err(|error| error.to_string())?;
    let repository = require_available_repository(&catalog, saved.repository_entry_id)?;
    let entry = catalog
        .open_entry(saved.repository_entry_id)
        .map_err(|error| error.to_string())?;
    let workspace = catalog
        .review_workspace_path(&review_id)
        .map_err(|error| error.to_string())?;
    if !workspace.exists() {
        return Ok(());
    }
    core_git::GitProcessAdapter::new(&repository)
        .remove_owned_worktree(
            &workspace,
            &entry.common_directory,
            &core_git::CancellationToken::default(),
        )
        .map_err(|error| error.to_string())
}

/// 严格清理除指定 Review 外的所有现存 workspace；任何所有权不匹配都会停止切换。
fn cleanup_review_workspaces(
    catalog: &repository_catalog::RepositoryCatalog,
    except_review_id: Option<&str>,
) -> Result<(), String> {
    for target in catalog
        .workspace_cleanup_targets()
        .map_err(|error| error.to_string())?
    {
        if except_review_id.is_some_and(|id| id == target.review_id)
            || !target.workspace_path.exists()
        {
            continue;
        }
        core_git::GitProcessAdapter::new(&target.repository_path)
            .remove_owned_worktree(
                &target.workspace_path,
                &target.common_directory,
                &core_git::CancellationToken::default(),
            )
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// 启动恢复逐项尝试清理；所有权不匹配的路径会原样保留且不会阻挡其他安全目标。
fn recover_interrupted_review_workspaces(catalog: &repository_catalog::RepositoryCatalog) {
    let Ok(targets) = catalog.workspace_cleanup_targets() else {
        return;
    };
    for target in targets {
        if !target.workspace_path.exists() {
            continue;
        }
        let _ = core_git::GitProcessAdapter::new(&target.repository_path).remove_owned_worktree(
            &target.workspace_path,
            &target.common_directory,
            &core_git::CancellationToken::default(),
        );
    }
}

/// 查询一个 Repository Entry 的持久化 Workspace Trust。
#[tauri::command]
fn repository_workspace_trust(
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    repository_entry_id: i64,
) -> Result<bool, String> {
    catalog
        .is_workspace_trusted(repository_entry_id)
        .map_err(|error| error.to_string())
}

/// 在用户确认风险说明后授予 Repository Entry 级 Workspace Trust。
#[tauri::command]
fn grant_repository_workspace_trust(
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    repository_entry_id: i64,
) -> Result<(), String> {
    catalog
        .grant_workspace_trust(repository_entry_id)
        .map_err(|error| error.to_string())
}

/// 注册一个经 Core Git 验证的 Repository Entry。
#[tauri::command]
fn register_repository_entry(
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    path: String,
) -> Result<repository_catalog::RepositoryEntry, String> {
    catalog
        .register(std::path::Path::new(&path))
        .map_err(|error| error.to_string())
}

/// 列出应用数据库中的全部 Repository Entries。
#[tauri::command]
fn list_repository_entries(
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
) -> Result<Vec<repository_catalog::RepositoryEntry>, String> {
    catalog.list().map_err(|error| error.to_string())
}

/// 列出一个 Repository Entry 的 Reviews；空 identity 表示全局最近 Reviews。
#[tauri::command]
fn list_reviews(
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    repository_entry_id: Option<i64>,
) -> Result<Vec<repository_catalog::ReviewSummary>, String> {
    catalog
        .list_reviews(repository_entry_id)
        .map_err(|error| error.to_string())
}

/// 使用 current Snapshot 按 durable reviewId 重新物化 Review。
#[tauri::command]
fn reopen_review(
    state: tauri::State<'_, OpenReviewStore>,
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    review_id: String,
) -> Result<ReviewSnapshotResponse, String> {
    let saved = catalog
        .open_review(&review_id)
        .map_err(|error| error.to_string())?;
    let repository_path = require_available_repository(&catalog, saved.repository_entry_id)?;
    open_saved_branch_review(&state, &catalog, repository_path, saved)
}

/// 打开 Repository Entry 并更新其最后打开时间。
#[tauri::command]
fn open_repository_entry(
    catalog: tauri::State<'_, repository_catalog::RepositoryCatalog>,
    id: i64,
) -> Result<repository_catalog::RepositoryEntry, String> {
    catalog.open_entry(id).map_err(|error| error.to_string())
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
    let oauth_client = tauri::async_runtime::block_on(async {
        Arc::new(
            github_auth::OctocrabOAuthClient::new(&oauth_base_url)
                .await
                .expect("build GitHub OAuth client"),
        )
    });
    let auth = Arc::new(github_auth::GitHubAuth::new(
        option_env!("NARRANT_GITHUB_CLIENT_ID").unwrap_or_default(),
        option_env!("NARRANT_GITHUB_APP_SLUG").unwrap_or_default(),
        oauth_client,
        Arc::new(github_auth::SystemCredentialStore),
    ));
    let builder = tauri::Builder::default()
        .manage(OpenReviewStore::default())
        .manage(GitOperationStore::default())
        .manage(auth)
        .setup(|app| {
            #[cfg(feature = "wdio")]
            let database_path = std::env::var_os("NARRANT_DATABASE_PATH")
                .map(std::path::PathBuf::from)
                .unwrap_or(app.path().app_data_dir()?.join("narrant.sqlite3"));
            #[cfg(not(feature = "wdio"))]
            let database_path = app.path().app_data_dir()?.join("narrant.sqlite3");
            let catalog = repository_catalog::RepositoryCatalog::open(&database_path)?;
            recover_interrupted_review_workspaces(&catalog);
            app.manage(catalog);
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
            refresh_branch_review,
            reopen_review,
            list_local_refs,
            cancel_git_operation,
            discover_github_pull_requests,
            open_pull_request_review,
            check_pull_request_review_update,
            refresh_pull_request_review,
            list_saved_reviews,
            open_saved_review,
            github_auth_status,
            start_github_authorization,
            complete_github_authorization,
            sign_out_github,
            read_review_file_change,
            close_review
        ])
        .plugin(tauri_plugin_opener::init());
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
            let catalog = app.state::<repository_catalog::RepositoryCatalog>();
            let _ = cleanup_review_workspaces(&catalog, None);
            let reviews = app.state::<OpenReviewStore>();
            let _ = reviews.retain_only(None);
        }
    });
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
