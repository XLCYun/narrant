mod credential_store;
mod identity;
mod oauth_client;
mod service;

use async_trait::async_trait;
use secrecy::SecretString;
use serde::Serialize;

pub use credential_store::CredentialStore;
#[cfg(feature = "test-isolation")]
pub use credential_store::EphemeralCredentialStore;
#[cfg(not(feature = "test-isolation"))]
pub use credential_store::SystemCredentialStore;
pub use identity::FileGitHubIdentityProvider;
pub use oauth_client::{OAuthClient, OctocrabOAuthClient};
pub use service::GitHubAuth;

/// WebView 可安全展示的 GitHub 公开身份。
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitHubIdentity {
    pub id: u64,
    pub login: String,
    pub name: Option<String>,
    pub avatar_path: Option<String>,
}

/// 隔离 GitHub 公开身份的网络读取和本地缓存。
#[async_trait]
pub trait GitHubIdentityProvider: Send + Sync {
    /// 读取上次成功持久化的公开 GitHub Identity。
    async fn cached(&self) -> Result<Option<GitHubIdentity>, String>;
    /// 使用当前 user access token 刷新公开资料和头像缓存。
    async fn refresh(&self, access_token: &SecretString) -> Result<GitHubIdentity, String>;
    /// 在退出 GitHub 时删除全部公开身份缓存。
    fn clear(&self) -> Result<(), String>;
}

/// 供 WebView 展示的公开认证状态。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationStatus {
    pub configured: bool,
    pub signed_in: bool,
    pub app_install_url: Option<String>,
    pub identity: Option<GitHubIdentity>,
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
