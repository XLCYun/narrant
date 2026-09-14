use super::*;

/// 一次成功 present 的对外投影与内部可恢复 presentation transition。
pub(super) struct PresentOutcome {
    pub(super) presentation: ReviewPresentation,
    pub(super) transition: PresentationTransition,
}

impl ReviewLifecycle {
    /// 返回 durable Review 当前 Snapshot identity，供跨模块缓存清理绑定旧 revision。
    pub fn snapshot_id(&self, review_id: &str) -> Result<String, ReviewLifecycleError> {
        self.store
            .open_review(review_id)
            .map(|saved| saved.snapshot_identity())
            .map_err(|error| ReviewLifecycleError::ReviewUnavailable(error.to_string()))
    }

    /// 返回已激活且与 Snapshot identity 一致的受管 Review Workspace 路径。
    pub fn workspace_path(
        &self,
        review_id: &str,
        snapshot_id: &str,
    ) -> Result<PathBuf, ReviewLifecycleError> {
        let current = self
            .store
            .open_review(review_id)
            .map_err(|error| ReviewLifecycleError::ReviewUnavailable(error.to_string()))?;
        if current.snapshot_identity() != snapshot_id {
            return Err(ReviewLifecycleError::StaleSnapshot);
        }
        self.runtime
            .lock()
            .map_err(|_| ReviewLifecycleError::RuntimeUnavailable)?
            .open_reviews
            .get(review_id)
            .ok_or_else(|| {
                ReviewLifecycleError::ReviewUnavailable(
                    "Review 已关闭或失效，请重新打开。".to_owned(),
                )
            })?;
        self.store
            .review_workspace_path(review_id)
            .map_err(|error| ReviewLifecycleError::ReviewUnavailable(error.to_string()))
    }

    /// 创建、重开或切换到指定 Review，并在成功后发布运行时读取句柄。
    pub async fn present(
        &self,
        operation: &OperationContext,
        request: PresentReview,
    ) -> Result<ReviewPresentation, ReviewLifecycleError> {
        let _operation_guard = self.enter_operation().await?;
        self.present_guarded(operation, request)
            .await
            .map(|outcome| outcome.presentation)
    }

    /// 在 lifecycle operation gate 内执行完整 present，供复合操作保持 shutdown 原子性。
    pub(super) async fn present_guarded(
        &self,
        operation: &OperationContext,
        request: PresentReview,
    ) -> Result<PresentOutcome, ReviewLifecycleError> {
        let saved = match request {
            PresentReview::Saved { review_id } => self
                .store
                .open_review(&review_id)
                .map_err(|error| ReviewLifecycleError::ReviewUnavailable(error.to_string()))?,
            PresentReview::Branch {
                repository_id,
                base_ref,
                head_ref,
            } => {
                if let Some(saved) = self
                    .store
                    .find_branch_review(repository_id, &base_ref, &head_ref)
                    .map_err(|error| ReviewLifecycleError::new(error.to_string()))?
                {
                    saved
                } else {
                    let repository = self.require_trusted_repository(repository_id)?;
                    let resolved = self
                        .branch_selections
                        .resolve(
                            operation,
                            &repository,
                            &BranchSelection { base_ref, head_ref },
                        )
                        .await
                        .map_err(ReviewLifecycleError::branch)?;
                    self.store
                        .save_branch_review(repository_id, &resolved.comparison)
                        .map_err(|error| ReviewLifecycleError::new(error.to_string()))?
                }
            }
            PresentReview::PullRequest {
                repository_id,
                selection,
            } => {
                if let Some(saved) = self
                    .store
                    .find_pull_request_review(
                        repository_id,
                        &selection.repository.owner,
                        &selection.repository.name,
                        selection.number,
                    )
                    .map_err(|error| ReviewLifecycleError::new(error.to_string()))?
                {
                    saved
                } else {
                    let repository_path = self.require_trusted_repository(repository_id)?;
                    let resolved = self
                        .pull_requests
                        .resolve(operation, &repository_path, &selection)
                        .await
                        .map_err(ReviewLifecycleError::pull_request)?;
                    self.store
                        .save_pull_request_review(
                            repository_id,
                            &resolved.metadata,
                            &resolved.comparison,
                        )
                        .map_err(|error| ReviewLifecycleError::new(error.to_string()))?
                }
            }
        };
        let review_lock = self.review_lock(&saved.review_id)?;
        let _review_guard = review_lock.lock().await;
        // 解析分支或 PR 期间可能发生一次刷新；拿到 Review 锁后重新读取 current。
        let saved = if saved.created {
            saved
        } else {
            self.store
                .open_review(&saved.review_id)
                .map_err(|error| ReviewLifecycleError::ReviewUnavailable(error.to_string()))?
        };
        if operation.cancellation().is_cancelled() {
            return Err(ReviewLifecycleError::Cancelled("操作已取消。".to_owned()));
        }
        let snapshot_id = saved.snapshot_identity();
        if let Some(open_review) = self.open_review_runtime(&saved.review_id)? {
            if open_review.snapshot_id == snapshot_id {
                let transition = self.present_open_review(&saved.review_id)?;
                if !transition.changed {
                    // 当前 Presented Review 的重复 present 必须是完全无副作用的 no-op。
                    return Ok(PresentOutcome {
                        presentation: presentation_from_saved(saved, Vec::new()),
                        transition,
                    });
                }
                // 已打开 Review 只切换内存中的 Presented 指针，不做 Git 物化或清理。
                let cleanup_warnings = self.mark_review_opened(&saved.review_id);
                return Ok(PresentOutcome {
                    presentation: presentation_from_saved(saved, cleanup_warnings),
                    transition,
                });
            }
        }
        self.materialize(saved, operation)
    }

