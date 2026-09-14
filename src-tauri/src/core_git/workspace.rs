use super::*;
use tokio_util::sync::CancellationToken;

impl GitProcessAdapter {
    /// 检查 checkout 的仓库归属、精确注册路径、HEAD 及已跟踪内容相对 HEAD 的一致性。
    /// 此次状态检查不覆盖未跟踪文件，也不提供分析期间的文件系统快照隔离。
    pub(crate) fn validate_worktree_revision(
        &self,
        workspace_path: &Path,
        expected_common_directory: &Path,
        expected_head_object_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        self.validate_registered_worktree(
            workspace_path,
            expected_common_directory,
            expected_head_object_id,
            cancellation,
        )?;
        Self::new(workspace_path)
            .run(["diff", "--quiet", "HEAD", "--"], cancellation)
            .map(|_| ())
            .map_err(|error| error.with_context("Review checkout 的已跟踪源码与固定提交不一致"))
    }

    /// 在精确受管路径创建 detached linked worktree，并校验其 HEAD。
    pub fn create_detached_worktree(
        &self,
        workspace_path: &Path,
        head_object_id: &str,
        cancellation: &CancellationToken,
        convergence_cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        if workspace_path.exists() {
            let actual_head = read_head_object_id(workspace_path, cancellation)?;
            if actual_head == head_object_id {
                return Ok(());
            }
            return Err(GitCommandError::new(
                "Review Workspace 已存在但 HEAD 与 current Snapshot 不一致；请关闭 Review 后重试。",
            ));
        }
        let parent = workspace_path
            .parent()
            .ok_or_else(|| GitCommandError::new("Review Workspace 路径无效"))?;
        std::fs::create_dir_all(parent).map_err(|error| {
            GitCommandError::new(format!("无法创建 Review Workspace 目录：{error}"))
        })?;
        let creation = self.run_workspace_mutation(
            [
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("--detach"),
                OsStr::new("--"),
                workspace_path.as_os_str(),
                OsStr::new(head_object_id),
            ],
            cancellation,
        );
        if let Err(error) = creation {
            let error = error.with_context(
                "无法创建 Review Workspace。请确认磁盘空间和仓库 worktree 状态后重试",
            );
            return Err(self.compensate_failed_worktree_creation(
                workspace_path,
                error,
                convergence_cancellation,
            ));
        }
        let postcondition = read_head_object_id(workspace_path, cancellation).and_then(|actual| {
            if actual == head_object_id {
                Ok(())
            } else {
                Err(GitCommandError::new(
                    "Review Workspace HEAD 与 Snapshot head object ID 不一致。",
                ))
            }
        });
        if let Err(error) = postcondition {
            return Err(self.compensate_failed_worktree_creation(
                workspace_path,
                error,
                convergence_cancellation,
            ));
        }
        Ok(())
    }

    /// 使用调用方派生的收敛信号补偿失败创建，并保留原始失败类别。
    fn compensate_failed_worktree_creation(
        &self,
        workspace_path: &Path,
        error: GitCommandError,
        convergence_cancellation: &CancellationToken,
    ) -> GitCommandError {
        match self.remove_worktree(workspace_path, convergence_cancellation) {
            Ok(()) => error,
            Err(cleanup_error) => error.with_context(format!(
                "自动清理失败：{cleanup_error}。请关闭 Narrant 并检查该仓库的 linked worktrees"
            )),
        }
    }

    /// 强制重建受管 Review Workspace，并将其恢复为指定 detached HEAD。
    fn force_restore_detached_worktree(
        &self,
        workspace_path: &Path,
        head_object_id: &str,
        cancellation: &CancellationToken,
        convergence_cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        self.remove_worktree(workspace_path, cancellation)?;
        self.create_detached_worktree(
            workspace_path,
            head_object_id,
            cancellation,
            convergence_cancellation,
        )
    }

