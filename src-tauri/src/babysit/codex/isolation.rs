//! Codex App Server 的进程级配置与认证隔离。

use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use tempfile::{Builder, TempDir};

/// 只属于单个 Narrant App Server generation 的临时 CODEX_HOME。
pub(super) struct IsolatedCodexHome {
    directory: TempDir,
}

impl IsolatedCodexHome {
    /// 创建 0700 临时目录，只复制官方 `auth.json`，不复制任何用户配置或扩展。
    pub(super) fn create() -> std::io::Result<Self> {
        Self::create_from(source_codex_home().as_deref())
    }

    /// 使用显式源 home 创建隔离目录，供协议测试验证只复制认证材料。
    fn create_from(source_home: Option<&Path>) -> std::io::Result<Self> {
        let directory = Builder::new().prefix("narrant-codex-").tempdir()?;
        #[cfg(unix)]
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;

        if let Some(source) = source_home.map(|home| home.join("auth.json")) {
            if source.is_file() {
                copy_auth(&source, &directory.path().join("auth.json"))?;
            }
        }
        write_minimal_config(directory.path())?;
        Ok(Self { directory })
    }

    /// 返回仅用于子进程环境变量的路径；调用方不得记录或展示它。
    pub(super) fn path(&self) -> &Path {
        self.directory.path()
    }
}

/// 找到用户实际登录所使用的 Codex home，不读取其中其它文件。
fn source_codex_home() -> Option<PathBuf> {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|value| PathBuf::from(value).join(".codex")))
}

/// 以 0600 新文件复制认证材料，拒绝符号链接，避免把刷新写回用户文件。
fn copy_auth(source: &Path, destination: &Path) -> std::io::Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Err(std::io::Error::other(
            "Codex auth material is not a regular file",
        ));
    }
    let bytes = fs::read(source)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(destination)?;
    file.write_all(&bytes)?;
    file.flush()
}

/// 写入固定最小配置；个人 MCP、plugins、hooks、instructions 从未进入此 home。
fn write_minimal_config(home: &Path) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(home.join("config.toml"))?;
    file.write_all(
        b"cli_auth_credentials_store = \"file\"\nproject_doc_max_bytes = 0\nnotify = []\n\n[features]\napps = false\nhooks = false\nplugins = false\nskill_search = false\nskill_mcp_dependency_install = false\nmulti_agent = false\n",
    )?;
    file.flush()
}

#[cfg(test)]
#[path = "../tests/codex/isolation.rs"]
mod tests;
