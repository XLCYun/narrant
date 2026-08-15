use crate::operation;
use crate::repository;
use crate::review;
use serde::Serialize;
use std::sync::Arc;

use super::review::run_cancellable_command;

/// Repository 本地分支标签所需的 canonical refs 与显式默认选择。
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BranchRefsResponse {
    refs: Vec<String>,
    default_ref: String,
    current_ref: String,
}

#[tauri::command]
/// 返回首页所需的 Repository 与全局最近 Review projection。
pub(crate) fn load_repository_navigation(
    application: tauri::State<'_, Arc<repository::RepositoryApplication>>,
) -> Result<repository::RepositoryNavigation, String> {
    application
        .load_navigation()
        .map_err(|error| error.to_string())
}

/// 打开 Repository，并将 availability、Trust 与 Review projection 收进一个 command。
#[tauri::command]
pub(crate) fn open_repository_overview(
    application: tauri::State<'_, Arc<repository::RepositoryApplication>>,
    repository_id: i64,
) -> Result<repository::RepositoryOpenResult, String> {
    application
        .open_overview(repository_id)
        .map_err(|error| error.to_string())
}

/// 授予 Repository Workspace Trust 后返回完整 Repository Overview。
#[tauri::command]
pub(crate) fn grant_repository_workspace_trust_and_open(
    application: tauri::State<'_, Arc<repository::RepositoryApplication>>,
    repository_id: i64,
) -> Result<repository::RepositoryOverview, String> {
    application
        .grant_trust_and_open(repository_id)
        .map_err(|error| error.to_string())
}

/// 一次读取当前 Repository 的统一 Review summaries。
#[tauri::command]
pub(crate) fn load_repository_reviews(
    application: tauri::State<'_, Arc<repository::RepositoryApplication>>,
    repository_id: i64,
) -> Result<repository::RepositoryReviewList, String> {
    application
        .load_reviews(repository_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
/// 返回 Repository 的 canonical local refs。
pub(crate) async fn list_local_refs(
    application: tauri::State<'_, Arc<repository::RepositoryApplication>>,
    operations: tauri::State<'_, operation::CancellableOperations>,
    operation_id: String,
    repository_id: i64,
) -> Result<BranchRefsResponse, String> {
    let repository_path = application
        .require_available_path(repository_id)
        .map_err(|error| error.to_string())?;
    run_cancellable_command(&operations, operation_id, |operation| async move {
        review::BranchRefCatalog
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
/// 请求取消由 operation ID 标识的 application operation。
pub(crate) fn cancel_operation(
    operations: tauri::State<'_, operation::CancellableOperations>,
    operation_id: String,
) -> Result<(), String> {
    let id = operation::OperationId::new(operation_id).map_err(|error| error.to_string())?;
    operations.cancel(&id).map_err(|error| error.to_string())
}

#[tauri::command]
/// 注册一个经 Core Git 验证的 Repository。
pub(crate) fn register_repository(
    application: tauri::State<'_, Arc<repository::RepositoryApplication>>,
    path: String,
) -> Result<repository::RepositorySummary, String> {
    application
        .register_repository(std::path::Path::new(&path))
        .map_err(|error| error.to_string())
}
