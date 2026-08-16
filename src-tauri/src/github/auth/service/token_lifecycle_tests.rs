use super::*;
use crate::github::auth::oauth_client::OAuthTokens;
use crate::github::auth::service::test_support::*;
use crate::github::auth::CredentialStore;
use secrecy::{ExposeSecret, SecretString};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// 为 refresh 行为测试构建即将过期的存储凭据。
fn expired_credentials() -> String {
    credentials_json(
        "old-access",
        "old-refresh",
        0,
        current_time_seconds() + 3_600,
    )
}

#[tokio::test]
async fn concurrent_expired_token_reads_share_one_refresh_and_rotate_credentials() {
    let store = Arc::new(MemoryStore::default());
    store.save(&expired_credentials()).unwrap();
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
    store.save(&expired_credentials()).unwrap();
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
    let auth = auth_with(store.clone(), unused_oauth());

    assert_eq!(auth.status().await.unwrap_err(), reauthorize_error());
    assert_eq!(store.load().unwrap(), None);
}
