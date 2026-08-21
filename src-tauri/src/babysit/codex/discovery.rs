//! Codex 可执行文件发现。
//!
//! 发现 seam 只负责解析和校验可执行文件。持久化设置由 `runtime.rs` 单独拥有，
//! App Server 协议和进程隔离仍由 provider 负责，避免路径进入 Review 状态和诊断。

use crate::babysit::BabysitError;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::Command;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// `codex --version` 必须快速结束；挂起的可执行文件不可用。
const VERSION_TIMEOUT: Duration = Duration::from_secs(5);

/// 已通过校验的可执行文件及其仅供内部使用的版本标签。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ResolvedExecutable {
    pub(super) path: PathBuf,
    pub(super) version: String,
}

/// 纯候选搜索与有界直接进程校验。
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct CodexExecutableDiscovery;

impl CodexExecutableDiscovery {
    /// 先搜索持久化路径，再搜索当前进程 PATH 和已知安装位置。
    pub(super) async fn resolve(
        &self,
        persisted: Option<&Path>,
    ) -> Result<ResolvedExecutable, BabysitError> {
        self.resolve_candidates(persisted, self.candidates()).await
    }

    /// 按既定优先级校验候选；测试可注入候选而不改变当前进程 PATH。
    async fn resolve_candidates<I>(
        &self,
        persisted: Option<&Path>,
        candidates: I,
    ) -> Result<ResolvedExecutable, BabysitError>
    where
        I: IntoIterator<Item = PathBuf>,
    {
        if let Some(path) = persisted.and_then(canonical_executable) {
            if let Ok(executable) = self.validate(path, true).await {
                return Ok(executable);
            }
        }

        for candidate in candidates {
            if let Some(candidate) = canonical_executable(&candidate) {
                if let Ok(executable) = self.validate(candidate, false).await {
                    return Ok(executable);
                }
            }
        }
        Err(BabysitError::CodexNotFound)
    }

    /// 校验用户选择的绝对路径，但不在发现模块内持久化。
    pub(super) async fn validate_selected(
        &self,
        path: PathBuf,
    ) -> Result<ResolvedExecutable, BabysitError> {
        let path = canonical_executable(&path).ok_or(BabysitError::CodexExecutableInvalid)?;
        self.validate(path, true).await
    }

    /// 校验 `--version`；内存中只保留简短版本字符串。
    async fn validate(
        &self,
        path: PathBuf,
        selected: bool,
    ) -> Result<ResolvedExecutable, BabysitError> {
        let output = Command::new(&path)
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .output();
        let result = tokio::time::timeout(VERSION_TIMEOUT, output)
            .await
            .map_err(|_| BabysitError::CodexExecutableInvalid)?
            .map_err(|_| {
                if selected {
                    BabysitError::CodexExecutableInvalid
                } else {
                    BabysitError::CodexNotFound
                }
            })?;
        if !result.status.success() {
            return Err(if selected {
                BabysitError::CodexExecutableInvalid
            } else {
                BabysitError::CodexNotFound
            });
        }
        let version = result
            .stdout
            .iter()
            .chain(result.stderr.iter())
            .copied()
            .collect::<Vec<_>>();
        let version = String::from_utf8_lossy(&version)
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .ok_or(if selected {
                BabysitError::CodexExecutableInvalid
            } else {
                BabysitError::CodexNotFound
            })?;
        Ok(ResolvedExecutable { path, version })
    }

    /// Search PATH first, followed by platform-supported installation locations.
    fn candidates(&self) -> Vec<PathBuf> {
        let mut candidates = Vec::new();
        if let Some(path) = std::env::var_os("PATH") {
            candidates.extend(std::env::split_paths(&path).flat_map(|directory| {
                executable_names()
                    .into_iter()
                    .map(move |name| directory.join(name))
            }));
        }
        candidates.extend(known_install_locations());
        let mut seen = HashSet::new();
        candidates.retain(|candidate| seen.insert(candidate.clone()));
        candidates
    }
}

/// 在启动前 canonicalize 候选路径，并要求它是普通可执行文件。
fn canonical_executable(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let canonical = path.canonicalize().ok()?;
    let metadata = std::fs::metadata(&canonical).ok()?;
    if !metadata.is_file() {
        return None;
    }
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o111 == 0 {
        return None;
    }
    Some(canonical)
}

/// 可执行名称覆盖 Windows 原生 shim，同时不启动 shell。
fn executable_names() -> Vec<OsString> {
    if cfg!(windows) {
        vec![OsString::from("codex.exe"), OsString::from("codex.cmd")]
    } else {
        vec![OsString::from("codex")]
    }
}

/// 已知位置覆盖桌面启动时 PATH 不完整的场景。
fn known_install_locations() -> Vec<PathBuf> {
    let mut locations = Vec::new();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        locations.extend([
            home.join(".local/bin/codex"),
            home.join(".cargo/bin/codex"),
            home.join(".volta/bin/codex"),
            home.join(".asdf/shims/codex"),
            home.join(".npm-global/bin/codex"),
            home.join(".bun/bin/codex"),
        ]);
    }
    #[cfg(target_os = "macos")]
    locations.extend([
        PathBuf::from("/usr/local/bin/codex"),
        PathBuf::from("/opt/homebrew/bin/codex"),
        PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex"),
        PathBuf::from("/Applications/Codex.app/Contents/Resources/codex"),
    ]);
    #[cfg(target_os = "linux")]
    locations.extend([
        PathBuf::from("/usr/local/bin/codex"),
        PathBuf::from("/usr/bin/codex"),
    ]);
    #[cfg(windows)]
    if let Some(app_data) = std::env::var_os("APPDATA").map(PathBuf::from) {
        locations.push(app_data.join("npm/codex.cmd"));
    }
    locations
}

#[cfg(test)]
#[path = "../tests/codex/discovery.rs"]
mod tests;
