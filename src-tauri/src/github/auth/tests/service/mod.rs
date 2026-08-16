use super::test_support::*;
use super::*;
use std::sync::atomic::AtomicUsize;
use std::sync::Mutex as StdMutex;

#[tokio::test]
async fn status_exposes_cached_github_identity_and_sign_out_clears_it() {
    let store = Arc::new(MemoryStore::default());
    store
        .save(&credentials_json(
            "access",
            "refresh",
            current_time_seconds() + 3_600,
            current_time_seconds() + 7_200,
        ))
        .unwrap();
    let identity = Arc::new(FakeIdentityProvider {
        cached: StdMutex::new(Some(GitHubIdentity {
            id: 42,
            login: "octocat".to_owned(),
            name: Some("Monalisa".to_owned()),
            avatar_path: Some("/cache/avatar.png".to_owned()),
        })),
    });
    let auth = auth_with_identity(
        store,
        Arc::new(FakeOAuthClient {
            refreshes: AtomicUsize::new(0),
            refresh_result: Err("not used".to_owned()),
            device_result: Err("not used".to_owned()),
        }),
        identity.clone(),
    );

    let status = auth.status().await.unwrap();
    assert!(status.signed_in);
    assert_eq!(status.identity.unwrap().login, "octocat");

    auth.sign_out().await.unwrap();
    assert_eq!(identity.cached().await.unwrap(), None);
}

#[tokio::test]
async fn sign_out_removes_credentials_without_touching_review_history_seams() {
    let store = Arc::new(MemoryStore::default());
    store.save("credentials").unwrap();
    let auth = auth_with(store.clone(), unused_oauth());

    auth.sign_out().await.unwrap();
    assert_eq!(store.load().unwrap(), None);
}
