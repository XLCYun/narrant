use crate::github::auth::oauth_client::OAuthTokens;
use crate::github::auth::service::test_support::*;
use crate::github::auth::CredentialStore;
use secrecy::{ExposeSecret, SecretString};
use std::sync::atomic::AtomicUsize;
use std::sync::Arc;

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