    /// 将 Workspace 切换到新 Snapshot；核心阶段失败时恢复旧 Snapshot HEAD。
    pub fn transition_detached_worktree(
        &self,
        workspace_path: &Path,
        expected_common_directory: &Path,
        previous_head_object_id: &str,
        next_head_object_id: &str,
        cancellation: &CancellationToken,
        convergence_cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        self.validate_registered_worktree(
            workspace_path,
            expected_common_directory,
            previous_head_object_id,
            cancellation,
        )?;
        if let Err(error) = self.force_restore_detached_worktree(
            workspace_path,
            next_head_object_id,
            cancellation,
            convergence_cancellation,
        ) {
            let rollback = self.force_restore_detached_worktree(
                workspace_path,
                previous_head_object_id,
                convergence_cancellation,
                convergence_cancellation,
            );
            return Err(match rollback {
                Ok(()) => error,
                Err(rollback_error) => {
                    error.with_context(format!("恢复旧 Review Workspace 失败：{rollback_error}"))
                }
            });
        }
        Ok(())
    }

    /// 先确认仓库 common directory，再用同一份清单核对精确路径及该记录的 HEAD。
    fn validate_registered_worktree(
        &self,
        workspace_path: &Path,
        expected_common_directory: &Path,
        expected_head_object_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        self.validate_common_directory(expected_common_directory, cancellation)?;
        let output =
            self.run_workspace_inspect(["worktree", "list", "--porcelain", "-z"], cancellation)?;
        let canonical_workspace = workspace_path.canonicalize().map_err(|error| {
            GitCommandError::new(format!("无法验证 Review Workspace canonical path：{error}"))
        })?;
        let expected_head = format!("HEAD {expected_head_object_id}").into_bytes();
        let mut fields = output.stdout.split(|byte| *byte == 0);
        if !fields
            .by_ref()
            .any(|field| worktree_field_matches_path(field, &canonical_workspace))
        {
            return Err(GitCommandError::new(
                "Review Workspace 所有权验证失败：路径不是该仓库注册的 linked worktree。",
            ));
        }
        if fields.next() != Some(expected_head.as_slice()) {
            return Err(GitCommandError::new(
                "Review Workspace 校验失败：目标路径的 Git worktree HEAD 与 current Snapshot 不一致。",
            ));
        }
        Ok(())
    }

    /// 精确移除调用方已验证归属的 disposable linked worktree。
    pub fn remove_worktree(
        &self,
        workspace_path: &Path,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        if !workspace_path.exists() && !self.is_registered_worktree(workspace_path, cancellation)? {
            return Ok(());
        }
        self.run_workspace_mutation(
            [
                OsStr::new("worktree"),
                OsStr::new("remove"),
                OsStr::new("--force"),
                OsStr::new("--force"),
                OsStr::new("--"),
                workspace_path.as_os_str(),
            ],
            cancellation,
        )
        .map(|_| ())
        .map_err(|error| error.with_context("无法移除 Review Workspace"))
    }

    /// 在 common directory 与 registered worktree path 精确匹配后强制移除受管 workspace。
    pub fn remove_owned_worktree(
        &self,
        workspace_path: &Path,
        expected_common_directory: &Path,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        self.validate_owned_worktree(workspace_path, expected_common_directory, cancellation)?;
        self.remove_worktree(workspace_path, cancellation)
    }

    /// 恢复阶段仅在路径存在或仍有 Git 注册时验证并移除受管 worktree。
    pub fn remove_owned_worktree_if_present(
        &self,
        workspace_path: &Path,
        expected_common_directory: &Path,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        if !workspace_path.exists() && !self.is_registered_worktree(workspace_path, cancellation)? {
            return Ok(());
        }
        self.remove_owned_worktree(workspace_path, expected_common_directory, cancellation)
    }

    /// 验证路径确属保存的 common directory，且仍是该仓库精确注册的 linked worktree。
    pub fn validate_owned_worktree(
        &self,
        workspace_path: &Path,
        expected_common_directory: &Path,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        self.validate_common_directory(expected_common_directory, cancellation)?;
        if !self.is_registered_worktree(workspace_path, cancellation)? {
            return Err(GitCommandError::new(
                "Review Workspace 所有权验证失败：路径不是该仓库注册的 linked worktree。",
            ));
        }
        Ok(())
    }

