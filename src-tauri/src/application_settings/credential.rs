//! Custom Built-in Agent Provider 使用的设备级凭据适配器。

#[cfg(not(feature = "wdio"))]
use keyring::Entry;
#[cfg(feature = "wdio")]
use std::sync::Mutex;

/// Custom 模型服务凭据在操作系统凭据管理器中的固定服务名。
#[cfg(not(feature = "wdio"))]
const CREDENTIAL_SERVICE: &str = "com.cyunlab.narrant.custom-provider";

/// Custom 模型服务使用的单一凭据槽位。
#[cfg(not(feature = "wdio"))]
const CREDENTIAL_ACCOUNT: &str = "model-service";

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

/// WDIO/E2E 使用的进程内凭据实现，避免测试读取真实用户钥匙串。
#[cfg(feature = "wdio")]
#[derive(Default)]
pub(crate) struct EphemeralCustomCredentialAdapter(Mutex<Option<String>>);

#[cfg(feature = "wdio")]
impl CustomCredentialAdapter for EphemeralCustomCredentialAdapter {
    /// 读取当前 E2E 进程中保存的 API key。
    fn load(&self) -> Result<Option<String>, String> {
        self.0
            .lock()
            .map(|value| value.clone())
            .map_err(|_| credential_error())
    }

    /// 仅在当前 E2E 进程中替换 API key。
    fn save(&self, value: &str) -> Result<(), String> {
        *self.0.lock().map_err(|_| credential_error())? = Some(value.to_owned());
        Ok(())
    }

    /// 清除当前 E2E 进程中的 API key。
    fn delete(&self) -> Result<(), String> {
        *self.0.lock().map_err(|_| credential_error())? = None;
        Ok(())
    }
}

/// 构造当前构建模式应使用的默认凭据适配器。
#[cfg(not(feature = "wdio"))]
pub(crate) fn default_custom_credential_adapter() -> std::sync::Arc<dyn CustomCredentialAdapter> {
    std::sync::Arc::new(SystemCustomCredentialAdapter)
}

/// 构造隔离于用户钥匙串的 E2E 凭据适配器。
#[cfg(feature = "wdio")]
pub(crate) fn default_custom_credential_adapter() -> std::sync::Arc<dyn CustomCredentialAdapter> {
    std::sync::Arc::new(EphemeralCustomCredentialAdapter::default())
}

/// 生成不包含原始 API key 的统一凭据错误。
fn credential_error() -> String {
    "无法访问当前系统用户的安全凭据存储。".to_owned()
}
