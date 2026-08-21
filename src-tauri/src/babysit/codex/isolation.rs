//! Codex App Server 的进程级配置与认证隔离。

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use tempfile::{Builder, TempDir};
use zeroize::Zeroizing;

/// 官方 Codex CLI auth 使用的系统钥匙串 service。
const CODEX_KEYRING_SERVICE: &str = "Codex Auth";

/// 隔离层读取 Codex credential 的最小 seam，测试不会访问真实系统钥匙串。
trait CodexCredentialReader {
    /// 读取官方 service/account 下的序列化 auth payload。
    fn load(&self, account: &str) -> std::io::Result<Option<String>>;
}

/// 使用 keyring crate 访问当前平台的原生凭据管理器。
struct SystemCodexCredentialReader;

impl CodexCredentialReader for SystemCodexCredentialReader {
    fn load(&self, account: &str) -> std::io::Result<Option<String>> {
        let entry = keyring::Entry::new(CODEX_KEYRING_SERVICE, account)
            .map_err(|_| std::io::Error::other("Codex keyring unavailable"))?;
        match entry.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(std::io::Error::other("Codex keyring unavailable")),
        }
    }
}

/// 只属于单个 Narrant App Server generation 的临时 CODEX_HOME。
pub(super) struct IsolatedCodexHome {
    directory: TempDir,
}

impl IsolatedCodexHome {
    /// 创建 0700 临时目录，只复制官方 `auth.json`，不复制任何用户配置或扩展。
    pub(super) fn create() -> std::io::Result<Self> {
        Self::create_with(source_codex_home().as_deref(), &SystemCodexCredentialReader)
    }

    /// 使用显式源 home 创建隔离目录，供协议测试验证只复制认证材料。
    fn create_with(
        source_home: Option<&Path>,
        credential_reader: &dyn CodexCredentialReader,
    ) -> std::io::Result<Self> {
        let directory = Builder::new().prefix("narrant-codex-").tempdir()?;
        #[cfg(unix)]
        fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;

        let mut copied_file_auth = false;
        if let Some(source) = source_home.map(|home| home.join("auth.json")) {
            if source.is_file() {
                copy_auth(&source, &directory.path().join("auth.json"))?;
                copied_file_auth = true;
            }
        }
        if !copied_file_auth {
            if let Some(source_home) = source_home {
                let account = official_keyring_account(source_home)?;
                if let Some(payload) = credential_reader.load(&account)? {
                    let payload = Zeroizing::new(payload);
                    write_keyring_auth(&payload, &directory.path().join("auth.json"))?;
                }
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

/// 复现 Codex 官方 `cli|sha256(canonical CODEX_HOME)[..16]` account 派生。
fn official_keyring_account(codex_home: &Path) -> std::io::Result<String> {
    let canonical = codex_home
        .canonicalize()
        .unwrap_or_else(|_| codex_home.to_path_buf());
    let digest = Sha256::digest(canonical.to_string_lossy().as_bytes());
    let hex = format!("{digest:x}");
    Ok(format!("cli|{}", &hex[..16]))
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

/// 校验 keyring payload 是 auth.json object 后写入 0600 隔离文件。
fn write_keyring_auth(payload: &str, destination: &Path) -> std::io::Result<()> {
    if !payload.trim_start().starts_with('{') {
        return Err(std::io::Error::other("Codex keyring payload is invalid"));
    }
    let mut deserializer = serde_json::Deserializer::from_str(payload);
    serde::de::IgnoredAny::deserialize(&mut deserializer)?;
    deserializer.end()?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(destination)?;
    file.write_all(payload.as_bytes())?;
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
