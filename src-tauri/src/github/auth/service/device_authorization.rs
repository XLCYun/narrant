use super::GitHubAuth;
use crate::github::auth::oauth_client::{OAuthDeviceCodes, OAuthPollResult};
use crate::github::auth::DeviceAuthorization;
use std::sync::atomic::Ordering;
use tokio::time::{sleep, Duration, Instant};

/// 将 Device Flow 尝试关联到创建它的登录代际。
pub(super) struct PendingAuthorization {
    codes: OAuthDeviceCodes,
    epoch: u64,
}

impl GitHubAuth {
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
                    let access_token = tokens.access_token.clone();
                    self.persist_tokens(tokens, pending.epoch).await?;
                    let _ = self.identity_provider.refresh(&access_token).await;
                    return Ok(());
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
}

#[cfg(test)]
#[path = "device_authorization_tests.rs"]
mod tests;
