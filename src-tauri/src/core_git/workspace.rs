use super::*;

impl GitProcessAdapter {
    /// 在精确受管路径创建 detached linked worktree，并校验其 HEAD。
    pub fn create_detached_worktree(
        &self,
        workspace_path: &Path,
        head_object_id: &str,
        cancellation: &CancellationToken,
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
        self.run_workspace(
            [
                OsStr::new("worktree"),
                OsStr::new("add"),
                OsStr::new("--detach"),
                OsStr::new("--"),
                workspace_path.as_os_str(),
                OsStr::new(head_object_id),
            ],
            cancellation,
        )
        .map_err(|error| {
            error
                .with_context("无法创建 Review Workspace。请确认磁盘空间和仓库 worktree 状态后重试")
        })?;
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
            let cleanup = self.remove_worktree(workspace_path, &CancellationToken::default());
            return Err(match cleanup {
                Ok(()) => error,
                Err(cleanup_error) => error.with_context(format!(
                    "自动清理失败：{cleanup_error}。请关闭 Narrant 并检查该仓库的 linked worktrees"
                )),
            });
        }
        Ok(())
    }

    /// 强制重建受管 Review Workspace，并将其恢复为指定 detached HEAD。
    fn force_restore_detached_worktree(
        &self,
        workspace_path: &Path,
        head_object_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        self.remove_worktree(workspace_path, cancellation)?;
        self.create_detached_worktree(workspace_path, head_object_id, cancellation)
    }

    /// 将 Workspace 切换到新 Snapshot；核心阶段失败时恢复旧 Snapshot HEAD。
    pub fn transition_detached_worktree(
        &self,
        workspace_path: &Path,
        expected_common_directory: &Path,
        previous_head_object_id: &str,
        next_head_object_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        self.validate_owned_worktree(workspace_path, expected_common_directory, cancellation)?;
        self.validate_registered_worktree(workspace_path, previous_head_object_id, cancellation)?;
        if let Err(error) =
            self.force_restore_detached_worktree(workspace_path, next_head_object_id, cancellation)
        {
            let rollback = self.force_restore_detached_worktree(
                workspace_path,
                previous_head_object_id,
                &CancellationToken::default(),
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

    /// 验证目标是当前 Repository 注册且位于预期 HEAD 的精确 linked worktree。
    fn validate_registered_worktree(
        &self,
        workspace_path: &Path,
        expected_head_object_id: &str,
        cancellation: &CancellationToken,
    ) -> Result<(), GitCommandError> {
        let output = self.run_workspace(["worktree", "list", "--porcelain", "-z"], cancellation)?;
        let canonical_workspace = workspace_path.canonicalize().map_err(|error| {
            GitCommandError::new(format!("无法验证 Review Workspace canonical path：{error}"))
        })?;
        let expected_path =
            format!("worktree {}", canonical_workspace.to_string_lossy()).into_bytes();
        let expected_head = format!("HEAD {expected_head_object_id}").into_bytes();
        let fields = output.stdout.split(|byte| *byte == 0).collect::<Vec<_>>();
        let registered = fields
            .windows(2)
            .any(|fields| fields[0] == expected_path && fields[1] == expected_head);
        if !registered {
            return Err(GitCommandError::new(
                "拒绝重建 Review Workspace：目标路径、Git worktree 注册或 current Snapshot HEAD 不一致。",
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
        if !workspace_path.exists() {
            return Ok(());
        }
        self.run_workspace(
            [
                OsStr::new("worktree"),
                OsStr::new("remove"),
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

    /// 验证路径确属保存的 common directory，且仍是该仓库精确注册的 linked worktree。
    pub fn validate_owned_worktree(
        &self,
        workspace_path: &Path,
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
        let expected_workspace = std::fs::canonicalize(workspace_path).map_err(|_| {
            GitCommandError::new("Review Workspace 所有权验证失败：受管路径不存在。")
        })?;
        let registered =
            self.run_workspace(["worktree", "list", "--porcelain", "-z"], cancellation)?;
        let path_is_registered = registered.stdout.split(|byte| *byte == 0).any(|field| {
            field
                .strip_prefix(b"worktree ")
                .and_then(|path| std::str::from_utf8(path).ok())
                .and_then(|path| std::fs::canonicalize(path).ok())
                .is_some_and(|path| path == expected_workspace)
        });
        if !path_is_registered {
            return Err(GitCommandError::new(
                "Review Workspace 所有权验证失败：路径不是该仓库注册的 linked worktree。",
            ));
        }
        Ok(())
    }
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
