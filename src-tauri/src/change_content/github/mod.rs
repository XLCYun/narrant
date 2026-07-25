mod content;
mod pull_request;

use super::{ChangeStatus, LoadedFileChange};
use reqwest::blocking::Client;

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
#[derive(Debug)]
struct GitHubClient {
    api_base_url: String,
    http: Client,
    access_token: Option<String>,
}

impl GitHubClient {
    fn new(api_base_url: String, access_token: Option<String>) -> Self {
        Self {
            api_base_url: api_base_url.trim_end_matches('/').to_owned(),
            http: Client::builder()
                .user_agent("Narrant/0.1")
                .build()
                .expect("build GitHub HTTP client"),
            access_token,
        }
    }

    fn get(&self, endpoint: String) -> reqwest::blocking::RequestBuilder {
        let request = self.http.get(endpoint);
        match &self.access_token {
            Some(token) => request.bearer_auth(token),
            None => request,
        }
    }

    fn set_access_token(&mut self, access_token: Option<String>) {
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
#[derive(Debug)]
pub struct GitHubOpenedReview {
    client: GitHubClient,
    snapshot: GitHubSnapshot,
}

impl GitHubOpenedReview {
    pub fn open(api_base_url: String, url: &str) -> Result<Self, GitHubError> {
        Self::open_with_token(api_base_url, url, None)
    }

    pub fn open_with_token(
        api_base_url: String,
        url: &str,
        access_token: Option<String>,
    ) -> Result<Self, GitHubError> {
        let client = GitHubClient::new(api_base_url, access_token);
        let snapshot = client.load_pull_request(url)?;
        Ok(Self { client, snapshot })
    }

    pub fn snapshot(&self) -> &GitHubSnapshot {
        &self.snapshot
    }

    pub fn read_file_change(&self, path: &str) -> Result<LoadedFileChange, GitHubError> {
        self.client.read_file_change(&self.snapshot, path)
    }

    pub fn set_access_token(&mut self, access_token: Option<String>) {
        self.client.set_access_token(access_token);
    }

    #[cfg(test)]
    pub(crate) fn empty_for_test() -> Self {
        Self {
            client: GitHubClient::new("https://api.github.com".to_owned(), None),
            snapshot: GitHubSnapshot {
                owner: "test".to_owned(),
                repository: "test".to_owned(),
                title: "test".to_owned(),
                number: 1,
                author: "test".to_owned(),
                state: "open".to_owned(),
                draft: false,
                base_ref: "main".to_owned(),
                base_revision: "base".to_owned(),
                comparison_base: "base".to_owned(),
                head_ref: "head".to_owned(),
                head_revision: "head".to_owned(),
                description: String::new(),
                changed_files: 0,
                additions: 0,
                deletions: 0,
                file_changes: Vec::new(),
                github_url: "https://github.com/test/test/pull/1".to_owned(),
            },
        }
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
