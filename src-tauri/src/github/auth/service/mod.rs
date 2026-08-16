mod device_authorization;
mod token_lifecycle;

use super::{
    AuthorizationStatus, CredentialStore, GitHubIdentity, GitHubIdentityProvider, OAuthClient,
};
use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tokio::sync::Mutex;

use device_authorization::PendingAuthorization;
use token_lifecycle::StoredCredentials;

/// 在没有配置身份缓存时维持原有认证能力。
#[allow(dead_code, reason = "保留无身份缓存 adapter 供显式认证装配使用")]
struct DisabledGitHubIdentityProvider;

#[async_trait]
impl GitHubIdentityProvider for DisabledGitHubIdentityProvider {
    /// 未配置 provider 时不存在已缓存身份。
    async fn cached(&self) -> Result<Option<GitHubIdentity>, String> {
        Ok(None)
    }

    /// 未配置 provider 时明确拒绝资料刷新。
    async fn refresh(&self, _: &SecretString) -> Result<GitHubIdentity, String> {
        Err("GitHub 身份资料未配置。".to_owned())
    }

    /// 未配置 provider 时无需清理本地资料。
    fn clear(&self) -> Result<(), String> {
        Ok(())
    }
}

/// 协调单账号授权、凭据轮换和登出策略。
pub struct GitHubAuth {
    client_id: SecretString,
    app_slug: String,
    oauth_client: Arc<dyn OAuthClient>,
    store: Arc<dyn CredentialStore>,
    identity_provider: Arc<dyn GitHubIdentityProvider>,
    credentials: Mutex<Option<StoredCredentials>>,
    authorization_epoch: AtomicU64,
    next_authorization_id: AtomicU64,
    pending_authorizations: Mutex<HashMap<String, PendingAuthorization>>,
}

impl GitHubAuth {
    /// 使用显式 adapter 构建 GitHub Authentication。
    #[allow(dead_code, reason = "保留无身份缓存的认证构造 interface")]
    pub fn new(
        client_id: impl Into<String>,
        app_slug: impl Into<String>,
        oauth_client: Arc<dyn OAuthClient>,
        store: Arc<dyn CredentialStore>,
    ) -> Self {
        Self::with_identity_provider(
            client_id,
            app_slug,
            oauth_client,
            store,
            Arc::new(DisabledGitHubIdentityProvider),
        )
    }

    /// 使用显式身份 provider 构建 GitHub Authentication。
    pub fn with_identity_provider(
        client_id: impl Into<String>,
        app_slug: impl Into<String>,
        oauth_client: Arc<dyn OAuthClient>,
        store: Arc<dyn CredentialStore>,
        identity_provider: Arc<dyn GitHubIdentityProvider>,
    ) -> Self {
        Self {
            client_id: SecretString::from(client_id.into()),
            app_slug: app_slug.into(),
            oauth_client,
            store,
            identity_provider,
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
        let identity = if signed_in {
            self.identity_provider.cached().await.unwrap_or(None)
        } else {
            None
        };
        Ok(AuthorizationStatus {
            configured,
            signed_in,
            app_install_url: configured.then(|| {
                format!(
                    "https://github.com/apps/{}/installations/new",
                    self.app_slug
                )
            }),
            identity,
        })
    }

    /// 清除凭据与待完成授权，不修改 Repository 或 Review 历史。
    pub async fn sign_out(&self) -> Result<(), String> {
        self.authorization_epoch.fetch_add(1, Ordering::AcqRel);
        self.pending_authorizations.lock().await.clear();
        let mut credentials = self.credentials.lock().await;
        self.store.delete()?;
        *credentials = None;
        self.identity_provider.clear()
    }

    /// 使用当前凭据刷新公开身份；失败不改变登录状态。
    pub async fn refresh_identity(&self) -> Result<GitHubIdentity, String> {
        let access_token = self
            .access_token()
            .await?
            .ok_or_else(|| "GitHub 尚未连接。".to_owned())?;
        self.identity_provider.refresh(&access_token).await
    }

    /// 在授权前确保构建时 GitHub App 配置存在。
    fn require_configuration(&self) -> Result<(), String> {
        if self.client_id.expose_secret().is_empty() || self.app_slug.is_empty() {
            Err("此构建未配置 GitHub App，仍可匿名打开公开 Pull Request。".to_owned())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod test_support;

#[cfg(test)]
mod tests;
