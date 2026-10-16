use super::{credential_error, CustomCredentialAdapter};
use std::sync::Mutex;

/// 桌面测试 使用的进程内凭据 adapter，避免测试构建读取真实用户钥匙串。
#[derive(Default)]
pub(super) struct EphemeralCustomCredentialAdapter(Mutex<Option<String>>);

impl CustomCredentialAdapter for EphemeralCustomCredentialAdapter {
    /// 读取当前 测试进程中保存的 API key。
    fn load(&self) -> Result<Option<String>, String> {
        self.0
            .lock()
            .map(|value| value.clone())
            .map_err(|_| credential_error())
    }

    /// 仅在当前 测试进程中替换 API key。
    fn save(&self, value: &str) -> Result<(), String> {
        *self.0.lock().map_err(|_| credential_error())? = Some(value.to_owned());
        Ok(())
    }

    /// 清除当前 测试进程中的 API key。
    fn delete(&self) -> Result<(), String> {
        *self.0.lock().map_err(|_| credential_error())? = None;
        Ok(())
    }
}
