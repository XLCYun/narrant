#[cfg(not(feature = "wdio"))]
use keyring::Entry;
#[cfg(feature = "wdio")]
use std::sync::Mutex;

/// 平台凭据管理器中的应用命名空间；与 Tauri bundle identifier 保持一致。
#[cfg(not(feature = "wdio"))]
const CREDENTIAL_SERVICE: &str = "com.cyunlab.narrant.github-auth";

/// 单账户策略下 GitHub.com 的固定凭据槽位。
///
/// 系统凭据管理器本身会按操作系统用户隔离，因此这里不需要、也不应写入
/// GitHub 登录名等用户身份信息。未来支持 GitHub Enterprise 时可改为对应 host。
#[cfg(not(feature = "wdio"))]
const CREDENTIAL_ACCOUNT: &str = "github.com";

/// 为当前操作系统用户持久化加密凭据封套。
pub trait CredentialStore: Send + Sync {
    /// 读取已有的加密凭据封套。
    fn load(&self) -> Result<Option<String>, String>;

    /// 替换加密凭据封套。
    fn save(&self, value: &str) -> Result<(), String>;

    /// 删除加密凭据封套。
    fn delete(&self) -> Result<(), String>;
}

/// 将 GitHub 凭据存储在平台凭据管理器中。
#[cfg(not(feature = "wdio"))]
pub struct SystemCredentialStore;

#[cfg(not(feature = "wdio"))]
impl CredentialStore for SystemCredentialStore {
    /// 从钥匙串或凭据管理器读取凭据。
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

    /// 将凭据写入钥匙串或凭据管理器。
    fn save(&self, value: &str) -> Result<(), String> {
        Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT)
            .and_then(|entry| entry.set_password(value))
            .map_err(|_| credential_error())
    }

    /// 从钥匙串或凭据管理器删除凭据。
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

/// WDIO 进程内的隔离凭据存储，避免测试访问用户的系统钥匙串。
#[cfg(feature = "wdio")]
#[derive(Default)]
pub struct EphemeralCredentialStore(Mutex<Option<String>>);

#[cfg(feature = "wdio")]
impl CredentialStore for EphemeralCredentialStore {
    /// 返回当前 E2E 应用进程内保存的凭据封套。
    fn load(&self) -> Result<Option<String>, String> {
        self.0
            .lock()
            .map(|value| value.clone())
            .map_err(|_| credential_error())
    }

    /// 仅在当前 E2E 应用进程内替换凭据封套。
    fn save(&self, value: &str) -> Result<(), String> {
        *self.0.lock().map_err(|_| credential_error())? = Some(value.to_owned());
        Ok(())
    }

    /// 删除当前 E2E 应用进程内的凭据封套。
    fn delete(&self) -> Result<(), String> {
        *self.0.lock().map_err(|_| credential_error())? = None;
        Ok(())
    }
}

/// 生成面向用户的凭据存储失败提示。
fn credential_error() -> String {
    "无法访问当前系统用户的安全凭据存储。".to_owned()
}

#[cfg(all(test, feature = "wdio"))]
#[path = "tests/credential_store.rs"]
mod tests;
