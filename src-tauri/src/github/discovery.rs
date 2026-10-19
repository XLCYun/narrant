use crate::core_git::{GitCommandError, GitProcessAdapter};
use crate::operation::OperationContext;
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use tokio_util::sync::CancellationToken;

/// 一个经规范化的 GitHub repository identity。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubRepository {
    pub owner: String,
    pub name: String,
}

impl GitHubRepository {
    /// 返回适合日志、排序和界面标题的稳定 repository identity。
    pub fn identity(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

/// Pull Request 当前选择的 ref 与精确 commit SHA。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestRevision {
    #[serde(rename = "ref")]
    pub reference: String,
    pub sha: String,
}

/// 创建 Review 所需的只读 Pull Request 协作 metadata。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredPullRequest {
    pub number: u64,
    pub title: String,
    pub description: String,
    pub author_username: String,
    pub state: String,
    pub is_draft: bool,
    pub web_url: String,
    pub github_updated_at: String,
    pub base: PullRequestRevision,
    pub head: PullRequestRevision,
}

/// 一个 GitHub repository 的独立 discovery 结果；失败不会抹掉其他分组。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestGroup {
    pub repository: GitHubRepository,
    pub remote_names: Vec<String>,
    pub pull_requests: Vec<DiscoveredPullRequest>,
    pub error: Option<String>,
    pub page: u32,
    /// 成功时指向后续页，失败时指向需要重试的原页；只有成功且为空才表示末尾。
    pub next_page: Option<u32>,
}

/// 用户选择的生命周期查询；Closed 与 Merged 共享 GitHub 的 closed 列表页。
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PullRequestStateFilter {
    Open,
    Closed,
    Merged,
    All,
}

impl PullRequestStateFilter {
    /// 将产品筛选转换为 REST 支持的状态；合并状态由页面元数据继续区分。
    fn api_state(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed | Self::Merged => "closed",
            Self::All => "all",
        }
    }
}

/// 为一个仍在 remotes 中的 GitHub repository 请求一个明确页码。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestPageRequest {
    pub repository: GitHubRepository,
    pub page: u32,
}

/// 一次有界 Discovery；不传 pages 时读取全部去重来源的首页。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestDiscoveryQuery {
    pub state: PullRequestStateFilter,
    pub pages: Option<Vec<PullRequestPageRequest>>,
}

/// 扫描 remotes 后，每个选定的 GitHub repository 最多请求一页协作元数据。
pub async fn discover_pull_requests(
    repository_path: &Path,
    api_base_url: &str,
    access_token: Option<SecretString>,
    query: PullRequestDiscoveryQuery,
) -> Result<Vec<PullRequestGroup>, String> {
    let mut requested_pages = BTreeMap::new();
    if let Some(pages) = &query.pages {
        for request in pages {
            if request.page == 0
                || requested_pages
                    .insert(
                        request.repository.identity().to_ascii_lowercase(),
                        request.page,
                    )
                    .is_some()
            {
                return Err("每个 GitHub repository 只能请求一个大于零的页码。".to_owned());
            }
        }
    }
    let repositories = github_repositories(repository_path)?;
    let client = Client::builder()
        .user_agent("Narrant/0.1")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|_| "GitHub discovery client 不可用。".to_owned())?;
    let mut groups = Vec::with_capacity(repositories.len());
    for (repository, remote_names) in repositories.into_values() {
        let page = match &query.pages {
            None => 1,
            Some(_) => match requested_pages.get(&repository.identity()) {
                Some(page) => *page,
                None => continue,
            },
        };
        let result = list_pull_requests(
            &client,
            api_base_url,
            &repository,
            access_token.as_ref(),
            query.state,
            page,
        )
        .await;
        groups.push(match result {
            Ok((pull_requests, next_page)) => PullRequestGroup {
                repository,
                remote_names,
                pull_requests,
                error: None,
                page,
                next_page,
            },
            Err(error) => PullRequestGroup {
                repository,
                remote_names,
                pull_requests: Vec::new(),
                error: Some(error),
                page,
                next_page: Some(page),
            },
        });
    }
    Ok(groups)
}

