use crate::github;
use crate::repository;
use std::sync::Arc;

/// 返回当前构建使用的 GitHub API 基础地址。
pub(crate) fn github_api_base_url() -> String {
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

/// 为 Repository 的全部 GitHub remotes 独立发现 Pull Requests。
#[tauri::command]
pub(crate) async fn discover_github_pull_requests(
    auth: tauri::State<'_, Arc<github::GitHubAuth>>,
    application: tauri::State<'_, Arc<repository::RepositoryApplication>>,
    repository_id: i64,
) -> Result<Vec<github::PullRequestGroup>, String> {
    let repository_path = application
        .require_available_path(repository_id)
        .map_err(|error| error.to_string())?;
    let access_token = auth.access_token().await?;
    let api_base_url = github_api_base_url();
    github::discover_pull_requests(&repository_path, &api_base_url, access_token).await
}

#[tauri::command]
/// 返回当前 GitHub App 授权及公开身份状态。
pub(crate) async fn github_auth_status(
    auth: tauri::State<'_, Arc<github::GitHubAuth>>,
) -> Result<github::AuthorizationStatus, String> {
    auth.status().await
}

/// 在不改变登录状态的情况下刷新 GitHub 公开身份。
#[tauri::command]
pub(crate) async fn refresh_github_identity(
    auth: tauri::State<'_, Arc<github::GitHubAuth>>,
) -> Result<github::GitHubIdentity, String> {
    auth.refresh_identity().await
}

#[tauri::command]
/// 开始 GitHub App device authorization。
pub(crate) async fn start_github_authorization(
    auth: tauri::State<'_, Arc<github::GitHubAuth>>,
) -> Result<github::DeviceAuthorization, String> {
    auth.start_device_authorization().await
}

#[tauri::command]
/// 完成指定 GitHub App device authorization。
pub(crate) async fn complete_github_authorization(
    auth: tauri::State<'_, Arc<github::GitHubAuth>>,
    authorization_id: String,
) -> Result<(), String> {
    auth.complete_device_authorization(&authorization_id).await
}

#[tauri::command]
/// 删除 GitHub 凭据和公开身份缓存。
pub(crate) async fn sign_out_github(
    auth: tauri::State<'_, Arc<github::GitHubAuth>>,
) -> Result<(), String> {
    auth.sign_out().await
}
