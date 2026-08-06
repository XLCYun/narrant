use crate::change_content::local::{LocalOpenedReview, LocalSnapshot};
use crate::change_content::LoadedFileChange;
use crate::core_git::{CancellationToken, GitProcessAdapter};
use crate::github_discovery;
use crate::repository_catalog::{
    PullRequestMetadata, RepositoryCatalog, SavedBranchReview, WorkspaceCleanupTarget,
};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Review 生命周期对外返回的稳定 Snapshot 投影。
pub struct ReviewPresentation {
    pub review_id: String,
    pub snapshot_id: String,
    pub snapshot: LocalSnapshot,
    pub pull_request: Option<PullRequestMetadata>,
    pub cleanup_warnings: Vec<String>,
}

/// 生命周期解析 Pull Request Selection 所需的当前协作观察值。
pub struct PullRequestObservation {
    pub owner: String,
    pub repository: String,
    pub number: u64,
    pub title: String,
    pub description: String,
    pub author: String,
    pub state: String,
    pub draft: bool,
    pub url: String,
    pub updated_at: String,
    pub base_ref: String,
    pub base_revision: String,
    pub head_ref: String,
    pub head_revision: String,
}

/// 打开或创建 Review 时允许的业务输入。
pub enum PresentReview {
    Saved {
        review_id: String,
    },
    Branch {
        repository_entry_id: i64,
        base_ref: String,
        head_ref: String,
    },
    PullRequest {
        repository_entry_id: i64,
        observation: Box<PullRequestObservation>,
    },
}

/// 更新检查使用的来源；Saved Branch 不需要额外协作 metadata。
pub enum UpdateSource {
    Branch,
    PullRequest {
        observation: Box<PullRequestObservation>,
    },
}

/// 用户确认刷新时携带的精确版本证据。
pub enum RefreshEvidence {
    Branch {
        current_base_revision: String,
        current_head_revision: String,
        latest_base_revision: String,
        latest_head_revision: String,
    },
    PullRequest {
        observation: Box<PullRequestObservation>,
    },
}

/// 更新检查结果，只描述 current 与 available，不改变任何 current 状态。
#[derive(Debug)]
pub struct ReviewUpdate {
    pub update_available: bool,
    pub current_base_revision: String,
    pub current_head_revision: String,
    pub available_base_revision: String,
    pub available_head_revision: String,
}

/// Review 逻辑关闭后的 workspace 清理状态。
pub enum WorkspaceCleanup {
    NotNeeded,
    Complete,
    Deferred(String),
}

/// 关闭结果明确区分逻辑关闭和可延后重试的物理清理。
pub struct CloseOutcome {
    pub workspace_cleanup: WorkspaceCleanup,
}

/// 启动恢复报告；候选放弃和 workspace 清理互不阻塞。
pub struct RecoveryReport {
    pub abandoned_snapshot_ids: Vec<String>,
    pub cleanup_warnings: Vec<String>,
}

/// Review 生命周期失败；类别用于调用方决定恢复动作，文本保持既有用户契约。
#[derive(Debug)]
pub enum ReviewLifecycleError {
    RepositoryUnavailable(String),
    WorkspaceTrustRequired,
    ReviewUnavailable(String),
    Busy,
    StaleSnapshot,
    StaleCurrent,
    SelectionMismatch(String),
    Cancelled(String),
    Catalog(String),
    Git(String),
    WorkspaceTransition(String),
    ActivationRollback {
        activation: String,
        rollback: Option<String>,
    },
    RuntimeUnavailable,
    Operation(String),
}

impl ReviewLifecycleError {
    /// 将下层诊断包装为生命周期错误。
    fn new(message: impl Into<String>) -> Self {
        Self::Operation(message.into())
    }

    /// 将 Catalog 失败标记为持久化边界故障。
    fn catalog(error: impl std::fmt::Display) -> Self {
        Self::Catalog(error.to_string())
    }

    /// 将 Core Git 失败分类为取消或普通 Git 故障。
    fn git(error: impl std::fmt::Display) -> Self {
        let message = error.to_string();
        if message.contains("取消") {
            Self::Cancelled(message)
        } else {
            Self::Git(message)
        }
    }
}

