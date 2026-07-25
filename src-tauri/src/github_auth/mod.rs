mod credential_store;
mod oauth_client;

use async_trait::async_trait;
use oauth_client::{OAuthDeviceCodes, OAuthPollResult, OAuthTokens};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration, Instant};

pub use credential_store::{CredentialStore, SystemCredentialStore};
pub use oauth_client::{OAuthClient, OctocrabOAuthClient};

/// 在登出时清理仅属于 GitHub 的最近条目。
pub trait GitHubRecentEntries: Send + Sync {
    /// 仅删除 GitHub 最近条目。
    fn clear_github_entries(&self) -> Result<(), String>;
}

/// 向 GitHub Source 提供当前可选的 GitHub 用户访问令牌。
#[async_trait]
pub trait GitHubAccessTokenProvider: Send + Sync {
    /// 返回匿名状态，或返回刷新后的密钥访问令牌。
    async fn access_token(&self) -> Result<Option<SecretString>, String>;
}

/// 在本地状态待办完成前提供无操作的最近条目行为。
pub struct NoGitHubRecentEntries;

impl GitHubRecentEntries for NoGitHubRecentEntries {
    /// 尚无持久化最近条目存储时直接成功返回。
    fn clear_github_entries(&self) -> Result<(), String> {
        Ok(())
    }
}

/// 供 WebView 展示的公开认证状态。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationStatus {
    pub configured: bool,
    pub signed_in: bool,
    pub app_install_url: Option<String>,
}

/// 供 WebView 展示的非密钥 Device Flow 信息。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceAuthorization {
    pub authorization_id: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// 协调单账号授权、凭据轮换和登出策略。
pub struct GitHubAuth {
    client_id: SecretString,
    app_slug: String,
    oauth_client: Arc<dyn OAuthClient>,
    store: Arc<dyn CredentialStore>,
    recent_entries: Arc<dyn GitHubRecentEntries>,
    credentials: Mutex<Option<StoredCredentials>>,
    authorization_epoch: AtomicU64,
    next_authorization_id: AtomicU64,
    pending_authorizations: Mutex<HashMap<String, PendingAuthorization>>,
}

/// 将 Device Flow 尝试关联到创建它的登录代际。
struct PendingAuthorization {
    codes: OAuthDeviceCodes,
    epoch: u64,
}

/// 保存密钥令牌及 GitHub 签发的过期时间。
#[derive(Clone)]
struct StoredCredentials {
    access_token: SecretString,
    refresh_token: SecretString,
    expires_at: u64,
    refresh_token_expires_at: u64,
}

/// 仅在凭据存储边界序列化密钥
/// 相比 StoredCredentials，StoredCredentialsWire 不包含 SecretString
/// 可以序列化为 JSON 并存储在系统凭据管理器中。
#[derive(Deserialize, Serialize)]
struct StoredCredentialsWire {
    access_token: String,
    refresh_token: String,
    expires_at: u64,
    refresh_token_expires_at: u64,
}

impl StoredCredentials {
    /// 根据 GitHub 的轮换响应构建下一组持久化凭据。
    fn from_tokens(tokens: OAuthTokens, now: u64) -> Self {
        Self {
            access_token: tokens.access_token,
            refresh_token: tokens.refresh_token,
            expires_at: now + tokens.expires_in,
            refresh_token_expires_at: now + tokens.refresh_token_expires_in,
        }
    }

    /// 将加密存储的线缆数据转换为运行时密钥状态。
    fn from_wire(value: StoredCredentialsWire) -> Self {
        Self {
            access_token: SecretString::from(value.access_token),
            refresh_token: SecretString::from(value.refresh_token),
            expires_at: value.expires_at,
            refresh_token_expires_at: value.refresh_token_expires_at,
        }
    }

    /// 将运行时密钥转换为加密存储所需的线缆表示。
    fn to_wire(&self) -> StoredCredentialsWire {
        StoredCredentialsWire {
            access_token: self.access_token.expose_secret().to_owned(),
            refresh_token: self.refresh_token.expose_secret().to_owned(),
            expires_at: self.expires_at,
            refresh_token_expires_at: self.refresh_token_expires_at,
        }
    }
}

impl GitHubAuth {
    /// 使用显式 adapter 构建 GitHub Authentication。
    pub fn new(
        client_id: impl Into<String>,
        app_slug: impl Into<String>,
        oauth_client: Arc<dyn OAuthClient>,
        store: Arc<dyn CredentialStore>,
        recent_entries: Arc<dyn GitHubRecentEntries>,
    ) -> Self {
        Self {
            client_id: SecretString::from(client_id.into()),
            app_slug: app_slug.into(),
            oauth_client,
            store,
            recent_entries,
            credentials: Mutex::new(None),
            authorization_epoch: AtomicU64::new(0),
            next_authorization_id: AtomicU64::new(0),
            pending_authorizations: Mutex::new(HashMap::new()),
        }
    }

    /// 报告 GitHub 授权是否已配置且处于有效状态。
    pub async fn status(&self) -> Result<AuthorizationStatus, String> {
        let signed_in = self.load_credentials().await?.is_some();
        let configured = !self.client_id.expose_secret().is_empty() && !self.app_slug.is_empty();
        Ok(AuthorizationStatus {
            configured,
            signed_in,
            app_install_url: configured.then(|| {
                format!(
                    "https://github.com/apps/{}/installations/new",
                    self.app_slug
                )
            }),
        })
    }

