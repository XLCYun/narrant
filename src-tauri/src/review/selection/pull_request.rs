use crate::core_git::{GitCommandError, GitProcessAdapter};
use crate::github::GitHubAuth;
use crate::github::{self, GitHubRepository};
use crate::operation::OperationContext;
use crate::review::{build_comparison, ChangeComparison, ResolvedRevisions};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;

/// Review 持久化的 Pull Request Selection identity。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestSelection {
    pub repository: GitHubRepository,
    pub number: u64,
}

/// Pull Request Selection 最近同步的协作元数据。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullRequestMetadata {
    pub owner: String,
    pub repository: String,
    pub number: u64,
    pub title: String,
    pub description: String,
    pub author_username: String,
    pub state: PullRequestState,
    pub is_draft: bool,
    pub web_url: String,
    pub base_ref: String,
    pub head_ref: String,
    #[serde(default)]
    pub github_updated_at: Option<String>,
    pub synchronized_at_millis: i64,
}

/// Narrant 归一化后的 Pull Request 生命周期状态。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum PullRequestState {
    Open,
    Closed,
    Merged,
}

/// 一次解析固定下来的 Change Comparison 与同步协作 metadata。
pub struct ResolvedPullRequestSelection {
    pub comparison: ChangeComparison,
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
    #[allow(dead_code, reason = "保留公开仓库与显式匿名解析 interface")]
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
        operation: &OperationContext,
        repository_path: &Path,
        selection: &PullRequestSelection,
    ) -> Result<ResolvedPullRequestSelection, PullRequestResolutionError> {
        let cancellation = operation.cancellation();
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
                () = cancellation.cancelled() => return Err(cancelled()),
            },
            None => None,
        };
        let observation = tokio::select! {
            result = github::observe_pull_request(
                &self.api_base_url,
                &selection.repository,
                selection.number,
                access_token.as_ref(),
            ) => result.map_err(PullRequestResolutionError::GitHub)?,
            () = cancellation.cancelled() => return Err(cancelled()),
        };
        let remotes = github::remote_names_for_repository_with_cancellation(
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
                operation,
            )
            .await
            {
                Ok(comparison) => {
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
                        author_username: observation.author_username,
                        state: match observation.state.as_str() {
                            "open" => PullRequestState::Open,
                            "closed" => PullRequestState::Closed,
                            "merged" => PullRequestState::Merged,
                            _ => {
                                return Err(PullRequestResolutionError::GitHub(
                                    "GitHub 返回了无法识别的 Pull Request 状态。".to_owned(),
                                ))
                            }
                        },
                        is_draft: observation.is_draft,
                        web_url: observation.web_url,
                        base_ref: observation.base.reference,
                        head_ref: observation.head.reference,
                        github_updated_at: Some(observation.github_updated_at),
                        synchronized_at_millis: synchronized_at,
                    };
                    return Ok(ResolvedPullRequestSelection {
                        comparison,
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

/// 区分 remote 获取失败与获取成功后的精确 revision 校验失败。
enum PullRequestObjectResolutionError {
    Fetch(GitCommandError),
    ExactRevision(GitCommandError),
}

impl PullRequestObjectResolutionError {
    /// 返回底层失败是否来自合作式取消。
    fn is_cancelled(&self) -> bool {
        match self {
            Self::Fetch(error) | Self::ExactRevision(error) => error.is_cancelled(),
        }
    }
}

impl std::fmt::Display for PullRequestObjectResolutionError {
    /// 输出底层 Core Git 诊断。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Fetch(error) | Self::ExactRevision(error) => error.fmt(formatter),
        }
    }
}

/// 获取 Pull Request 对象并通过共享 kernel 构建精确 comparison。
async fn resolve_pull_request_objects(
    repository_path: &Path,
    remote: &str,
    pull_request_number: u64,
    base_ref: &str,
    base_revision: &str,
    head_revision: &str,
    operation: &OperationContext,
) -> Result<ChangeComparison, PullRequestObjectResolutionError> {
    if remote.is_empty() || base_ref.is_empty() || pull_request_number == 0 {
        return Err(PullRequestObjectResolutionError::Fetch(
            GitCommandError::new("Pull Request fetch 参数无效"),
        ));
    }
    let adapter = GitProcessAdapter::new(repository_path);
    let base_source = format!("refs/heads/{base_ref}");
    let head_source = format!("refs/pull/{pull_request_number}/head");
    for source in [&base_source, &head_source] {
        adapter
            .run_network(
                [
                    "fetch",
                    "--no-write-fetch-head",
                    "--no-tags",
                    remote,
                    source,
                ],
                operation.cancellation(),
            )
            .map_err(PullRequestObjectResolutionError::Fetch)?;
    }
    build_comparison(
        operation,
        repository_path,
        ResolvedRevisions {
            base_ref: base_ref.to_owned(),
            head_ref: format!("pull/{pull_request_number}/head"),
            base_revision: base_revision.to_owned(),
            head_revision: head_revision.to_owned(),
        },
    )
    .map_err(PullRequestObjectResolutionError::ExactRevision)
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

/// 构造不依赖展示文本的 resolver 取消结果。
fn cancelled() -> PullRequestResolutionError {
    PullRequestResolutionError::Cancelled("Pull Request Selection resolution 已取消。".to_owned())
}

#[cfg(test)]
#[path = "pull_request_tests.rs"]
mod tests;
