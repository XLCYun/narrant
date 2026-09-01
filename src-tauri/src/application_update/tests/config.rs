use super::super::config::{
    trusted_package_url, TrustedUpdaterConfig, UpdaterConfigError, PRODUCTION_UPDATE_ENDPOINT,
};
use base64::Engine;

const PUBLIC_KEY: &str = "untrusted comment: minisign public key E7620F1842B4E81F\nRWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";

/// 将真实 minisign 公钥文本包装成 Tauri updater 配置格式。
fn encoded_public_key() -> String {
    base64::engine::general_purpose::STANDARD.encode(PUBLIC_KEY)
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
        "http://github.com/cyunlab/narrant/releases/download/v1.2.0/update.tar.gz",
        "https://example.com/cyunlab/narrant/releases/download/v1.2.0/update.tar.gz",
        "https://github.com/other/narrant/releases/download/v1.2.0/update.tar.gz",
        "https://github.com/cyunlab/narrant/releases/latest/download/update.tar.gz",
    ] {
        assert!(!trusted_package_url(&denied.parse().unwrap()), "{denied}");
    }
    assert!(trusted_package_url(
        &"https://github.com/cyunlab/narrant/releases/download/v1.2.0/narrant-darwin-aarch64-updater.app.tar.gz"
            .parse()
            .unwrap()
    ));
}
