use super::super::config::{
    trusted_manifest_url, trusted_package_url, TrustedUpdaterConfig, UpdaterConfigError,
    PRODUCTION_UPDATE_ENDPOINT,
};
use base64::Engine;

const PUBLIC_KEY: &str = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";

/// 将真实 minisign 公钥文本包装成 Tauri updater 配置格式。
fn encoded_public_key() -> String {
    base64::engine::general_purpose::STANDARD.encode(PUBLIC_KEY)
}

#[test]
fn manifest_redirect_allowlist_requires_https_default_port_and_known_path() {
    assert!(trusted_manifest_url(
        &PRODUCTION_UPDATE_ENDPOINT.parse().unwrap()
    ));
    assert!(trusted_manifest_url(
        &"https://updates.cyunlab.com/narrant/channels/stable/latest.json"
            .parse()
            .unwrap()
    ));
    for denied in [
        "http://updates.cyunlab.com/narrant/channels/stable/latest.json",
        "https://updates.cyunlab.com:444/narrant/channels/stable/latest.json",
        "https://user@updates.cyunlab.com/narrant/channels/stable/latest.json",
        "https://updates.cyunlab.com/dsh-desktop/channels/stable/latest.json",
        "https://updates.cyunlab.com/narrant/channels/stable/other.json",
        "https://updates.cyunlab.com/narrant/channels/stable/latest.json?download=1",
        "https://updates.cyunlab.com/narrant/channels/stable/latest.json#fragment",
    ] {
        assert!(!trusted_manifest_url(&denied.parse().unwrap()), "{denied}");
    }
}

#[test]
fn production_manifest_policy_accepts_latest_tagged_and_asset_chain() {
    let chain = [PRODUCTION_UPDATE_ENDPOINT];
    assert!(chain
        .iter()
        .all(|url| trusted_manifest_url(&url.parse().unwrap())));
}

#[test]
fn production_config_fails_closed_when_build_inputs_are_missing_or_changed() {
    let key = encoded_public_key();
    assert_eq!(
        TrustedUpdaterConfig::parse(None, Some(&key)),
        Err(UpdaterConfigError::MissingEndpoint)
    );
    assert_eq!(
        TrustedUpdaterConfig::parse(Some(PRODUCTION_UPDATE_ENDPOINT), None),
        Err(UpdaterConfigError::MissingPublicKey)
    );
    assert_eq!(
        TrustedUpdaterConfig::parse(Some("http://github.com/latest.json"), Some(&key)),
        Err(UpdaterConfigError::InsecureEndpoint)
    );
    assert_eq!(
        TrustedUpdaterConfig::parse(Some("https://example.com/latest.json"), Some(&key)),
        Err(UpdaterConfigError::UnexpectedEndpoint)
    );
    assert!(TrustedUpdaterConfig::parse(Some(PRODUCTION_UPDATE_ENDPOINT), Some(&key)).is_ok());
}

#[test]
fn package_allowlist_accepts_only_narrant_tag_specific_https_assets() {
    for denied in [
        "http://updates.cyunlab.com/narrant/releases/1.2.0/darwin-aarch64/hash-update.tar.gz",
        "https://example.com/narrant/releases/1.2.0/darwin-aarch64/hash-update.tar.gz",
        "https://updates.cyunlab.com/dsh-desktop/releases/1.2.0/darwin-aarch64/hash-update.tar.gz",
        "https://updates.cyunlab.com/narrant/channels/stable/update.tar.gz",
    ] {
        assert!(!trusted_package_url(&denied.parse().unwrap()), "{denied}");
    }
    assert!(trusted_package_url(
        &"https://updates.cyunlab.com/narrant/releases/1.2.0/darwin-aarch64/0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef-narrant-darwin-aarch64-updater.app.tar.gz"
            .parse()
            .unwrap()
    ));
}
