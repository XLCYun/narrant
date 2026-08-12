use super::*;

impl ReviewLifecycle {
    /// 创建、重开或切换到指定 Review，并在成功后发布运行时读取句柄。
    pub async fn present(
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
                    let operation = OperationContext::from_cancellation(cancellation.clone());
                    let resolved = self
                        .branch_selections
                        .resolve(
                            &repository,
                            &BranchSelection { base_ref, head_ref },
                            &operation,
                        )
                        .await
                        .map_err(ReviewLifecycleError::branch)?;
                    self.catalog
                        .save_branch_review(repository_entry_id, &resolved.snapshot)
                        .map_err(|error| ReviewLifecycleError::new(error.to_string()))?
                }
            }
            PresentReview::PullRequest {
                repository_entry_id,
                selection,
            } => {
                let repository_path = self.require_trusted_repository(repository_entry_id)?;
                if let Some(saved) = self
                    .catalog
                    .find_pull_request_review(
                        repository_entry_id,
                        &selection.repository.owner,
                        &selection.repository.name,
                        selection.number,
                    )
                    .map_err(|error| ReviewLifecycleError::new(error.to_string()))?
                {
                    saved
                } else {
                    let operation = OperationContext::from_cancellation(cancellation.clone());
                    let resolved = self
                        .pull_requests
                        .resolve(&repository_path, &selection, &operation)
                        .await
                        .map_err(ReviewLifecycleError::pull_request)?;
                    self.catalog
                        .save_pull_request_review(
                            repository_entry_id,
                            &resolved.metadata,
                            &resolved.snapshot,
                        )
                        .map_err(|error| ReviewLifecycleError::new(error.to_string()))?
                }
            }
        };
        self.materialize(saved, cancellation)
    }

    /// 从已激活的 current Snapshot 读取单个 File Change 内容。
    pub async fn read_file_change(
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
        let operation = OperationContext::from_cancellation(cancellation.clone());
        let content = review
            .read(path, &operation)
            .await
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        Ok(content)
    }

    /// 将 durable current Snapshot 物化为 workspace 与运行时读取句柄。
    fn materialize(
        &self,
        saved: SavedBranchReview,
        cancellation: &CancellationToken,
    ) -> Result<ReviewPresentation, ReviewLifecycleError> {
        let repository_path = self.require_trusted_repository(saved.repository_entry_id)?;
        let entry = self
            .repository_application
            .inspect_entry(saved.repository_entry_id)
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
            Arc::new(LocalSnapshotReader::restore(&repository_path, snapshot)),
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
    pub(super) fn require_available_repository(
        &self,
        id: i64,
    ) -> Result<PathBuf, ReviewLifecycleError> {
        self.repository_application
            .require_available_path(id)
            .map_err(Self::repository_application_error)
    }

    /// 返回已授权 Workspace Trust 的可用 repository path。
    fn require_trusted_repository(&self, id: i64) -> Result<PathBuf, ReviewLifecycleError> {
        self.repository_application
            .require_trusted_path(id)
            .map_err(Self::repository_application_error)
    }

    /// 将 Repository application 的 gate 结果映射为 Review 生命周期错误。
    fn repository_application_error(error: RepositoryApplicationError) -> ReviewLifecycleError {
        match error {
            RepositoryApplicationError::Unavailable(message) => {
                ReviewLifecycleError::RepositoryUnavailable(message)
            }
            RepositoryApplicationError::WorkspaceTrustRequired => {
                ReviewLifecycleError::WorkspaceTrustRequired
            }
            RepositoryApplicationError::Catalog(message) => ReviewLifecycleError::Catalog(message),
            RepositoryApplicationError::Operation(message) => {
                ReviewLifecycleError::Operation(message)
            }
        }
    }
}

/// 将 durable Snapshot 转为稳定的生命周期输出。
pub(super) fn presentation_from_saved(
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
