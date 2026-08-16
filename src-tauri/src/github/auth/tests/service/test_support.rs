use super::*;
use crate::github::auth::oauth_client::{OAuthDeviceCodes, OAuthPollResult, OAuthTokens};
use async_trait::async_trait;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex as StdMutex;
use tokio::time::Duration;

/// 为边界测试完全在内存中保存凭据存储值。
#[derive(Default)]
pub(super) struct MemoryStore(pub(super) StdMutex<Option<String>>);

impl CredentialStore for MemoryStore {
    /// 返回测试进程内保存的凭据封套。
    fn load(&self) -> Result<Option<String>, String> {
        Ok(self
            .0
            .lock()
            .map_err(|_| "store lock poisoned".to_owned())?
            .clone())
    }

    /// 替换测试进程内保存的凭据封套。
    fn save(&self, value: &str) -> Result<(), String> {
        *self
            .0
            .lock()
            .map_err(|_| "store lock poisoned".to_owned())? = Some(value.to_owned());
        Ok(())
    }

    /// 删除测试进程内保存的凭据封套。
    fn delete(&self) -> Result<(), String> {
        *self
            .0
            .lock()
            .map_err(|_| "store lock poisoned".to_owned())? = None;
        Ok(())
    }
}

/// 不经网络请求提供确定性的 OAuth 结果。
pub(super) struct FakeOAuthClient {
    pub(super) refreshes: AtomicUsize,
    pub(super) refresh_result: Result<OAuthTokens, String>,
    pub(super) device_result: Result<OAuthTokens, String>,
}

/// 在认证公开边界下提供可观察的 GitHub Identity 缓存。
pub(super) struct FakeIdentityProvider {
    pub(super) cached: StdMutex<Option<GitHubIdentity>>,
}

#[async_trait]
impl GitHubIdentityProvider for FakeIdentityProvider {
    /// 返回测试预置的公开身份。
    async fn cached(&self) -> Result<Option<GitHubIdentity>, String> {
        Ok(self.cached.lock().unwrap().clone())
    }

    /// 将测试预置身份作为刷新结果。
    async fn refresh(&self, _: &SecretString) -> Result<GitHubIdentity, String> {
        self.cached
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| "profile unavailable".to_owned())
    }

    /// 清空测试身份，以便从公开状态观察退出行为。
    fn clear(&self) -> Result<(), String> {
        *self.cached.lock().unwrap() = None;
        Ok(())
    }
}

#[async_trait]
impl OAuthClient for FakeOAuthClient {
    /// 返回不包含真实网络状态的测试设备码。
    async fn start_device_flow(&self, _: &SecretString) -> Result<OAuthDeviceCodes, String> {
        Ok(OAuthDeviceCodes::for_test())
    }

    /// 将预置的设备授权结果映射为成功响应。
    async fn poll_device_flow(
        &self,
        _: &SecretString,
        _: &OAuthDeviceCodes,
    ) -> Result<OAuthPollResult, String> {
        self.device_result.clone().map(OAuthPollResult::Authorized)
    }

    /// 记录刷新次数并返回预置的令牌轮换结果。
    async fn refresh(&self, _: &SecretString, _: &SecretString) -> Result<OAuthTokens, String> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(10)).await;
        self.refresh_result.clone()
    }
}

/// 使用可控 OAuth adapter 构建已配置的认证服务。
pub(super) fn auth_with(store: Arc<MemoryStore>, oauth: Arc<FakeOAuthClient>) -> GitHubAuth {
    GitHubAuth::new("client-id", "narrant", oauth, store)
}

/// 使用可观察的身份缓存构建 GitHub Authentication。
pub(super) fn auth_with_identity(
    store: Arc<MemoryStore>,
    oauth: Arc<FakeOAuthClient>,
    identity: Arc<FakeIdentityProvider>,
) -> GitHubAuth {
    GitHubAuth::with_identity_provider("client-id", "narrant", oauth, store, identity)
}

/// 构建系统凭据存储使用的 JSON 测试封套。
pub(super) fn credentials_json(
    access_token: &str,
    refresh_token: &str,
    expires_at: u64,
    refresh_token_expires_at: u64,
) -> String {
    serde_json::json!({
        "access_token": access_token,
        "refresh_token": refresh_token,
        "expires_at": expires_at,
        "refresh_token_expires_at": refresh_token_expires_at,
    })
    .to_string()
}

/// 返回当前测试使用的 Unix 秒级时间戳。
pub(super) fn current_time_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 构建测试所需的默认 OAuth adapter。
pub(super) fn unused_oauth() -> Arc<FakeOAuthClient> {
    Arc::new(FakeOAuthClient {
        refreshes: AtomicUsize::new(0),
        refresh_result: Err("not used".to_owned()),
        device_result: Err("not used".to_owned()),
    })
}
