//! Custom Built-in Agent Provider 使用的设备级凭据适配器。

#[cfg(not(feature = "wdio"))]
use keyring::Entry;
#[cfg(all(debug_assertions, not(feature = "wdio")))]
use serde::{Deserialize, Serialize};
#[cfg(all(debug_assertions, not(feature = "wdio")))]
use std::io::Write;
#[cfg(all(debug_assertions, not(feature = "wdio")))]
use std::path::{Path, PathBuf};

#[cfg(feature = "wdio")]
mod ephemeral;
#[cfg(feature = "wdio")]
use ephemeral::EphemeralCustomCredentialAdapter;

/// Custom 模型服务凭据在操作系统凭据管理器中的固定服务名。
#[cfg(not(feature = "wdio"))]
const CREDENTIAL_SERVICE: &str = "com.cyunlab.narrant.custom-provider";

/// Custom 模型服务使用的单一凭据槽位。
#[cfg(not(feature = "wdio"))]
const CREDENTIAL_ACCOUNT: &str = "model-service";

/// Debug 构建使用的本地凭据文件名；与 settings.json 同目录且不进入仓库。
#[cfg(all(debug_assertions, not(feature = "wdio")))]
const DEVELOPMENT_CREDENTIAL_FILE: &str = "custom-provider-credential.dev.json";

/// 显式允许 Debug 构建从钥匙串执行一次迁移的环境变量。
#[cfg(all(debug_assertions, not(feature = "wdio")))]
const DEVELOPMENT_KEYCHAIN_IMPORT_ENV: &str = "NARRANT_DEV_IMPORT_CUSTOM_PROVIDER_KEYCHAIN";

/// Application Settings 依赖的凭据边界，便于测试注入内存实现。
pub(crate) trait CustomCredentialAdapter: Send + Sync {
    /// 读取当前设备用户保存的 API key。
    fn load(&self) -> Result<Option<String>, String>;

    /// 替换当前设备用户保存的 API key。
    fn save(&self, value: &str) -> Result<(), String>;

    /// 删除当前设备用户保存的 API key。
    fn delete(&self) -> Result<(), String>;
}

/// 使用当前操作系统安全凭据存储保存 Custom API key。
#[cfg(not(feature = "wdio"))]
pub(crate) struct SystemCustomCredentialAdapter;

#[cfg(not(feature = "wdio"))]
impl CustomCredentialAdapter for SystemCustomCredentialAdapter {
    /// 从钥匙串或平台凭据管理器读取 API key。
    fn load(&self) -> Result<Option<String>, String> {
        match Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT)
            .map_err(|_| credential_error())?
            .get_password()
        {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(credential_error()),
        }
    }

    /// 将 API key 写入钥匙串或平台凭据管理器。
    fn save(&self, value: &str) -> Result<(), String> {
        Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT)
            .and_then(|entry| entry.set_password(value))
            .map_err(|_| credential_error())
    }

    /// 删除钥匙串中的 API key；槽位不存在时视为成功。
    fn delete(&self) -> Result<(), String> {
        match Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT)
            .map_err(|_| credential_error())?
            .delete_credential()
        {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(credential_error()),
        }
    }
}

/// Debug 构建的本地凭据文档；`null` 明确表示用户已删除开发凭据。
#[cfg(all(debug_assertions, not(feature = "wdio")))]
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct DevelopmentCredentialDocument {
    /// 本地开发使用的 Custom API key；不会写入正式 settings.json。
    api_key: Option<String>,
}

/// Debug 构建的明文本地凭据适配器，避免日常开发反复触发系统钥匙串授权。
#[cfg(all(debug_assertions, not(feature = "wdio")))]
pub(super) struct DevelopmentCustomCredentialAdapter {
    path: PathBuf,
    keychain: std::sync::Arc<dyn CustomCredentialAdapter>,
    import_from_keychain: bool,
}

#[cfg(all(debug_assertions, not(feature = "wdio")))]
impl DevelopmentCustomCredentialAdapter {
    /// 从 settings 路径派生本地凭据路径，并保存显式的一次性迁移策略。
    pub(super) fn new(
        settings_path: &Path,
        keychain: std::sync::Arc<dyn CustomCredentialAdapter>,
        import_from_keychain: bool,
    ) -> Self {
        Self {
            path: settings_path.with_file_name(DEVELOPMENT_CREDENTIAL_FILE),
            keychain,
            import_from_keychain,
        }
    }