    /// 从已激活的 current Snapshot 读取单个 File Change 内容。
    pub async fn read_file_change(
        &self,
        operation: &OperationContext,
        review_id: &str,
        snapshot_id: &str,
        path: &str,
    ) -> Result<FileChangeContent, ReviewLifecycleError> {
        let current = self
            .store
            .open_review(review_id)
            .map_err(|error| ReviewLifecycleError::ReviewUnavailable(error.to_string()))?;
        if current.snapshot_identity() != snapshot_id {
            return Err(ReviewLifecycleError::StaleSnapshot);
        }
        let open_review = self.open_review_runtime(review_id)?.ok_or_else(|| {
            ReviewLifecycleError::ReviewUnavailable("Review 已关闭或失效，请重新打开。".to_owned())
        })?;
        if open_review.snapshot_id != snapshot_id {
            return Err(ReviewLifecycleError::StaleSnapshot);
        }
        let content = open_review
            .reader
            .read(operation, path)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        Ok(content)
    }

    /// 将 durable current Snapshot 物化为 workspace 与运行时读取句柄。
    fn materialize(
        &self,
        saved: PersistedReviewState,
        operation: &OperationContext,
    ) -> Result<PresentOutcome, ReviewLifecycleError> {
        let repository_path = self.require_trusted_repository(saved.repository_id)?;
        let entry = self
            .repository_application
            .get_repository(saved.repository_id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        let workspace = self
            .store
            .review_workspace_path(&saved.review_id)
            .map_err(|error| ReviewLifecycleError::new(error.to_string()))?;
        let adapter = GitProcessAdapter::new(&repository_path);
        if operation.cancellation().is_cancelled() {
            if saved.created {
                self.store
                    .abandon_refresh_candidate(&saved)
                    .map_err(ReviewLifecycleError::persistence)?;
            }
            return Err(ReviewLifecycleError::Cancelled("操作已取消。".to_owned()));
        }
        let convergence = operation.without_cancellation();
        if workspace.exists() {
            adapter
                .validate_owned_worktree(
                    &workspace,
                    &entry.git_common_directory,
                    convergence.cancellation(),
                )
                .map_err(|error| ReviewLifecycleError::WorkspaceTransition(error.to_string()))?;
        }
        adapter
            .create_detached_worktree(
                &workspace,
                &saved.head_revision,
                convergence.cancellation(),
                convergence.cancellation(),
            )
            .map_err(ReviewLifecycleError::git)?;
        if saved.created {
            if let Err(error) = self.store.activate_initial_snapshot(&saved) {
                let cleanup = adapter.remove_owned_worktree(
                    &workspace,
                    &entry.git_common_directory,
                    convergence.cancellation(),
                );
                return Err(ReviewLifecycleError::new(match cleanup {
                    Ok(()) => error.to_string(),
                    Err(cleanup_error) => {
                        format!("{error}；候选 Workspace 清理失败：{cleanup_error}")
                    }
                }));
            }
        }
        let snapshot = saved.to_change_comparison();
        let reader = Arc::new(SnapshotContentReader::restore(&repository_path, snapshot));
        let transition = self
            .update_open_review_runtime(
                saved.review_id.clone(),
                saved.snapshot_identity(),
                reader,
                OpenReviewRuntimeUpdate::OpenAndPresent,
            )?
            .expect("OpenAndPresent 必须返回 presentation transition");
        let cleanup_warnings = self.mark_review_opened(&saved.review_id);
        Ok(PresentOutcome {
            presentation: presentation_from_saved(saved, cleanup_warnings),
            transition,
        })
    }

    /// 更新 Review 的最近打开时间；失败只保留为非致命诊断，不能撤销已安装的运行时。
    fn mark_review_opened(&self, review_id: &str) -> Vec<String> {
        self.store
            .mark_review_opened(review_id)
            .err()
            .map(|error| vec![error.to_string()])
            .unwrap_or_default()
    }

    /// 返回可用 Repository 的当前 canonical path。
    pub(super) fn require_available_repository(
        &self,
        id: i64,
    ) -> Result<PathBuf, ReviewLifecycleError> {
        self.repository_application
            .require_available_path(id)
            .map_err(Self::repository_application_error)
    }

    /// 返回已授权 Workspace Trust 的可用 repository path。
    pub(super) fn require_trusted_repository(
        &self,
        id: i64,
    ) -> Result<PathBuf, ReviewLifecycleError> {
        self.repository_application
            .require_trusted_path(id)
            .map_err(Self::repository_application_error)
    }

    /// 将 Repository application 的 gate 结果映射为 Review 生命周期错误。
    pub(super) fn repository_application_error(
        error: RepositoryApplicationError,
    ) -> ReviewLifecycleError {
        match error {
            RepositoryApplicationError::Unavailable(message) => {
                ReviewLifecycleError::RepositoryUnavailable(message)
            }
            RepositoryApplicationError::WorkspaceTrustRequired => {
                ReviewLifecycleError::WorkspaceTrustRequired
            }
            RepositoryApplicationError::Persistence(message) => {
                ReviewLifecycleError::Persistence(message)
            }
            RepositoryApplicationError::Operation(message) => {
                ReviewLifecycleError::Operation(message)
            }
        }
    }
}

/// 将 durable Snapshot 转为稳定的生命周期输出。
pub(super) fn presentation_from_saved(
    saved: PersistedReviewState,
    cleanup_warnings: Vec<String>,
) -> ReviewPresentation {
    ReviewPresentation {
        review_id: saved.review_id.clone(),
        snapshot_id: saved.snapshot_identity(),
        snapshot: saved.to_change_comparison(),
        pull_request: saved.pull_request,
        cleanup_warnings,
    }
}
