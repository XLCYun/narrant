use super::{
    map_response_error, ChangeStatus, GitHubClient, GitHubError, GitHubFileChange, GitHubSnapshot,
};
use serde::Deserialize;

const INVALID_PULL_REQUEST_URL: &str =
    "请输入完整的 GitHub Pull Request URL，例如 https://github.com/owner/repo/pull/123";

/// 从标准 GitHub Pull Request URL 解析出的仓库位置。
#[derive(Debug, PartialEq)]
pub struct PullRequestLocation {
    pub owner: String,
    pub repository: String,
    pub number: u64,
}

/// Pull Request URL 不完整或不符合标准 GitHub URL 格式。
#[derive(Debug, PartialEq)]
pub struct ParsePullRequestLocationError;

impl std::fmt::Display for ParsePullRequestLocationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(INVALID_PULL_REQUEST_URL)
    }
}

impl std::error::Error for ParsePullRequestLocationError {}

impl PullRequestLocation {
    pub fn parse(value: &str) -> Result<Self, ParsePullRequestLocationError> {
        let path = value
            .strip_prefix("https://github.com/")
            .ok_or(ParsePullRequestLocationError)?;
        let parts = path.split('/').collect::<Vec<_>>();
        let [owner, repository, "pull", number] = parts.as_slice() else {
            return Err(ParsePullRequestLocationError);
        };
        if owner.is_empty() || repository.is_empty() {
            return Err(ParsePullRequestLocationError);
        }
        let number = number.parse().map_err(|_| ParsePullRequestLocationError)?;
        if number == 0 {
            return Err(ParsePullRequestLocationError);
        }
        Ok(Self {
            owner: (*owner).to_owned(),
            repository: (*repository).to_owned(),
            number,
        })
    }
}

impl GitHubClient {
    pub(super) fn load_pull_request(&self, url: &str) -> Result<GitHubSnapshot, GitHubError> {
        let location = PullRequestLocation::parse(url)?;
        let endpoint = format!(
            "{}/repos/{}/{}/pulls/{}",
            self.api_base_url, location.owner, location.repository, location.number
        );
        let response = self
            .http
            .get(endpoint)
            .send()
            .map_err(|_| GitHubError("当前无法连接 GitHub，请检查网络后重试。".to_owned()))?;
        if !response.status().is_success() {
            return Err(map_response_error(response.status().as_u16()));
        }
        let metadata: PullRequestResponse = response
            .json()
            .map_err(|_| GitHubError("GitHub 返回了无法识别的数据，请稍后重试。".to_owned()))?;
        let github_url = format!(
            "https://github.com/{}/{}/pull/{}",
            location.owner, location.repository, location.number
        );
        if metadata.changed_files > 3000 {
            return Err(GitHubError(format!(
                "此 Pull Request 有 {} 个 File Change，超过 GitHub API 的 3000 个上限，无法保证审核完整。请在 GitHub 查看：{}",
                metadata.changed_files, github_url
            )));
        }

        let comparison_endpoint = format!(
            "{}/repos/{}/{}/compare/{}...{}",
            self.api_base_url,
            location.owner,
            location.repository,
            metadata.base.sha,
            metadata.head.sha
        );
        let comparison_response = self
            .http
            .get(comparison_endpoint)
            .send()
            .map_err(|_| GitHubError("当前无法连接 GitHub，请检查网络后重试。".to_owned()))?;
        if !comparison_response.status().is_success() {
            return Err(map_response_error(comparison_response.status().as_u16()));
        }
        let comparison: ComparisonResponse = comparison_response
            .json()
            .map_err(|_| GitHubError("GitHub 返回了无法识别的数据，请稍后重试。".to_owned()))?;

        let mut file_changes = Vec::with_capacity(metadata.changed_files);
        let mut page = 1;
        while file_changes.len() < metadata.changed_files {
            let endpoint = format!(
                "{}/repos/{}/{}/pulls/{}/files",
                self.api_base_url, location.owner, location.repository, location.number
            );
            let response = self
                .http
                .get(endpoint)
                .query(&[("per_page", 100), ("page", page)])
                .send()
                .map_err(|_| GitHubError("当前无法连接 GitHub，请检查网络后重试。".to_owned()))?;
            if !response.status().is_success() {
                return Err(map_response_error(response.status().as_u16()));
            }
            let page_changes: Vec<FileResponse> = response
                .json()
                .map_err(|_| GitHubError("GitHub 返回了无法识别的数据，请稍后重试。".to_owned()))?;
            if page_changes.is_empty() {
                break;
            }
            file_changes.extend(page_changes.into_iter().map(file_change));
            page += 1;
        }
        if file_changes.len() != metadata.changed_files {
            return Err(GitHubError(
                "GitHub 返回的 File Change 列表不完整，请稍后重试或在 GitHub 查看。".to_owned(),
            ));
        }

        Ok(GitHubSnapshot {
            owner: location.owner,
            repository: location.repository,
            title: metadata.title,
            number: metadata.number,
            author: metadata.user.login,
            state: if metadata.merged_at.is_some() {
                "merged".to_owned()
            } else {
                metadata.state
            },
            draft: metadata.draft,
            base_ref: metadata.base.reference,
            base_revision: metadata.base.sha,
            comparison_base: comparison.merge_base_commit.sha,
            head_ref: metadata.head.reference,
            head_revision: metadata.head.sha,
            description: metadata.body.unwrap_or_default(),
            changed_files: metadata.changed_files,
            additions: metadata.additions,
            deletions: metadata.deletions,
            file_changes,
            github_url,
        })
    }
}

/// GitHub Pull Request metadata endpoint 的最小响应模型。
#[derive(Deserialize)]
struct PullRequestResponse {
    title: String,
    number: u64,
    user: UserResponse,
    state: String,
    draft: bool,
    merged_at: Option<String>,
    base: RevisionResponse,
    head: RevisionResponse,
    body: Option<String>,
    changed_files: usize,
    additions: usize,
    deletions: usize,
}

/// Pull Request 作者响应片段。
#[derive(Deserialize)]
struct UserResponse {
    login: String,
}

/// Base 或 Head Ref 及其当前精确 revision。
#[derive(Deserialize)]
struct RevisionResponse {
    #[serde(rename = "ref")]
    reference: String,
    sha: String,
}

/// GitHub compare endpoint 返回的 merge-base 片段。
#[derive(Deserialize)]
struct ComparisonResponse {
    merge_base_commit: CommitResponse,
}

/// 仅包含 commit SHA 的响应片段。
#[derive(Deserialize)]
struct CommitResponse {
    sha: String,
}

/// Pull Request files endpoint 中一个 File Change 的响应模型。
#[derive(Deserialize)]
struct FileResponse {
    filename: String,
    previous_filename: Option<String>,
    status: String,
}

fn file_change(response: FileResponse) -> GitHubFileChange {
    GitHubFileChange {
        path: response.filename,
        old_path: response.previous_filename,
        status: match response.status.as_str() {
            "added" => ChangeStatus::Added,
            "removed" => ChangeStatus::Deleted,
            "renamed" => ChangeStatus::Renamed,
            _ => ChangeStatus::Modified,
        },
    }
}
