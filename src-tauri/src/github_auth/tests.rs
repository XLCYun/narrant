use super::*;
use async_trait::async_trait;
use secrecy::ExposeSecret;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex as StdMutex;

/// 为边界测试完全在内存中保存凭据存储值。
#[derive(Default)]
struct MemoryStore(StdMutex<Option<String>>);

impl CredentialStore for MemoryStore {
    fn load(&self) -> Result<Option<String>, String> {
        Ok(self
            .0
            .lock()
            .map_err(|_| "store lock poisoned".to_owned())?
            .clone())
    }

    fn save(&self, value: &str) -> Result<(), String> {
        *self
            .0
            .lock()
            .map_err(|_| "store lock poisoned".to_owned())? = Some(value.to_owned());
        Ok(())
    }

    fn delete(&self) -> Result<(), String> {
        *self
            .0
            .lock()
            .map_err(|_| "store lock poisoned".to_owned())? = None;
        Ok(())
    }
}

/// 不经网络请求提供确定性的 OAuth refresh 结果。
struct FakeOAuthClient {
    refreshes: AtomicUsize,
    refresh_result: Result<OAuthTokens, String>,
    device_result: Result<OAuthTokens, String>,
}

#[async_trait]
impl OAuthClient for FakeOAuthClient {
    async fn start_device_flow(&self, _: &SecretString) -> Result<OAuthDeviceCodes, String> {
        Ok(OAuthDeviceCodes::for_test())
    }

    async fn poll_device_flow(
        &self,
        _: &SecretString,
        _: &OAuthDeviceCodes,
    ) -> Result<OAuthPollResult, String> {
        self.device_result.clone().map(OAuthPollResult::Authorized)
    }

    async fn refresh(&self, _: &SecretString, _: &SecretString) -> Result<OAuthTokens, String> {
        self.refreshes.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(10)).await;
        self.refresh_result.clone()
    }
}

/// 为 refresh 行为测试构建即将过期的存储凭据。
fn expired_credentials() -> StoredCredentialsWire {
    StoredCredentialsWire {
        access_token: "old-access".to_owned(),
        refresh_token: "old-refresh".to_owned(),
        expires_at: 0,
        refresh_token_expires_at: now_seconds() + 3_600,
    }
}

/// 使用可控 OAuth adapter 构建已配置的认证服务。
fn auth_with(store: Arc<MemoryStore>, oauth: Arc<FakeOAuthClient>) -> GitHubAuth {
    GitHubAuth::new("client-id", "narrant", oauth, store)
}

#[tokio::test]
async fn concurrent_expired_token_reads_share_one_refresh_and_rotate_credentials() {
    let store = Arc::new(MemoryStore::default());
    store
        .save(&serde_json::to_string(&expired_credentials()).unwrap())
        .unwrap();
    let oauth = Arc::new(FakeOAuthClient {
        refreshes: AtomicUsize::new(0),
        refresh_result: Ok(OAuthTokens {
            access_token: SecretString::from("new-access"),
            refresh_token: SecretString::from("new-refresh"),
            expires_in: 28_800,
            refresh_token_expires_in: 15_897_600,
        }),
        device_result: Err("not used".to_owned()),
    });
    let auth = Arc::new(auth_with(store.clone(), oauth.clone()));

    let (first, second, third) = tokio::join!(
        auth.access_token(),
        auth.access_token(),
        auth.access_token()
    );
    for token in [first, second, third] {
        assert_eq!(token.unwrap().unwrap().expose_secret(), "new-access");
    }
    assert_eq!(oauth.refreshes.load(Ordering::SeqCst), 1);
    assert!(store.load().unwrap().unwrap().contains("new-refresh"));
}

#[tokio::test]
async fn failed_refresh_deletes_credentials_and_requires_reauthorization() {
    let store = Arc::new(MemoryStore::default());
    store
        .save(&serde_json::to_string(&expired_credentials()).unwrap())
        .unwrap();
    let auth = auth_with(
        store.clone(),
        Arc::new(FakeOAuthClient {
            refreshes: AtomicUsize::new(0),
            refresh_result: Err("invalid refresh token".to_owned()),
            device_result: Err("not used".to_owned()),
        }),
    );

    assert_eq!(auth.access_token().await.unwrap_err(), reauthorize_error());
    assert_eq!(store.load().unwrap(), None);
}

