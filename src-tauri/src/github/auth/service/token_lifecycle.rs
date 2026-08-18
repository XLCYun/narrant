use super::GitHubAuth;
use crate::github::auth::oauth_client::OAuthTokens;
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

/// 保存密钥令牌及 GitHub 签发的过期时间。
#[derive(Clone)]
pub(super) struct StoredCredentials {
    access_token: SecretString,
    refresh_token: SecretString,
    expires_at: u64,
    refresh_token_expires_at: u64,
}

/// 仅在凭据存储边界序列化密钥。
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
    /// 在持有单一刷新闸门时仅加载一次凭据状态。
    pub(super) async fn load_credentials(&self) -> Result<Option<StoredCredentials>, String> {
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
    pub(super) async fn persist_tokens(
        &self,
        tokens: OAuthTokens,
        expected_epoch: u64,
    ) -> Result<(), String> {
        let mut stored_credentials = self.credentials.lock().await;
        if self.authorization_epoch.load(Ordering::Acquire) != expected_epoch {
            return Err("GitHub 授权已取消，请重新开始授权。".to_owned());
        }
        let credentials = StoredCredentials::from_tokens(tokens, now_seconds());
        self.save_credentials(&credentials)?;
        *stored_credentials = Some(credentials);
        Ok(())
    }

    /// 返回匿名状态，或刷新一组共享的密钥令牌对。
    pub async fn access_token(&self) -> Result<Option<SecretString>, String> {
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
#[path = "../tests/service/token_lifecycle.rs"]
mod tests;
