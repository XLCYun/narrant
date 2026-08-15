use super::RepositoryApplicationError;
use crate::core_git::{CancellationToken, GitProcessAdapter};
use crate::persistence::RepositoryLocation;
use std::path::{Path, PathBuf};

/// 通过机器可读 Core Git 输出验证 working tree、canonical path、common directory 和能力。
pub(crate) fn verify_repository_location(
    selected_path: &Path,
) -> Result<RepositoryLocation, RepositoryApplicationError> {
    let selected_path = selected_path.canonicalize().map_err(|error| {
        RepositoryApplicationError::Unavailable(format!("Repository 路径不存在或不可访问：{error}"))
    })?;
    let adapter = GitProcessAdapter::new(&selected_path);
    let cancellation = CancellationToken::default();
    let output = adapter
        .run(
            [
                "rev-parse",
                "--is-inside-work-tree",
                "--show-toplevel",
                "--git-common-dir",
                "--show-object-format",
            ],
            &cancellation,
        )
        .map_err(|error| {
            RepositoryApplicationError::Operation(format!(
        "所选路径不是兼容的 working-tree Git 仓库；请确认已安装支持所需能力的 Core Git：{error}"
    ))
        })?;
    let lines = String::from_utf8(output.stdout)
        .map_err(|_| {
            RepositoryApplicationError::Operation("Core Git 返回了无效的仓库路径编码。".into())
        })?
        .lines()
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if lines.len() != 4 || lines[0] != "true" || lines[3].is_empty() {
        return Err(RepositoryApplicationError::Operation(
            "所选路径不是兼容的 working-tree Git 仓库。".into(),
        ));
    }
    let canonical_path = PathBuf::from(&lines[1]).canonicalize().map_err(|error| {
        RepositoryApplicationError::Operation(format!(
            "无法解析 canonical repository path：{error}"
        ))
    })?;
    let common_path = PathBuf::from(&lines[2]);
    let git_common_directory = if common_path.is_absolute() {
        common_path
    } else {
        canonical_path.join(common_path)
    }
    .canonicalize()
    .map_err(|error| {
        RepositoryApplicationError::Operation(format!("无法解析 Git common directory：{error}"))
    })?;
    verify_required_git_capabilities(&adapter, &cancellation)?;
    Ok(RepositoryLocation {
        canonical_path,
        git_common_directory,
    })
}

/// 只读探测 Repository 页面立即依赖的 refs、remotes 与 worktree 能力。
fn verify_required_git_capabilities(
    adapter: &GitProcessAdapter,
    cancellation: &CancellationToken,
) -> Result<(), RepositoryApplicationError> {
    for arguments in [
        vec![
            "for-each-ref",
            "--format=%(refname)%00",
            "refs/heads",
            "refs/remotes",
        ],
        vec!["remote"],
        vec!["worktree", "list", "--porcelain", "-z"],
    ] {
        adapter.run(arguments, cancellation).map_err(|error| {
            RepositoryApplicationError::Operation(format!(
                "Core Git 缺少 Repository 所需的 refs、remotes 或 worktree 能力：{error}"
            ))
        })?;
    }
    Ok(())
}