#[tokio::test]
async fn malformed_credentials_are_deleted_and_not_reported_as_signed_in() {
    let store = Arc::new(MemoryStore::default());
    store.save("not-json").unwrap();
    let auth = auth_with(
        store.clone(),
        Arc::new(FakeOAuthClient {
            refreshes: AtomicUsize::new(0),
            refresh_result: Err("not used".to_owned()),
            device_result: Err("not used".to_owned()),
        }),
    );

    assert_eq!(auth.status().await.unwrap_err(), reauthorize_error());
    assert_eq!(store.load().unwrap(), None);
}

#[tokio::test]
async fn sign_out_removes_credentials_without_touching_review_history_seams() {
    let store = Arc::new(MemoryStore::default());
    store.save("credentials").unwrap();
    let auth = auth_with(
        store.clone(),
        Arc::new(FakeOAuthClient {
            refreshes: AtomicUsize::new(0),
            refresh_result: Err("not used".to_owned()),
            device_result: Err("not used".to_owned()),
        }),
    );

    auth.sign_out().await.unwrap();
    assert_eq!(store.load().unwrap(), None);
}

#[tokio::test]
async fn sign_out_invalidates_pending_device_authorizations_before_they_can_persist() {
    let store = Arc::new(MemoryStore::default());
    let auth = auth_with(
        store.clone(),
        Arc::new(FakeOAuthClient {
            refreshes: AtomicUsize::new(0),
            refresh_result: Err("not used".to_owned()),
            device_result: Ok(OAuthTokens {
                access_token: SecretString::from("device-access"),
                refresh_token: SecretString::from("device-refresh"),
                expires_in: 28_800,
                refresh_token_expires_in: 15_897_600,
            }),
        }),
    );
    let authorization = auth.start_device_authorization().await.unwrap();

    auth.sign_out().await.unwrap();

    assert_eq!(
        auth.complete_device_authorization(&authorization.authorization_id)
            .await
            .unwrap_err(),
        "设备授权已失效，请重新开始授权。"
    );
    assert_eq!(store.load().unwrap(), None);
}

#[tokio::test]
async fn device_flow_exposes_no_device_secret_and_persists_authorized_tokens() {
    let store = Arc::new(MemoryStore::default());
    let auth = auth_with(
        store.clone(),
        Arc::new(FakeOAuthClient {
            refreshes: AtomicUsize::new(0),
            refresh_result: Err("not used".to_owned()),
            device_result: Ok(OAuthTokens {
                access_token: SecretString::from("device-access"),
                refresh_token: SecretString::from("device-refresh"),
                expires_in: 28_800,
                refresh_token_expires_in: 15_897_600,
            }),
        }),
    );

    let authorization = auth.start_device_authorization().await.unwrap();
    assert_eq!(authorization.user_code, "TEST-CODE");
    assert!(!serde_json::to_string(&authorization)
        .unwrap()
        .contains("device_code"));
    auth.complete_device_authorization(&authorization.authorization_id)
        .await
        .unwrap();
    assert_eq!(
        auth.access_token().await.unwrap().unwrap().expose_secret(),
        "device-access"
    );
    assert!(store.load().unwrap().unwrap().contains("device-refresh"));
}

#[test]
fn maps_terminal_device_flow_errors_to_recovery_guidance() {
    for (code, expected) in [
        ("access_denied", "你已取消 GitHub 授权。"),
        ("expired_token", "设备码已过期，请重新开始授权。"),
        (
            "device_flow_disabled",
            "此 GitHub App 尚未启用 Device Flow。",
        ),
        ("malformed", "GitHub 授权失败，请重新尝试。"),
    ] {
        assert_eq!(oauth_client::map_device_flow_error(code), expected);
    }
}

#[tokio::test]
async fn constructs_the_octocrab_client_inside_a_tokio_runtime() {
    oauth_client::OctocrabOAuthClient::new("https://github.com")
        .await
        .expect("在 Tokio runtime 中构建 OAuth 客户端");
}