/// 按稳定 identity 读取一个 Pull Request 的最新协作 metadata 与 revisions。
pub async fn observe_pull_request(
    api_base_url: &str,
    repository: &GitHubRepository,
    number: u64,
    access_token: Option<&SecretString>,
) -> Result<DiscoveredPullRequest, String> {
    if number == 0 {
        return Err("Pull Request number 必须大于零。".to_owned());
    }
    let client = Client::builder()
        .user_agent("Narrant/0.1")
        .build()
        .map_err(|_| "GitHub observation client 不可用。".to_owned())?;
    let endpoint = format!(
        "{}/repos/{}/{}/pulls/{number}",
        api_base_url.trim_end_matches('/'),
        repository.owner,
        repository.name
    );
    let mut request = client.get(endpoint);
    if let Some(token) = access_token {
        request = request.bearer_auth(token.expose_secret());
    }
    let response = request
        .send()
        .await
        .map_err(|_| "当前无法连接 GitHub，请检查网络后重试。".to_owned())?;
    if !response.status().is_success() {
        return Err(map_discovery_error(
            response.status().as_u16(),
            access_token.is_some(),
        ));
    }
    response
        .json::<PullRequestListItem>()
        .await
        .map(DiscoveredPullRequest::from)
        .map_err(|_| "GitHub 返回了无法识别的 Pull Request metadata。".to_owned())
}

/// 读取全部 remote URL，并按大小写不敏感的 GitHub identity 合并同仓库 remote。
fn github_repositories(
    repository_path: &Path,
) -> Result<BTreeMap<String, (GitHubRepository, Vec<String>)>, String> {
    github_repositories_with_cancellation(
        repository_path,
        OperationContext::background().cancellation(),
    )
    .map_err(|error| error.to_string())
}

/// 使用调用方 cancellation 扫描 GitHub remotes。
fn github_repositories_with_cancellation(
    repository_path: &Path,
    cancellation: &CancellationToken,
) -> Result<BTreeMap<String, (GitHubRepository, Vec<String>)>, GitCommandError> {
    let adapter = GitProcessAdapter::new(repository_path);
    let remote_names = adapter.run(["remote"], cancellation)?;
    if remote_names.stdout.is_empty() {
        return Ok(BTreeMap::new());
    }
    let output = adapter.run(
        ["config", "--get-regexp", r"^remote\..*\.url$"],
        cancellation,
    )?;
    let text = String::from_utf8(output.stdout)
        .map_err(|_| GitCommandError::new("Core Git 返回了无法识别的 remote 配置。"))?;
    let mut repositories = BTreeMap::new();
    for line in text.lines() {
        let Some((key, url)) = line.split_once(char::is_whitespace) else {
            continue;
        };
        let Some(remote_name) = key
            .strip_prefix("remote.")
            .and_then(|key| key.strip_suffix(".url"))
        else {
            continue;
        };
        let Some(repository) = normalize_github_remote(url.trim()) else {
            continue;
        };
        let identity = repository.identity();
        let (_, names) = repositories
            .entry(identity)
            .or_insert_with(|| (repository, Vec::new()));
        if !names.iter().any(|name| name == remote_name) {
            names.push(remote_name.to_owned());
            names.sort();
        }
    }
    Ok(repositories)
}

/// 查找代表指定 GitHub base repository identity 的本地 remote 名称。
#[allow(dead_code, reason = "保留非取消调用入口供未来非操作上下文使用")]
pub fn remote_names_for_repository(
    repository_path: &Path,
    repository: &GitHubRepository,
) -> Result<Vec<String>, String> {
    remote_names_for_repository_with_cancellation(
        repository_path,
        repository,
        OperationContext::background().cancellation(),
    )
    .map_err(|error| error.to_string())
}

/// 使用调用方 cancellation 查找代表指定 GitHub repository 的 remotes。
pub fn remote_names_for_repository_with_cancellation(
    repository_path: &Path,
    repository: &GitHubRepository,
    cancellation: &CancellationToken,
) -> Result<Vec<String>, GitCommandError> {
    Ok(
        github_repositories_with_cancellation(repository_path, cancellation)?
            .remove(&repository.identity().to_ascii_lowercase())
            .map(|(_, remote_names)| remote_names)
            .unwrap_or_default(),
    )
}

/// 将 GitHub HTTPS、scp-like SSH 与 ssh:// remote 规范化为 repository identity。
fn normalize_github_remote(url: &str) -> Option<GitHubRepository> {
    let path = if let Some((scheme, remainder)) = url.split_once("://") {
        if !matches!(
            scheme.to_ascii_lowercase().as_str(),
            "http" | "https" | "ssh"
        ) {
            return None;
        }
        let (authority, path) = remainder.split_once('/')?;
        let host_with_port = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        let host = host_with_port.split(':').next()?;
        if !host.eq_ignore_ascii_case("github.com") {
            return None;
        }
        path
    } else if let Some((authority, path)) = url.split_once(':') {
        let host = authority
            .rsplit_once('@')
            .map_or(authority, |(_, host)| host);
        if !host.eq_ignore_ascii_case("github.com") {
            return None;
        }
        path
    } else {
        return None;
    };
    let path = path.trim_end_matches('/').trim_end_matches(".git");
    let mut parts = path.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    if owner.is_empty() || name.is_empty() || parts.next().is_some() {
        return None;
    }
    Some(GitHubRepository {
        owner: owner.to_ascii_lowercase(),
        name: name.to_ascii_lowercase(),
    })
}

