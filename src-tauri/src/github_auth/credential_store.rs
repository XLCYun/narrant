use keyring::Entry;

const CREDENTIAL_SERVICE: &str = "com.narrant.github";
const CREDENTIAL_ACCOUNT: &str = "current-user";

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
pub struct SystemCredentialStore;

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

/// 生成面向用户的凭据存储失败提示。
fn credential_error() -> String {
    "无法访问当前系统用户的安全凭据存储。".to_owned()
}
