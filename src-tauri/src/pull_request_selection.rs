use crate::change_content::local::{
    resolve_pull_request_objects, LocalSnapshot, PullRequestObjectResolutionError,
};
use crate::core_git::CancellationToken;
use crate::github_auth::GitHubAuth;
use crate::github_discovery::{self, GitHubRepository};
use crate::repository_catalog::PullRequestMetadata;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Review 持久化的 Pull Request Selection identity。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestSelection {
    pub repository: GitHubRepository,
    pub number: u64,
}

/// 一次解析固定下来的本地 Snapshot 与同步协作 metadata。
pub struct ResolvedPullRequestSelection {
    pub snapshot: LocalSnapshot,
    pub metadata: PullRequestMetadata,
}

/// Pull Request Selection 解析失败，保留外部观察与本地 transport 的故障类别。
#[derive(Debug)]
pub enum PullRequestResolutionError {
    InvalidSelection,
    GitHub(String),
    Git(String),
    Cancelled(String),
    MissingRemote(String),
    GitTransport { attempts: Vec<(String, String)> },
    ExactRevision { attempts: Vec<(String, String)> },
    Clock,
}

impl std::fmt::Display for PullRequestResolutionError {
    /// 输出 Tauri transport 可直接呈现的中文诊断。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidSelection => formatter.write_str("Pull Request Selection identity 无效。"),
            Self::GitHub(message) => write!(formatter, "GitHub observation：{message}"),
            Self::Git(message) => write!(formatter, "Git transport：{message}"),
            Self::Cancelled(message) => formatter.write_str(message),
            Self::MissingRemote(identity) => write!(
                formatter,
                "Git transport：找不到代表 base repository {identity} 的 Git remote。"
            ),
            Self::GitTransport { attempts } => {
                formatter.write_str("Git transport：所有匹配 remote 均无法解析 Pull Request")?;
                for (remote, error) in attempts {
                    write!(formatter, "；{remote}: {error}")?;
                }
                Ok(())
            }
            Self::ExactRevision { attempts } => {
                formatter.write_str("精确 Pull Request revisions 无法在本地验证")?;
                for (remote, error) in attempts {
                    write!(formatter, "；{remote}: {error}")?;
                }
                Ok(())
            }
            Self::Clock => formatter.write_str("系统时间超出可保存范围。"),
        }
    }
}

impl std::error::Error for PullRequestResolutionError {}

/// 将 GitHub observation、remote fallback、对象获取与三点比较隐藏在一个 seam 后。
pub struct PullRequestSelectionResolver {
    api_base_url: String,
    auth: Option<Arc<GitHubAuth>>,
}

impl PullRequestSelectionResolver {
    /// 创建使用匿名 GitHub observation 的 resolver，主要用于公开仓库与测试。
    pub fn anonymous(api_base_url: impl Into<String>) -> Self {
        Self {
            api_base_url: api_base_url.into(),
            auth: None,
        }
    }

    /// 创建复用应用 GitHub authorization 的生产 resolver。
    pub fn authenticated(api_base_url: impl Into<String>, auth: Arc<GitHubAuth>) -> Self {
        Self {
            api_base_url: api_base_url.into(),
            auth: Some(auth),
        }
    }

    /// 将稳定 Selection identity 解析成当前、精确且已在本地验证的 Snapshot。
    pub async fn resolve(
        &self,
        repository_path: &Path,
        selection: &PullRequestSelection,
        cancellation: &CancellationToken,
    ) -> Result<ResolvedPullRequestSelection, PullRequestResolutionError> {
        if selection.number == 0
            || selection.repository.owner.trim().is_empty()
            || selection.repository.name.trim().is_empty()
        {
            return Err(PullRequestResolutionError::InvalidSelection);
        }
        require_not_cancelled(cancellation)?;
        let access_token = match &self.auth {
            Some(auth) => tokio::select! {
                result = auth.access_token() => result.map_err(PullRequestResolutionError::GitHub)?,
                () = wait_for_cancellation(cancellation) => return Err(cancelled()),
            },
            None => None,
        };
        let observation = tokio::select! {
            result = github_discovery::observe_pull_request(
                &self.api_base_url,
                &selection.repository,
                selection.number,
                access_token.as_ref(),
            ) => result.map_err(PullRequestResolutionError::GitHub)?,
            () = wait_for_cancellation(cancellation) => return Err(cancelled()),
        };
        let remotes = github_discovery::remote_names_for_repository_with_cancellation(
            repository_path,
            &selection.repository,
            cancellation,
        )
        .map_err(|error| {
            if error.is_cancelled() {
                PullRequestResolutionError::Cancelled(error.to_string())
            } else {
                PullRequestResolutionError::Git(error.to_string())
            }
        })?;
        if remotes.is_empty() {
            return Err(PullRequestResolutionError::MissingRemote(
                selection.repository.identity(),
            ));
        }

        let mut transport_attempts = Vec::new();
        let mut revision_attempts = Vec::new();
        for remote in remotes {
            match resolve_pull_request_objects(
                repository_path,
                &remote,
                selection.number,
                &observation.base.reference,
                &observation.base.sha,
                &observation.head.sha,
                cancellation,
            ) {
                Ok(opened) => {
                    let synchronized_at = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map_err(|_| PullRequestResolutionError::Clock)?
                        .as_millis()
                        .try_into()
                        .map_err(|_| PullRequestResolutionError::Clock)?;
                    let metadata = PullRequestMetadata {
                        owner: selection.repository.owner.clone(),
                        repository: selection.repository.name.clone(),
                        number: selection.number,
                        title: observation.title,
                        description: observation.description,
                        author: observation.author,
                        state: observation.state,
                        draft: observation.draft,
                        url: observation.url,
                        base_ref: observation.base.reference,
                        head_ref: observation.head.reference,
                        updated_at: Some(observation.updated_at),
                        synchronized_at,
                    };
                    return Ok(ResolvedPullRequestSelection {
                        snapshot: opened.snapshot().clone(),
                        metadata,
                    });
                }
                Err(error) if error.is_cancelled() => {
                    return Err(PullRequestResolutionError::Cancelled(error.to_string()));
                }
                Err(PullRequestObjectResolutionError::Fetch(error)) => {
                    transport_attempts.push((remote, error.to_string()));
                }
                Err(PullRequestObjectResolutionError::ExactRevision(error)) => {
                    revision_attempts.push((remote, error.to_string()));
                }
            }
        }
        if revision_attempts.is_empty() {
            Err(PullRequestResolutionError::GitTransport {
                attempts: transport_attempts,
            })
        } else {
            revision_attempts.extend(transport_attempts);
            Err(PullRequestResolutionError::ExactRevision {
                attempts: revision_attempts,
            })
        }
    }
}

/// 在任何外部工作开始前响应已经发生的取消。
fn require_not_cancelled(
    cancellation: &CancellationToken,
) -> Result<(), PullRequestResolutionError> {
    if cancellation.is_cancelled() {
        Err(cancelled())
    } else {
        Ok(())
    }
}

/// 等待共享 cancellation token，供异步 GitHub 请求参与 select。
async fn wait_for_cancellation(cancellation: &CancellationToken) {
    while !cancellation.is_cancelled() {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

/// 构造不依赖展示文本的 resolver 取消结果。
fn cancelled() -> PullRequestResolutionError {
    PullRequestResolutionError::Cancelled("Pull Request Selection resolution 已取消。".to_owned())
}

#[cfg(test)]
#[path = "pull_request_selection_tests.rs"]
mod tests;
