use super::*;
use httpmock::prelude::*;
use secrecy::SecretString;

#[tokio::test]
async fn refresh_fetches_authenticated_profile_and_caches_a_valid_avatar() {
    let server = MockServer::start();
    let cache = tempfile::tempdir().unwrap();
    let profile = server.mock(|when, then| {
        when.method(GET)
            .path("/user")
            .header("authorization", "Bearer access-token");
        then.status(200).json_body(serde_json::json!({
            "id": 42,
            "login": "octocat",
            "name": "Monalisa",
            "avatar_url": format!("{}/avatar", server.base_url())
        }));
    });
    let avatar = server.mock(|when, then| {
        when.method(GET).path("/avatar");
        then.status(200)
            .header("content-type", "image/png")
            .body(b"\x89PNG\r\n\x1a\navatar".as_slice());
    });
    let provider = FileGitHubIdentityProvider::new(server.base_url(), cache.path().to_owned());

    let identity = provider
        .refresh(&SecretString::from("access-token"))
        .await
        .unwrap();

    assert_eq!(identity.login, "octocat");
    assert_eq!(identity.name.as_deref(), Some("Monalisa"));
    assert_eq!(
        std::fs::read(identity.avatar_path.unwrap()).unwrap(),
        b"\x89PNG\r\n\x1a\navatar"
    );
    assert_eq!(provider.cached().await.unwrap().unwrap().id, 42);
    profile.assert();
    avatar.assert();
}

#[tokio::test]
async fn failed_avatar_refresh_preserves_the_last_valid_local_avatar() {
    let server = MockServer::start();
    let cache = tempfile::tempdir().unwrap();
    std::fs::write(cache.path().join("avatar.png"), b"\x89PNG\r\n\x1a\nold").unwrap();
    std::fs::write(
        cache.path().join(PROFILE_FILE),
        serde_json::to_vec(&CachedGitHubIdentity {
            id: 42,
            login: "octocat".to_owned(),
            name: Some("Old Name".to_owned()),
            avatar_file: Some("avatar.png".to_owned()),
        })
        .unwrap(),
    )
    .unwrap();
    server.mock(|when, then| {
        when.method(GET).path("/user");
        then.status(200).json_body(serde_json::json!({
            "id": 42,
            "login": "octocat",
            "name": "New Name",
            "avatar_url": format!("{}/avatar", server.base_url())
        }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/avatar");
        then.status(200)
            .header("content-type", "text/plain")
            .body("not an image");
    });
    let provider = FileGitHubIdentityProvider::new(server.base_url(), cache.path().to_owned());

    let refreshed = provider
        .refresh(&SecretString::from("access-token"))
        .await
        .unwrap();

    assert_eq!(refreshed.name.as_deref(), Some("New Name"));
    assert_eq!(
        refreshed.avatar_path.as_deref(),
        Some(cache.path().join("avatar.png").to_str().unwrap())
    );
}