impl std::fmt::Display for ReviewLifecycleError {
    /// 输出面向用户且可恢复的中文诊断。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RepositoryUnavailable(message)
            | Self::ReviewUnavailable(message)
            | Self::SelectionMismatch(message)
            | Self::Cancelled(message)
            | Self::Catalog(message)
            | Self::Git(message)
            | Self::WorkspaceTransition(message)
            | Self::Operation(message) => formatter.write_str(message),
            Self::WorkspaceTrustRequired => {
                formatter.write_str("尚未授予 Workspace Trust；请先阅读风险说明并确认授权。")
            }
            Self::Busy => formatter.write_str("此 Review 正在刷新；请等待当前操作完成。"),
            Self::StaleSnapshot => {
                formatter.write_str("Snapshot identity 与 Review 的 current Snapshot 不一致。")
            }
            Self::StaleCurrent => formatter.write_str("current Snapshot 已改变；请重新检查更新。"),
            Self::ActivationRollback {
                activation,
                rollback: None,
            } => formatter.write_str(activation),
            Self::ActivationRollback {
                activation,
                rollback: Some(rollback),
            } => write!(
                formatter,
                "{activation}；恢复旧 Review Workspace 失败：{rollback}"
            ),
            Self::RuntimeUnavailable => formatter.write_str("Review 状态不可用。"),
        }
    }
}

impl std::error::Error for ReviewLifecycleError {}

/// 统一拥有 durable Review、current Snapshot、Workspace 与运行时读取句柄的一致性边界。
pub struct ReviewLifecycle {
    catalog: Arc<RepositoryCatalog>,
    reviews: Mutex<HashMap<String, Arc<Mutex<LocalOpenedReview>>>>,
    active_refreshes: Mutex<HashSet<String>>,
}

impl ReviewLifecycle {
    /// 使用共享 Repository Catalog 创建生命周期服务。
    pub fn new(catalog: Arc<RepositoryCatalog>) -> Self {
        Self {
            catalog,
            reviews: Mutex::new(HashMap::new()),
            active_refreshes: Mutex::new(HashSet::new()),
        }
    }

    /// 返回底层 Catalog，供尚未迁移的只读 Tauri 查询复用同一连接。
    pub fn catalog(&self) -> &RepositoryCatalog {
        &self.catalog
    }

    /// 创建、重开或切换到指定 Review，并在成功后发布运行时读取句柄。
    pub fn present(
        &self,
        request: PresentReview,
        cancellation: &CancellationToken,
    ) -> Result<ReviewPresentation, ReviewLifecycleError> {
        let saved = match request {
            PresentReview::Saved { review_id } => self
                .catalog
                .open_review(&review_id)
                .map_err(|error| ReviewLifecycleError::ReviewUnavailable(error.to_string()))?,
            PresentReview::Branch {
                repository_entry_id,
                base_ref,
                head_ref,
            } => {
                let repository = self.require_trusted_repository(repository_entry_id)?;
                if let Some(saved) = self
                    .catalog
                    .find_branch_review(repository_entry_id, &base_ref, &head_ref)
                    .map_err(|error| ReviewLifecycleError::new(error.to_string()))?
                {
                    saved
                } else {
                    crate::change_content::local::fetch_selected_remote_refs(
                        &repository,
                        &base_ref,
                        &head_ref,
                        cancellation,
                    )
                    .map_err(ReviewLifecycleError::git)?;
                    let opened = LocalOpenedReview::open_with_cancellation(
                        &repository,
                        &base_ref,
                        &head_ref,
                        cancellation,
                    )
                    .map_err(ReviewLifecycleError::git)?;
                    self.catalog
                        .save_branch_review(repository_entry_id, opened.snapshot())
                        .map_err(|error| ReviewLifecycleError::new(error.to_string()))?
                }
            }
            PresentReview::PullRequest {
                repository_entry_id,
                observation,
            } => {
                let repository_path = self.require_trusted_repository(repository_entry_id)?;
                if let Some(saved) = self
                    .catalog
                    .find_pull_request_review(
                        repository_entry_id,
                        &observation.owner,
                        &observation.repository,
                        observation.number,
                    )
                    .map_err(|error| ReviewLifecycleError::new(error.to_string()))?
                {
                    saved
                } else {
                    let (opened, metadata) = calculate_pull_request_review(
                        &repository_path,
                        *observation,
                        cancellation,
                    )?;
                    self.catalog
                        .save_pull_request_review(repository_entry_id, &metadata, opened.snapshot())
                        .map_err(|error| ReviewLifecycleError::new(error.to_string()))?
                }
            }
        };
        self.materialize(saved, cancellation)
    }

