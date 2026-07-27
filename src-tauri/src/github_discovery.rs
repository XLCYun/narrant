use crate::core_git::{CancellationToken, GitProcessAdapter};
use reqwest::Client;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

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
    pub author: String,
    pub state: String,
    pub draft: bool,
    pub url: String,
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
}

/// 扫描 Repository Entry 的全部 remotes，按 GitHub repository 去重并列出 Pull Requests。
pub async fn discover_pull_requests(
    repository_path: &Path,
    api_base_url: &str,
    access_token: Option<SecretString>,
) -> Result<Vec<PullRequestGroup>, String> {
    let repositories = github_repositories(repository_path)?;
    let client = Client::builder()
        .user_agent("Narrant/0.1")
        .build()
        .map_err(|_| "GitHub discovery client 不可用。".to_owned())?;
    let mut groups = Vec::with_capacity(repositories.len());
    for (repository, remote_names) in repositories.into_values() {
        let result =
            list_pull_requests(&client, api_base_url, &repository, access_token.as_ref()).await;
        groups.push(match result {
            Ok(pull_requests) => PullRequestGroup {
                repository,
                remote_names,
                pull_requests,
                error: None,
            },
            Err(error) => PullRequestGroup {
                repository,
                remote_names,
                pull_requests: Vec::new(),
                error: Some(error),
            },
        });
    }
    Ok(groups)
}

/// 读取全部 remote URL，并按大小写不敏感的 GitHub identity 合并同仓库 remote。
fn github_repositories(
    repository_path: &Path,
) -> Result<BTreeMap<String, (GitHubRepository, Vec<String>)>, String> {
    let adapter = GitProcessAdapter::new(repository_path);
    let remote_names = adapter
        .run(["remote"], &CancellationToken::default())
        .map_err(|error| error.to_string())?;
    if remote_names.stdout.is_empty() {
        return Ok(BTreeMap::new());
    }
    let output = adapter
        .run(
            ["config", "--get-regexp", r"^remote\..*\.url$"],
            &CancellationToken::default(),
        )
        .map_err(|error| error.to_string())?;
    let text = String::from_utf8(output.stdout)
        .map_err(|_| "Core Git 返回了无法识别的 remote 配置。".to_owned())?;
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
pub fn remote_names_for_repository(
    repository_path: &Path,
    repository: &GitHubRepository,
) -> Result<Vec<String>, String> {
    Ok(github_repositories(repository_path)?
        .remove(&repository.identity())
        .map(|(_, remote_names)| remote_names)
        .unwrap_or_default())
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

/// 分页调用 Pull Requests list endpoint；这里不访问任何代码内容 endpoint。
async fn list_pull_requests(
    client: &Client,
    api_base_url: &str,
    repository: &GitHubRepository,
    access_token: Option<&SecretString>,
) -> Result<Vec<DiscoveredPullRequest>, String> {
    let endpoint = format!(
        "{}/repos/{}/{}/pulls",
        api_base_url.trim_end_matches('/'),
        repository.owner,
        repository.name
    );
    let mut result = Vec::new();
    for page in 1_u16..=100 {
        let mut request = client.get(&endpoint).query(&[
            ("state", "all".to_owned()),
            ("per_page", "100".to_owned()),
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
        let page_items: Vec<PullRequestListItem> = response
            .json()
            .await
            .map_err(|_| "GitHub 返回了无法识别的 Pull Request metadata。".to_owned())?;
        if page_items.is_empty() {
            return Ok(result);
        }
        result.extend(page_items.into_iter().map(DiscoveredPullRequest::from));
    }
    Err("GitHub Pull Request 分页超过安全上限，请缩小查询范围。".to_owned())
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
            author: value.user.login,
            state: if value.merged_at.is_some() {
                "merged".to_owned()
            } else {
                value.state
            },
            draft: value.draft,
            url: value.html_url,
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