    /// 发起 Device Flow，并仅在 Rust 中保留密钥 device code。
    pub async fn start_device_authorization(&self) -> Result<DeviceAuthorization, String> {
        self.require_configuration()?;
        let codes = self.oauth_client.start_device_flow(&self.client_id).await?;
        let authorization_id = format!(
            "authorization-{}",
            self.next_authorization_id.fetch_add(1, Ordering::Relaxed)
        );
        self.pending_authorizations.lock().await.insert(
            authorization_id.clone(),
            PendingAuthorization {
                codes: codes.clone(),
                epoch: self.authorization_epoch.load(Ordering::Acquire),
            },
        );
        Ok(DeviceAuthorization {
            authorization_id,
            user_code: codes.user_code,
            verification_uri: codes.verification_uri,
            expires_in: codes.expires_in,
            interval: codes.interval,
        })
    }

    /// 等待 Device Flow 授权并持久化其轮换后的凭据对。
    pub async fn complete_device_authorization(
        &self,
        authorization_id: &str,
    ) -> Result<(), String> {
        let pending = self
            .pending_authorizations
            .lock()
            .await
            .remove(authorization_id)
            .ok_or_else(|| "设备授权已失效，请重新开始授权。".to_owned())?;
        let deadline = Instant::now() + Duration::from_secs(pending.codes.expires_in);
        let mut interval = pending.codes.interval.max(1);
        loop {
            match self
                .oauth_client
                .poll_device_flow(&self.client_id, &pending.codes)
                .await?
            {
                OAuthPollResult::Authorized(tokens) => {
                    return self.persist_tokens(tokens, pending.epoch).await;
                }
                OAuthPollResult::Pending => {}
                OAuthPollResult::SlowDown => interval += 5,
            }
            if Instant::now() >= deadline {
                return Err("设备码已过期，请重新开始授权。".to_owned());
            }
            sleep(Duration::from_secs(interval)).await;
        }
    }

    /// 清除凭据及 GitHub 专属持久化状态。
    pub async fn sign_out(&self) -> Result<(), String> {
        self.authorization_epoch.fetch_add(1, Ordering::AcqRel);
        self.pending_authorizations.lock().await.clear();
        let mut credentials = self.credentials.lock().await;
        self.store.delete()?;
        *credentials = None;
        self.recent_entries.clear_github_entries()
    }

    /// 在授权前确保构建时 GitHub App 配置存在。
    fn require_configuration(&self) -> Result<(), String> {
        if self.client_id.expose_secret().is_empty() || self.app_slug.is_empty() {
            Err("此构建未配置 GitHub App，仍可匿名打开公开 Pull Request。".to_owned())
        } else {
            Ok(())
        }
    }

    /// 在持有单一刷新闸门时仅加载一次凭据状态。
    async fn load_credentials(&self) -> Result<Option<StoredCredentials>, String> {
        let mut credentials = self.credentials.lock().await;
        if credentials.is_none() {
            *credentials = self.read_store()?;
        }
        Ok(credentials.clone())
    }

    /// 读取并校验加密凭据，删除格式错误的状态。
    fn read_store(&self) -> Result<Option<StoredCredentials>, String> {
        let Some(value) = self.store.load()? else {
            return Ok(None);
        };
        match serde_json::from_str::<StoredCredentialsWire>(&value) {
            Ok(value) => Ok(Some(StoredCredentials::from_wire(value))),
            Err(_) => {
                self.store.delete()?;
                Err(reauthorize_error())
            }
        }
    }

    /// 仅通过操作系统凭据存储持久化密钥令牌。
    fn save_credentials(&self, credentials: &StoredCredentials) -> Result<(), String> {
        let value =
            serde_json::to_string(&credentials.to_wire()).map_err(|_| auth_state_error())?;
        self.store.save(&value)
    }

    /// 保存新授权的令牌对并更新当前刷新闸门。
    async fn persist_tokens(&self, tokens: OAuthTokens, expected_epoch: u64) -> Result<(), String> {
        let mut stored_credentials = self.credentials.lock().await;
        if self.authorization_epoch.load(Ordering::Acquire) != expected_epoch {
            return Err("GitHub 授权已取消，请重新开始授权。".to_owned());
        }
        let credentials = StoredCredentials::from_tokens(tokens, now_seconds());
        self.save_credentials(&credentials)?;
        *stored_credentials = Some(credentials);
        Ok(())
    }
}

#[async_trait]
impl GitHubAccessTokenProvider for GitHubAuth {
    /// 返回匿名状态，或刷新一组共享的密钥令牌对。
    async fn access_token(&self) -> Result<Option<SecretString>, String> {
        let mut credentials = self.credentials.lock().await;
        if credentials.is_none() {
            *credentials = self.read_store()?;
        }
        let Some(current) = credentials.as_ref() else {
            return Ok(None);
        };
        if current.expires_at > now_seconds() + 60 {
            return Ok(Some(current.access_token.clone()));
        }
        if current.refresh_token_expires_at <= now_seconds() {
            self.store.delete()?;
            *credentials = None;
            return Err(reauthorize_error());
        }
        let refresh_token = current.refresh_token.clone();
        match self
            .oauth_client
            .refresh(&self.client_id, &refresh_token)
            .await
        {
            Ok(tokens) => {
                let next = StoredCredentials::from_tokens(tokens, now_seconds());
                self.save_credentials(&next)?;
                let access_token = next.access_token.clone();
                *credentials = Some(next);
                Ok(Some(access_token))
            }
            Err(_) => {
                self.store.delete()?;
                *credentials = None;
                Err(reauthorize_error())
            }
        }
    }
}

/// 返回当前 Unix 秒级时间戳。
fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// 返回稳定的认证状态失败提示。
fn auth_state_error() -> String {
    "GitHub 登录状态不可用。".to_owned()
}

/// 返回稳定的重新授权恢复提示。
fn reauthorize_error() -> String {
    "GitHub 登录已失效，请重新授权。".to_owned()
}

#[cfg(test)]
mod tests;