    /// 解析最新可用版本，但不创建候选、不切换 workspace、不推进 current。
    pub fn check_update(
        &self,
        review_id: &str,
        source: UpdateSource,
        cancellation: &CancellationToken,
    ) -> Result<ReviewUpdate, ReviewLifecycleError> {
        let current = self
            .catalog
            .open_review(review_id)
            .map_err(|error| ReviewLifecycleError::ReviewUnavailable(error.to_string()))?;
        let repository_path = self.require_available_repository(current.repository_entry_id)?;
        let available = match source {
            UpdateSource::Branch => {
                crate::change_content::local::fetch_selected_remote_refs(
                    &repository_path,
                    &current.base_ref,
                    &current.head_ref,
                    cancellation,
                )
                .map_err(ReviewLifecycleError::git)?;
                LocalOpenedReview::open_with_cancellation(
                    &repository_path,
                    &current.base_ref,
                    &current.head_ref,
                    cancellation,
                )
                .map_err(ReviewLifecycleError::git)?
            }
            UpdateSource::PullRequest { observation } => {
                require_pull_request_identity(&current, &observation)?;
                calculate_pull_request_review(&repository_path, *observation, cancellation)?.0
            }
        };
        Ok(ReviewUpdate {
            update_available: current.base_revision != available.snapshot().base_revision
                || current.head_revision != available.snapshot().head_revision,
            current_base_revision: current.base_revision,
            current_head_revision: current.head_revision,
            available_base_revision: available.snapshot().base_revision.clone(),
            available_head_revision: available.snapshot().head_revision.clone(),
        })
    }

    /// 刷新 Review；准备阶段可取消，开始改变 workspace 后必须激活或回滚到旧版本。
    pub fn refresh(
        &self,
        review_id: &str,
        evidence: RefreshEvidence,
        cancellation: &CancellationToken,
    ) -> Result<ReviewPresentation, ReviewLifecycleError> {
        self.begin_refresh(review_id)?;
        let result = self.refresh_exclusive(review_id, evidence, cancellation);
        self.finish_refresh(review_id);
        result
    }

    /// 从已激活的 current Snapshot 读取单个 File Change 内容。
    pub fn read_file_change(
        &self,
        review_id: &str,
        snapshot_id: &str,
        path: &str,
        cancellation: &CancellationToken,
    ) -> Result<LoadedFileChange, ReviewLifecycleError> {
        let current = self
            .catalog
            .open_review(review_id)
            .map_err(|error| ReviewLifecycleError::ReviewUnavailable(error.to_string()))?;
        if current.snapshot_identity() != snapshot_id {
            return Err(ReviewLifecycleError::StaleSnapshot);
        }
        let review = self
            .reviews
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?
            .get(review_id)
            .cloned()
            .ok_or_else(|| {
                ReviewLifecycleError::ReviewUnavailable(
                    "Review 已关闭或失效，请重新打开。".to_owned(),
                )
            })?;
        let content = review
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?
            .read_file_change_with_cancellation(path, cancellation)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        Ok(content)
    }