    /// 读取已经存在的本地文档；缺失文件交给可选迁移流程处理。
    fn load_local(&self) -> Result<Option<Option<String>>, String> {
        match std::fs::read_to_string(&self.path) {
            Ok(contents) => serde_json::from_str::<DevelopmentCredentialDocument>(&contents)
                .map(|document| Some(document.api_key.filter(|value| !value.is_empty())))
                .map_err(|_| credential_error()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(credential_error()),
        }
    }

    /// 原子写入本地凭据文档，并在 Unix 平台限制为当前用户读写。
    fn persist_local(&self, api_key: Option<&str>) -> Result<(), String> {
        let parent = self.path.parent().ok_or_else(credential_error)?;
        std::fs::create_dir_all(parent).map_err(|_| credential_error())?;
        let payload = serde_json::to_vec_pretty(&DevelopmentCredentialDocument {
            api_key: api_key.map(str::to_owned),
        })
        .map_err(|_| credential_error())?;
        let mut temporary =
            tempfile::NamedTempFile::new_in(parent).map_err(|_| credential_error())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            temporary
                .as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o600))
                .map_err(|_| credential_error())?;
        }
        temporary
            .write_all(&payload)
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|_| credential_error())?;
        temporary
            .persist(&self.path)
            .map_err(|_| credential_error())?;
        Ok(())
    }

    /// 首次且显式开启迁移时读取一次钥匙串，并把结果冻结到本地文档。
    fn import_keychain_once(&self) -> Result<Option<String>, String> {
        let value = self.keychain.load()?;
        self.persist_local(value.as_deref())?;
        Ok(value)
    }
}

#[cfg(all(debug_assertions, not(feature = "wdio")))]
impl CustomCredentialAdapter for DevelopmentCustomCredentialAdapter {
    /// 本地文档存在时绝不访问钥匙串；缺失时也仅在显式迁移开关下访问一次。
    fn load(&self) -> Result<Option<String>, String> {
        match self.load_local()? {
            Some(value) => Ok(value),
            None if self.import_from_keychain => self.import_keychain_once(),
            None => Ok(None),
        }
    }

    /// Debug 保存只更新本地凭据文件，不触发系统授权。
    fn save(&self, value: &str) -> Result<(), String> {
        self.persist_local(Some(value))
    }

    /// Debug 删除写入明确的空值，防止未来误触发一次性迁移。
    fn delete(&self) -> Result<(), String> {
        self.persist_local(None)
    }
}

/// Release 构建使用系统凭据管理器，不读取开发文件或迁移环境变量。
#[cfg(all(not(feature = "wdio"), not(debug_assertions)))]
pub(crate) fn default_custom_credential_adapter(
    _settings_path: &std::path::Path,
) -> std::sync::Arc<dyn CustomCredentialAdapter> {
    std::sync::Arc::new(SystemCustomCredentialAdapter)
}

/// Debug 构建使用 settings 同目录的本地文件，并仅接受显式的一次性钥匙串迁移。
#[cfg(all(not(feature = "wdio"), debug_assertions))]
pub(crate) fn default_custom_credential_adapter(
    settings_path: &std::path::Path,
) -> std::sync::Arc<dyn CustomCredentialAdapter> {
    let import_from_keychain = std::env::var(DEVELOPMENT_KEYCHAIN_IMPORT_ENV)
        .is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE"));
    std::sync::Arc::new(DevelopmentCustomCredentialAdapter::new(
        settings_path,
        std::sync::Arc::new(SystemCustomCredentialAdapter),
        import_from_keychain,
    ))
}

/// 构造隔离于用户钥匙串的 E2E 凭据适配器。
#[cfg(feature = "wdio")]
pub(crate) fn default_custom_credential_adapter(
    _settings_path: &std::path::Path,
) -> std::sync::Arc<dyn CustomCredentialAdapter> {
    std::sync::Arc::new(EphemeralCustomCredentialAdapter::default())
}

/// 生成不包含原始 API key 的统一凭据错误。
fn credential_error() -> String {
    "无法访问当前系统用户的安全凭据存储。".to_owned()
}
