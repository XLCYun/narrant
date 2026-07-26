mod content;
mod pull_request;

use super::{ChangeStatus, LoadedFileChange};
use crate::github_auth::GitHubAccessTokenProvider;
use reqwest::blocking::Client;
use secrecy::{ExposeSecret, SecretString};
use std::sync::Arc;

pub use pull_request::{ParsePullRequestLocationError, PullRequestLocation};

/// 打开 Pull Request 时固定下来的 GitHub Review Snapshot。
///
/// Snapshot 只保存元数据和 File Change 列表，不缓存文件正文。后续读取必须使用这里
/// 记录的精确 revision，避免 Pull Request 更新后混入新内容。
#[derive(Clone, Debug)]
pub struct GitHubSnapshot {
    pub owner: String,
    pub repository: String,
    pub title: String,
    pub number: u64,
    pub author: String,
    pub state: String,
    pub draft: bool,
    pub base_ref: String,
    /// 打开 Review 时 Base Ref 指向的 commit；用于描述 Snapshot 身份。
    pub base_revision: String,
    /// Base 与 Head 的 merge base；File Change 的 before 内容从这里读取。
    pub comparison_base: String,
    pub head_ref: String,
    /// 打开 Review 时 Head Ref 指向的 commit；File Change 的 after 内容从这里读取。
    pub head_revision: String,
    pub description: String,
    pub changed_files: usize,
    pub additions: usize,
    pub deletions: usize,
    pub file_changes: Vec<GitHubFileChange>,
    pub github_url: String,
}

/// Snapshot 中一个 File Change 的轻量索引。
///
/// 文件正文按需加载；重命名文件以新路径作为公开查找键，旧路径仅用于读取 before。
#[derive(Clone, Debug, PartialEq)]
pub struct GitHubFileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub status: ChangeStatus,
}

/// 可直接呈现给用户的 GitHub adapter 错误。
#[derive(Debug)]
pub struct GitHubError(String);

impl std::fmt::Display for GitHubError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for GitHubError {}

impl From<ParsePullRequestLocationError> for GitHubError {
    fn from(error: ParsePullRequestLocationError) -> Self {
        Self(error.to_string())
    }
}

/// GitHub adapter 内部共享的请求边界。
///
/// API 地址和 HTTP client 集中在这里，确保 PR 加载与内容读取复用同一份请求配置。
#[derive(Clone, Debug)]
struct GitHubClient {
    api_base_url: String,
    http: Client,
    access_token: Option<SecretString>,
}

impl GitHubClient {
    /// 构建一次 GitHub 操作使用的阻塞 HTTP 客户端和可选密钥令牌。
    fn new(api_base_url: String, access_token: Option<SecretString>) -> Self {
        Self {
            api_base_url: api_base_url.trim_end_matches('/').to_owned(),
            http: Client::builder()
                .user_agent("Narrant/0.1")
                .build()
                .expect("build GitHub HTTP client"),
            access_token,
        }
    }

    /// 构建 GET 请求，并仅在已认证时附加 Bearer 请求头。
    fn get(&self, endpoint: String) -> reqwest::blocking::RequestBuilder {
        let request = self.http.get(endpoint);
        match &self.access_token {
            Some(token) => request.bearer_auth(token.expose_secret()),
            None => request,
        }
    }

    /// 在发出阻塞请求前替换本次操作使用的令牌。
    fn set_access_token(&mut self, access_token: Option<SecretString>) {
        self.access_token = access_token;
    }

    fn map_content_error(&self, status: u16) -> GitHubError {
        if self.access_token.is_some() {
            map_authenticated_pull_request_response_error(status)
        } else {
            map_content_response_error(status)
        }
    }
}

/// 已打开的 GitHub Review。
///
/// 同时持有不可变 Snapshot 与读取该 Snapshot 内容所需的 client，供 Review Session
/// 在生命周期内重复按需读取 File Change。
#[derive(Clone)]
pub struct GitHubOpenedReview {
    client: GitHubClient,
    snapshot: GitHubSnapshot,
    access_token_provider: Option<Arc<dyn GitHubAccessTokenProvider>>,
}

impl std::fmt::Debug for GitHubOpenedReview {
    /// 省略令牌提供者，避免诊断输出暴露认证内部信息。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("GitHubOpenedReview")
            .field("snapshot", &self.snapshot)
            .finish_non_exhaustive()
    }
}