    /// 先完成逻辑关闭，再尝试清理 workspace；清理失败不会重新打开 Review。
    pub fn close(&self, review_id: &str) -> Result<CloseOutcome, ReviewLifecycleError> {
        if self.is_refreshing(review_id)? {
            return Err(ReviewLifecycleError::Busy);
        }
        self.reviews
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?
            .remove(review_id);
        let saved = self
            .catalog
            .open_review(review_id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        let target = self.cleanup_target(&saved)?;
        if !target.workspace_path.exists() {
            return Ok(CloseOutcome {
                workspace_cleanup: WorkspaceCleanup::NotNeeded,
            });
        }
        let cleanup = remove_workspace(&target)
            .map_or_else(WorkspaceCleanup::Deferred, |()| WorkspaceCleanup::Complete);
        Ok(CloseOutcome {
            workspace_cleanup: cleanup,
        })
    }

    /// 启动时放弃未激活候选，并逐项清理遗留受管 workspace。
    pub fn recover(&self) -> Result<RecoveryReport, ReviewLifecycleError> {
        let abandoned_snapshot_ids = self
            .catalog
            .abandon_unactivated_candidates()
            .map_err(ReviewLifecycleError::catalog)?;
        let cleanup_warnings = self.cleanup_workspaces(None)?;
        Ok(RecoveryReport {
            abandoned_snapshot_ids,
            cleanup_warnings,
        })
    }

    /// 应用退出时清空运行时句柄，并尽力清理全部 workspace。
    pub fn shutdown(&self) -> Result<Vec<String>, ReviewLifecycleError> {
        self.reviews
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?
            .clear();
        self.cleanup_workspaces(None)
    }

    /// 在刷新互斥区中执行准备、workspace 收敛、激活和必要回滚。
    fn refresh_exclusive(
        &self,
        review_id: &str,
        evidence: RefreshEvidence,
        cancellation: &CancellationToken,
    ) -> Result<ReviewPresentation, ReviewLifecycleError> {
        let current = self
            .catalog
            .open_review(review_id)
            .map_err(|error| ReviewLifecycleError::ReviewUnavailable(error.to_string()))?;
        let repository_path = self.require_available_repository(current.repository_entry_id)?;
        let entry = self
            .catalog
            .open_entry(current.repository_entry_id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        let candidate = match evidence {
            RefreshEvidence::Branch {
                current_base_revision,
                current_head_revision,
                latest_base_revision,
                latest_head_revision,
            } => {
                if current.base_revision != current_base_revision
                    || current.head_revision != current_head_revision
                {
                    return Err(ReviewLifecycleError::StaleCurrent);
                }
                crate::change_content::local::fetch_selected_remote_refs(
                    &repository_path,
                    &current.base_ref,
                    &current.head_ref,
                    cancellation,
                )
                .map_err(ReviewLifecycleError::git)?;
                let available = LocalOpenedReview::open_with_cancellation(
                    &repository_path,
                    &current.base_ref,
                    &current.head_ref,
                    cancellation,
                )
                .map_err(ReviewLifecycleError::git)?;
                if available.snapshot().base_revision != latest_base_revision
                    || available.snapshot().head_revision != latest_head_revision
                {
                    return Err(ReviewLifecycleError::SelectionMismatch(
                        "Branch Selection 在确认后再次变化；请重新检查并确认新版本。".to_owned(),
                    ));
                }
                if current.base_revision == latest_base_revision
                    && current.head_revision == latest_head_revision
                {
                    return Ok(presentation_from_saved(current, Vec::new()));
                }
                self.catalog
                    .save_refresh_snapshot(review_id, available.snapshot())
                    .map_err(|error| ReviewLifecycleError::new(error.to_string()))?
            }
            RefreshEvidence::PullRequest { observation } => {
                require_pull_request_identity(&current, &observation)?;
                let (available, metadata) =
                    calculate_pull_request_review(&repository_path, *observation, cancellation)?;
                if current.base_revision == available.snapshot().base_revision
                    && current.head_revision == available.snapshot().head_revision
                {
                    return Ok(presentation_from_saved(current, Vec::new()));
                }
                self.catalog
                    .stage_pull_request_snapshot(&current, &metadata, available.snapshot())
                    .map_err(|error| ReviewLifecycleError::new(error.to_string()))?
            }
        };

        let workspace = self
            .catalog
            .review_workspace_path(review_id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        let mut reviews = self
            .reviews
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?;
        let convergence = CancellationToken::default();
        let adapter = GitProcessAdapter::new(&repository_path);
        if let Err(error) = adapter.transition_detached_worktree(
            &workspace,
            &entry.common_directory,
            &current.head_revision,
            &candidate.head_revision,
            &convergence,
        ) {
            let _ = self.catalog.abandon_refresh_candidate(&candidate);
            return Err(ReviewLifecycleError::WorkspaceTransition(error.to_string()));
        }
        if let Err(activation_error) = self.catalog.activate_refresh_snapshot(&candidate, &current)
        {
            let rollback = adapter.transition_detached_worktree(
                &workspace,
                &entry.common_directory,
                &candidate.head_revision,
                &current.head_revision,
                &convergence,
            );
            let _ = self.catalog.abandon_refresh_candidate(&candidate);
            return Err(ReviewLifecycleError::ActivationRollback {
                activation: activation_error.to_string(),
                rollback: rollback.err().map(|error| error.to_string()),
            });
        }
        let snapshot = candidate.snapshot();
        reviews.insert(
            review_id.to_owned(),
            Arc::new(Mutex::new(LocalOpenedReview::restore(
                &repository_path,
                snapshot,
            ))),
        );
        Ok(presentation_from_saved(candidate, Vec::new()))
    }

    /// 将 durable current Snapshot 物化为 workspace 与运行时读取句柄。
    fn materialize(
        &self,
        saved: SavedBranchReview,
        cancellation: &CancellationToken,
    ) -> Result<ReviewPresentation, ReviewLifecycleError> {
        let repository_path = self.require_trusted_repository(saved.repository_entry_id)?;
        let entry = self
            .catalog
            .open_entry(saved.repository_entry_id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        let workspace = self
            .catalog
            .review_workspace_path(&saved.review_id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        let mut reviews = self
            .reviews
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?;
        let adapter = GitProcessAdapter::new(&repository_path);
        if workspace.exists() {
            adapter
                .validate_owned_worktree(&workspace, &entry.common_directory, cancellation)
                .map_err(|error| ReviewLifecycleError::WorkspaceTransition(error.to_string()))?;
        }
        adapter
            .create_detached_worktree(&workspace, &saved.head_revision, cancellation)
            .map_err(ReviewLifecycleError::git)?;
        if saved.created {
            if let Err(error) = self.catalog.activate_snapshot(&saved) {
                let cleanup = adapter.remove_owned_worktree(
                    &workspace,
                    &entry.common_directory,
                    &CancellationToken::default(),
                );
                return Err(ReviewLifecycleError::new(match cleanup {
                    Ok(()) => error.to_string(),
                    Err(cleanup_error) => {
                        format!("{error}；候选 Workspace 清理失败：{cleanup_error}")
                    }
                }));
            }
        }
        let snapshot = saved.snapshot();
        reviews.insert(
            saved.review_id.clone(),
            Arc::new(Mutex::new(LocalOpenedReview::restore(
                &repository_path,
                snapshot,
            ))),
        );
        reviews.retain(|review_id, _| review_id == &saved.review_id);
        drop(reviews);
        let mut cleanup_warnings = Vec::new();
        if let Err(error) = self.catalog.mark_review_opened(&saved.review_id) {
            cleanup_warnings.push(error.to_string());
        }
        cleanup_warnings.extend(self.cleanup_workspaces(Some(&saved.review_id))?);
        Ok(presentation_from_saved(saved, cleanup_warnings))
    }

    /// 返回可用 Repository Entry 的当前 canonical path。
    fn require_available_repository(&self, id: i64) -> Result<PathBuf, ReviewLifecycleError> {
        let entry = self
            .catalog
            .open_entry(id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        if entry.available {
            Ok(entry.canonical_path)
        } else {
            Err(ReviewLifecycleError::RepositoryUnavailable(
                entry
                    .unavailable_reason
                    .unwrap_or_else(|| "Repository Entry 当前不可用。".to_owned()),
            ))
        }
    }

    /// 返回已授权 Workspace Trust 的可用 repository path。
    fn require_trusted_repository(&self, id: i64) -> Result<PathBuf, ReviewLifecycleError> {
        let repository = self.require_available_repository(id)?;
        if !self
            .catalog
            .is_workspace_trusted(id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?
        {
            return Err(ReviewLifecycleError::WorkspaceTrustRequired);
        }
        Ok(repository)
    }

    /// 获取同一 Review 的独占刷新权。
    fn begin_refresh(&self, review_id: &str) -> Result<(), ReviewLifecycleError> {
        let mut active = self
            .active_refreshes
            .lock()
            .map_err(|_| ReviewLifecycleError::new("Review 刷新状态不可用。"))?;
        if !active.insert(review_id.to_owned()) {
            return Err(ReviewLifecycleError::Busy);
        }
        Ok(())
    }

    /// 释放同一 Review 的独占刷新权。
    fn finish_refresh(&self, review_id: &str) {
        if let Ok(mut active) = self.active_refreshes.lock() {
            active.remove(review_id);
        }
    }

    /// 查询 Review 当前是否正在刷新。
    fn is_refreshing(&self, review_id: &str) -> Result<bool, ReviewLifecycleError> {
        self.active_refreshes
            .lock()
            .map(|active| active.contains(review_id))
            .map_err(|_| ReviewLifecycleError::new("Review 刷新状态不可用。"))
    }

    /// 构造某个 Review 的精确清理目标。
    fn cleanup_target(
        &self,
        saved: &SavedBranchReview,
    ) -> Result<WorkspaceCleanupTarget, ReviewLifecycleError> {
        let entry = self
            .catalog
            .open_entry(saved.repository_entry_id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        Ok(WorkspaceCleanupTarget {
            review_id: saved.review_id.clone(),
            repository_path: entry.canonical_path,
            common_directory: entry.common_directory,
            workspace_path: self
                .catalog
                .review_workspace_path(&saved.review_id)
                .map_err(|error| ReviewLifecycleError::new(error.to_string()))?,
        })
    }

    /// 尽力清理除指定 Review 外的受管 workspace，并保留全部可恢复诊断。
    fn cleanup_workspaces(
        &self,
        except_review_id: Option<&str>,
    ) -> Result<Vec<String>, ReviewLifecycleError> {
        let targets = self
            .catalog
            .workspace_cleanup_targets()
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        Ok(targets
            .into_iter()
            .filter(|target| {
                !except_review_id.is_some_and(|review_id| review_id == target.review_id)
                    && target.workspace_path.exists()
            })
            .filter_map(|target| remove_workspace(&target).err())
            .collect())
    }
}

/// 将 durable Snapshot 转为稳定的生命周期输出。
fn presentation_from_saved(
    saved: SavedBranchReview,
    cleanup_warnings: Vec<String>,
) -> ReviewPresentation {
    ReviewPresentation {
        review_id: saved.review_id.clone(),
        snapshot_id: saved.snapshot_identity(),
        snapshot: saved.snapshot(),
        pull_request: saved.pull_request,
        cleanup_warnings,
    }
}

/// 验证 Pull Request 请求没有改变 durable Selection identity。
fn require_pull_request_identity(
    current: &SavedBranchReview,
    observation: &PullRequestObservation,
) -> Result<(), ReviewLifecycleError> {
    let saved = current.pull_request.as_ref().ok_or_else(|| {
        ReviewLifecycleError::SelectionMismatch(
            "该 Review 不是 Pull Request Selection。".to_owned(),
        )
    })?;
    if !saved.owner.eq_ignore_ascii_case(&observation.owner)
        || !saved
            .repository
            .eq_ignore_ascii_case(&observation.repository)
        || saved.number != observation.number
    {
        return Err(ReviewLifecycleError::SelectionMismatch(
            "Pull Request Selection identity 与 durable Review 不一致。".to_owned(),
        ));
    }
    Ok(())
}

/// 将 GitHub metadata 与 Core Git transport 收敛为固定候选 Snapshot。
fn calculate_pull_request_review(
    repository_path: &Path,
    observation: PullRequestObservation,
    cancellation: &CancellationToken,
) -> Result<(LocalOpenedReview, PullRequestMetadata), ReviewLifecycleError> {
    let repository = github_discovery::GitHubRepository {
        owner: observation.owner.clone(),
        name: observation.repository.clone(),
    };
    let remote = github_discovery::remote_names_for_repository(repository_path, &repository)
        .map_err(ReviewLifecycleError::new)?
        .into_iter()
        .next()
        .ok_or_else(|| {
            ReviewLifecycleError::new(format!(
                "Git transport：找不到代表 base repository {}/{} 的 Git remote。",
                repository.owner, repository.name
            ))
        })?;
    let calculated = crate::change_content::local::fetch_pull_request_objects(
        repository_path,
        &remote,
        observation.number,
        &observation.base_ref,
        &observation.base_revision,
        &observation.head_revision,
        cancellation,
    )
    .map_err(|error| ReviewLifecycleError::git(format!("Git transport：{error}")))?;
    let metadata = PullRequestMetadata {
        owner: observation.owner,
        repository: observation.repository,
        number: observation.number,
        title: observation.title,
        description: observation.description,
        author: observation.author,
        state: observation.state,
        draft: observation.draft,
        url: observation.url,
        base_ref: observation.base_ref,
        head_ref: observation.head_ref,
        updated_at: Some(observation.updated_at),
        synchronized_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| {
                ReviewLifecycleError::new("系统时间早于 Unix epoch，无法保存 GitHub metadata。")
            })?
            .as_millis()
            .try_into()
            .map_err(|_| ReviewLifecycleError::new("系统时间超出可保存范围。"))?,
    };
    Ok((calculated, metadata))
}

/// 精确移除已验证归属的受管 workspace。
fn remove_workspace(target: &WorkspaceCleanupTarget) -> Result<(), String> {
    GitProcessAdapter::new(&target.repository_path)
        .remove_owned_worktree(
            &target.workspace_path,
            &target.common_directory,
            &CancellationToken::default(),
        )
        .map_err(|error| format!("{}：{error}", target.review_id))
}

#[cfg(test)]
#[path = "review_lifecycle_tests.rs"]
mod tests;