/// 只读取指定的一页，不为填满筛选结果追读下一页，也不访问代码内容 endpoint。
async fn list_pull_requests(
    client: &Client,
    api_base_url: &str,
    repository: &GitHubRepository,
    access_token: Option<&SecretString>,
    state: PullRequestStateFilter,
    page: u32,
) -> Result<(Vec<DiscoveredPullRequest>, Option<u32>), String> {
    let endpoint = format!(
        "{}/repos/{}/{}/pulls",
        api_base_url.trim_end_matches('/'),
        repository.owner,
        repository.name
    );
    let mut request = client
        .get(&endpoint)
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .query(&[
            ("state", state.api_state().to_owned()),
            ("sort", "updated".to_owned()),
            ("direction", "desc".to_owned()),
            ("per_page", "30".to_owned()),
            ("page", page.to_string()),
        ]);
    if let Some(token) = access_token {
        request = request.bearer_auth(token.expose_secret());
    }
    let response = request
        .send()
        .await
        .map_err(|_| "当前无法连接 GitHub，请检查网络后重试。".to_owned())?;
    if !response.status().is_success() {
        return Err(map_discovery_error(
            response.status().as_u16(),
            access_token.is_some(),
        ));
    }
    // 只读取 Link 的 next 关系，不访问其中的 URL，避免把凭据发往非 GitHub 来源。
    let has_next = response
        .headers()
        .get_all(reqwest::header::LINK)
        .iter()
        .any(|header| {
            header.to_str().is_ok_and(|value| {
                value.split(',').any(|link| {
                    link.split(';').skip(1).any(|parameter| {
                        parameter
                            .trim()
                            .strip_prefix("rel=")
                            .is_some_and(|relations| {
                                relations
                                    .trim_matches('"')
                                    .split_whitespace()
                                    .any(|rel| rel == "next")
                            })
                    })
                })
            })
        });
    let next_page = if has_next {
        Some(
            page.checked_add(1)
                .ok_or_else(|| "GitHub 页码超出支持范围。".to_owned())?,
        )
    } else {
        None
    };
    let page_items: Vec<PullRequestListItem> = response
        .json()
        .await
        .map_err(|_| "GitHub 返回了无法识别的 Pull Request metadata。".to_owned())?;
    let mut items = BTreeMap::<u64, DiscoveredPullRequest>::new();
    for item in page_items.into_iter().map(DiscoveredPullRequest::from) {
        if items
            .get(&item.number)
            .is_none_or(|previous| previous.github_updated_at <= item.github_updated_at)
        {
            items.insert(item.number, item);
        }
    }
    Ok((items.into_values().collect(), next_page))
}

/// GitHub Pull Requests list endpoint 的最小只读响应模型。
#[derive(Deserialize)]
struct PullRequestListItem {
    number: u64,
    title: String,
    body: Option<String>,
    user: PullRequestUser,
    state: String,
    merged_at: Option<String>,
    draft: bool,
    html_url: String,
    updated_at: String,
    base: PullRequestRevision,
    head: PullRequestRevision,
}

/// Pull Request 作者响应片段。
#[derive(Deserialize)]
struct PullRequestUser {
    login: String,
}

impl From<PullRequestListItem> for DiscoveredPullRequest {
    /// 将 GitHub wire model 收敛为稳定的协作 metadata。
    fn from(value: PullRequestListItem) -> Self {
        Self {
            number: value.number,
            title: value.title,
            description: value.body.unwrap_or_default(),
            author_username: value.user.login,
            state: if value.merged_at.is_some() {
                "merged".to_owned()
            } else {
                value.state
            },
            is_draft: value.draft,
            web_url: value.html_url,
            github_updated_at: value.updated_at,
            base: value.base,
            head: value.head,
        }
    }
}

/// 区分匿名限流、认证权限与一般服务失败。
fn map_discovery_error(status: u16, authenticated: bool) -> String {
    match (authenticated, status) {
        (false, 403 | 429) => "GitHub 匿名访问次数已用尽，请稍后重试或登录后继续。".to_owned(),
        (false, 404) => "找不到这个公开 GitHub repository；私有仓库需要登录后访问。".to_owned(),
        (true, 401) => "GitHub 登录已失效，请重新授权。".to_owned(),
        (true, 403 | 404) => {
            "当前账号或 GitHub App 安装范围无权访问此 repository，请检查仓库授权或组织审批。"
                .to_owned()
        }
        (_, 429) => "GitHub 访问次数已用尽，请稍后重试。".to_owned(),
        _ => "GitHub 服务暂时不可用，请稍后重试。".to_owned(),
    }
}