    /// 对照保存的 canonical common directory 验证当前 Repository 的身份。
    fn validate_common_directory(
        &self,
        expected_common_directory: &Path,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        let actual_common = self
            .run(
                ["rev-parse", "--path-format=absolute", "--git-common-dir"],
                cancellation,
            )
            .and_then(|output| {
                String::from_utf8(output.stdout)
                    .map(|value| PathBuf::from(value.trim()))
                    .map_err(|_| GitCommandError::new("Git common directory 编码无效"))
            })?;
        let actual_common = std::fs::canonicalize(&actual_common).map_err(|error| {
            GitCommandError::new(format!("无法解析 Git common directory：{error}"))
        })?;
        let expected_common =
            std::fs::canonicalize(expected_common_directory).map_err(|error| {
                GitCommandError::new(format!("无法解析保存的 Git common directory：{error}"))
            })?;
        if actual_common != expected_common {
            return Err(GitCommandError::new(
                "Review Workspace 所有权验证失败：Git common directory 不匹配。",
            ));
        }
        Ok(())
    }

    /// 通过机器格式 worktree 清单判断精确受管路径是否仍有 Git 注册。
    pub(crate) fn is_registered_worktree(
        &self,
        workspace_path: &Path,
        cancellation: &CancellationToken,
    ) -> Result<bool, GitCommandError> {
        let expected_workspace = canonicalize_with_missing_tail(workspace_path)?;
        let registered =
            self.run_workspace_inspect(["worktree", "list", "--porcelain", "-z"], cancellation)?;
        Ok(registered
            .stdout
            .split(|byte| *byte == 0)
            .any(|field| worktree_field_matches_path(field, &expected_workspace)))
    }
}

/** 比较 Git porcelain worktree 字段与本地规范路径，并消除 Windows 路径表示差异。 */
pub(crate) fn worktree_field_matches_path(field: &[u8], expected: &Path) -> bool {
    let Some(actual) = field.strip_prefix(b"worktree ") else {
        return false;
    };
    #[cfg(windows)]
    {
        let Ok(actual) = std::str::from_utf8(actual) else {
            return false;
        };
        canonicalize_with_missing_tail(Path::new(actual)).is_ok_and(|actual| actual == expected)
    }
    #[cfg(not(windows))]
    {
        actual == expected.to_string_lossy().as_bytes()
    }
}

/// 规范化可能已经消失的受管路径，同时保留其不存在的尾部组件。
fn canonicalize_with_missing_tail(path: &Path) -> Result<PathBuf, GitCommandError> {
    let mut existing = path;
    let mut missing = Vec::new();
    while !existing.exists() {
        let name = existing.file_name().ok_or_else(|| {
            GitCommandError::new("Review Workspace 所有权验证失败：无法规范化受管路径。")
        })?;
        missing.push(name.to_os_string());
        existing = existing.parent().ok_or_else(|| {
            GitCommandError::new("Review Workspace 所有权验证失败：无法规范化受管路径。")
        })?;
    }
    let mut canonical = std::fs::canonicalize(existing).map_err(|error| {
        GitCommandError::new(format!(
            "Review Workspace 所有权验证失败：无法规范化受管路径：{error}"
        ))
    })?;
    for component in missing.into_iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}

/// 读取 linked worktree 的精确 HEAD object ID，供创建前后统一校验。
fn read_head_object_id(
    workspace_path: &Path,
    cancellation: &CancellationToken,
) -> Result<String, GitCommandError> {
    GitProcessAdapter::new(workspace_path)
        .run(["rev-parse", "HEAD"], cancellation)
        .and_then(|output| {
            String::from_utf8(output.stdout)
                .map(|value| value.trim().to_owned())
                .map_err(|_| GitCommandError::new("Review Workspace HEAD 编码无效"))
        })
}
