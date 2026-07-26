use async_trait::async_trait;
use octocrab::{auth, Octocrab};
use secrecy::{ExposeSecret, SecretString};
use serde::Serialize;

/// 仅由 Rust 授权状态保存的 Device Flow 数据。
#[derive(Clone)]
pub struct OAuthDeviceCodes {
    inner: Option<auth::DeviceCodes>,
    pub device_code: SecretString,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    pub interval: u64,
}

/// 一次 Device Flow 轮询的结果。
pub enum OAuthPollResult {
    Authorized(OAuthTokens),
    Pending,
    SlowDown,
}

/// OAuth 返回的轮换后 GitHub 用户令牌对。
#[derive(Clone)]
pub struct OAuthTokens {
    pub access_token: SecretString,
    pub refresh_token: SecretString,
    pub expires_in: u64,
    pub refresh_token_expires_in: u64,
}

/// 执行 GitHub OAuth 协议请求，但不承载 Narrant 策略。
#[async_trait]
pub trait OAuthClient: Send + Sync {
    /// 发起 GitHub Device Flow 授权。
    async fn start_device_flow(&self, client_id: &SecretString)
        -> Result<OAuthDeviceCodes, String>;

    /// 向 GitHub 轮询一次 Device Flow 授权结果。
    async fn poll_device_flow(
        &self,
        client_id: &SecretString,
        codes: &OAuthDeviceCodes,
    ) -> Result<OAuthPollResult, String>;

    /// 用 refresh token 换取轮换后的用户令牌对。
    async fn refresh(
        &self,
        client_id: &SecretString,
        refresh_token: &SecretString,
    ) -> Result<OAuthTokens, String>;
}

/// 使用 Octocrab 发起 GitHub Device Flow 和 refresh HTTP 请求。
pub struct OctocrabOAuthClient {
    client: Octocrab,
}

impl OctocrabOAuthClient {
    /// 在 Tokio runtime 中为 GitHub OAuth 源站构建 Octocrab 客户端。
    pub async fn new(oauth_base_url: &str) -> Result<Self, String> {
        let client = Octocrab::builder()
            .base_uri(oauth_base_url)
            .map_err(|_| "GitHub 授权配置不可用。".to_owned())?
            .add_header(http::header::ACCEPT, "application/json".to_owned())
            .build()
            .map_err(|_| "GitHub 授权配置不可用。".to_owned())?;
        Ok(Self { client })
    }
}

#[async_trait]
impl OAuthClient for OctocrabOAuthClient {
    /// 通过 Octocrab 发起 Device Flow 请求。
    async fn start_device_flow(
        &self,
        client_id: &SecretString,
    ) -> Result<OAuthDeviceCodes, String> {
        let codes = self
            .client
            .authenticate_as_device(client_id, std::iter::empty::<&str>())
            .await
            .map_err(|_| "GitHub 暂时无法开始设备授权，请稍后重试。".to_owned())?;
        Ok(OAuthDeviceCodes {
            inner: Some(codes.clone()),
            device_code: SecretString::from(codes.device_code),
            user_code: codes.user_code,
            verification_uri: codes.verification_uri,
            expires_in: codes.expires_in,
            interval: codes.interval,
        })
    }

    /// 将 Octocrab 轮询响应转换为 Narrant 稳定的结果。
    async fn poll_device_flow(
        &self,
        client_id: &SecretString,
        codes: &OAuthDeviceCodes,
    ) -> Result<OAuthPollResult, String> {
        let device_codes = codes
            .inner
            .as_ref()
            .ok_or_else(|| "GitHub 授权状态不可用，请重新开始授权。".to_owned())?;
        match device_codes.poll_once(&self.client, client_id).await {
            Ok(auth::TokenResponse::Ok(tokens)) => {
                OAuthTokens::from_device_flow(tokens).map(OAuthPollResult::Authorized)
            }
            Ok(auth::TokenResponse::Continue {
                error: auth::Continue::AuthorizationPending,
            }) => Ok(OAuthPollResult::Pending),
            Ok(auth::TokenResponse::Continue {
                error: auth::Continue::SlowDown,
            }) => Ok(OAuthPollResult::SlowDown),
            Err(error) => Err(map_device_poll_error(error)),
        }
    }

    /// 通过 Octocrab 发送 GitHub refresh 请求。
    async fn refresh(
        &self,
        client_id: &SecretString,
        refresh_token: &SecretString,
    ) -> Result<OAuthTokens, String> {
        let response: RefreshResponse = self
            .client
            .post(
                "/login/oauth/access_token",
                Some(&RefreshRequest {
                    client_id: client_id.expose_secret(),
                    grant_type: "refresh_token",
                    refresh_token: refresh_token.expose_secret(),
                }),
            )
            .await
            .map_err(|_| "GitHub 登录已失效，请重新授权。".to_owned())?;
        Ok(response.into())
    }
}

#[cfg(test)]
#[path = "oauth_client_tests.rs"]
mod tests;

/// 序列化由 Octocrab 处理的 GitHub refresh-token 请求。
#[derive(Serialize)]
struct RefreshRequest<'a> {
    client_id: &'a str,
    grant_type: &'a str,
    refresh_token: &'a str,
}

/// 反序列化由 Octocrab 处理的 GitHub refresh-token 响应。
#[derive(serde::Deserialize)]
struct RefreshResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    refresh_token_expires_in: u64,
}

impl OAuthTokens {
    /// 将 Octocrab 成功的 Device Flow 响应转换为密钥令牌。
    fn from_device_flow(value: auth::OAuth) -> Result<Self, String> {
        let refresh_token = value
            .refresh_token
            .ok_or_else(|| "GitHub 授权响应不完整，请重新开始授权。".to_owned())?;
        Ok(Self {
            access_token: value.access_token,
            refresh_token,
            expires_in: value.expires_in.unwrap_or_default() as u64,
            refresh_token_expires_in: value.refresh_token_expires_in.unwrap_or_default() as u64,
        })
    }
}

impl From<RefreshResponse> for OAuthTokens {
    /// 将 refresh 响应转换为密钥令牌。
    fn from(value: RefreshResponse) -> Self {
        Self {
            access_token: SecretString::from(value.access_token),
            refresh_token: SecretString::from(value.refresh_token),
            expires_in: value.expires_in,
            refresh_token_expires_in: value.refresh_token_expires_in,
        }
    }
}

/// 将 GitHub Device Flow 终态协议错误转换为恢复提示。
fn map_device_poll_error(error: octocrab::Error) -> String {
    match error {
        octocrab::Error::GitHub { source, .. } => map_device_flow_error(&source.message),
        _ => "GitHub 授权失败，请重新尝试。".to_owned(),
    }
}

/// 映射 GitHub 返回的稳定 Device Flow 错误码。
pub(super) fn map_device_flow_error(error: &str) -> String {
    match error {
        "access_denied" => "你已取消 GitHub 授权。".to_owned(),
        "expired_token" => "设备码已过期，请重新开始授权。".to_owned(),
        "device_flow_disabled" => "此 GitHub App 尚未启用 Device Flow。".to_owned(),
        _ => "GitHub 授权失败，请重新尝试。".to_owned(),
    }
}