impl GitHubOpenedReview {
    /// 不使用用户凭据打开公开 Pull Request。
    pub fn open(api_base_url: String, url: &str) -> Result<Self, GitHubError> {
        Self::open_with_token(api_base_url, url, None::<SecretString>)
    }

    /// 供旧同步调用方使用可选令牌打开 Pull Request。
    pub fn open_with_token<T>(
        api_base_url: String,
        url: &str,
        access_token: Option<T>,
    ) -> Result<Self, GitHubError>
    where
        T: Into<SecretString>,
    {
        let client = GitHubClient::new(api_base_url, access_token.map(Into::into));
        let snapshot = client.load_pull_request(url)?;
        Ok(Self {
            client,
            snapshot,
            access_token_provider: None,
        })
    }

    /// 使用共享 GitHub 认证边界提供的可选令牌打开 Review。
    ///
    /// 同步 HTTP adapter 在阻塞工作线程运行；令牌获取保持异步，避免 refresh token
    /// 轮换阻塞 Tauri 的异步运行时。
    pub async fn open_with_access_token_provider(
        api_base_url: String,
        url: String,
        access_token_provider: Arc<dyn GitHubAccessTokenProvider>,
    ) -> Result<Self, GitHubError> {
        let access_token = access_token_provider
            .access_token()
            .await
            .map_err(GitHubError)?;
        let review = tokio::task::spawn_blocking(move || {
            Self::open_with_token(api_base_url, &url, access_token)
        })
        .await
        .map_err(|_| GitHubError("GitHub 请求任务意外中断，请重试。".to_owned()))??;
        Ok(Self {
            access_token_provider: Some(access_token_provider),
            ..review
        })
    }

    /// 返回打开 Review 时固定下来的不可变 Pull Request 快照。
    pub fn snapshot(&self) -> &GitHubSnapshot {
        &self.snapshot
    }

    /// 供旧同步调用方从固定快照读取一个文件。
    pub fn read_file_change(&self, path: &str) -> Result<LoadedFileChange, GitHubError> {
        self.client.read_file_change(&self.snapshot, path)
    }

    /// 供旧同步调用方替换可选令牌。
    pub fn set_access_token<T>(&mut self, access_token: Option<T>)
    where
        T: Into<SecretString>,
    {
        self.client.set_access_token(access_token.map(Into::into));
    }

    /// 使用新获取的可选令牌和阻塞 HTTP 工作线程读取一个文件。
    pub async fn read_file_change_with_access_token_provider(
        &self,
        path: String,
    ) -> Result<LoadedFileChange, GitHubError> {
        let access_token = match &self.access_token_provider {
            Some(provider) => provider.access_token().await.map_err(GitHubError)?,
            None => self.client.access_token.clone(),
        };
        let mut client = self.client.clone();
        client.set_access_token(access_token);
        let snapshot = self.snapshot.clone();
        tokio::task::spawn_blocking(move || client.read_file_change(&snapshot, &path))
            .await
            .map_err(|_| GitHubError("GitHub 请求任务意外中断，请重试。".to_owned()))?
    }
}

fn map_pull_request_response_error(status: u16) -> GitHubError {
    match status {
        404 => GitHubError(
            "找不到这个公开 Pull Request。请检查 URL，私有仓库需要登录后访问。".to_owned(),
        ),
        403 | 429 => GitHubError("GitHub 匿名访问次数已用尽，请稍后重试或登录后继续。".to_owned()),
        _ => GitHubError("GitHub 服务暂时不可用，请稍后重试。".to_owned()),
    }
}

fn map_authenticated_pull_request_response_error(status: u16) -> GitHubError {
    match status {
        401 => GitHubError("GitHub 登录已失效，请重新授权。".to_owned()),
        403 | 404 => GitHubError(
            "当前账号或 GitHub App 安装范围无权访问此 Pull Request，请检查仓库授权或组织审批。"
                .to_owned(),
        ),
        429 => GitHubError("GitHub 访问次数已用尽，请稍后重试。".to_owned()),
        _ => GitHubError("GitHub 服务暂时不可用，请稍后重试。".to_owned()),
    }
}

fn map_content_response_error(status: u16) -> GitHubError {
    match status {
        404 => GitHubError(
            "无法读取固定版本中的 File Change（404），请重新打开 Pull Request 后重试。".to_owned(),
        ),
        403 | 429 => GitHubError("GitHub 匿名访问次数已用尽，请稍后重试或登录后继续。".to_owned()),
        _ => GitHubError("GitHub 服务暂时不可用，请稍后重试。".to_owned()),
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
