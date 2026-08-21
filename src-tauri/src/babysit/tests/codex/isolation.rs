use super::*;
use std::sync::Mutex;

/// 返回固定 auth payload 并记录官方 account 的 fake keyring。
struct FakeCredentialReader {
    payload: Option<String>,
    account: Mutex<Option<String>>,
}

impl CodexCredentialReader for FakeCredentialReader {
    fn load(&self, account: &str) -> std::io::Result<Option<String>> {
        *self.account.lock().unwrap() = Some(account.to_owned());
        Ok(self.payload.clone())
    }
}

#[test]
fn official_keyring_account_matches_codex_path_hash_contract() {
    assert_eq!(
        official_keyring_account(std::path::Path::new("/narrant/codex-home")).unwrap(),
        "cli|38bdf2abab114c84"
    );
}

#[test]
fn isolated_home_copies_only_auth_and_is_deleted_on_drop() {
    let source = tempfile::tempdir().unwrap();
    std::fs::write(source.path().join("auth.json"), br#"{"token":"secret"}"#).unwrap();
    std::fs::write(
        source.path().join("config.toml"),
        "[mcp_servers.bad]\ncommand='bad'",
    )
    .unwrap();
    std::fs::create_dir(source.path().join("plugins")).unwrap();

    let reader = FakeCredentialReader {
        payload: None,
        account: Mutex::new(None),
    };
    let isolated = IsolatedCodexHome::create_with(Some(source.path()), &reader).unwrap();
    let path = isolated.path().to_owned();
    assert_eq!(
        std::fs::read(path.join("auth.json")).unwrap(),
        br#"{"token":"secret"}"#
    );
    let config = std::fs::read_to_string(path.join("config.toml")).unwrap();
    assert!(!config.contains("mcp_servers"));
    assert!(!path.join("plugins").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(path.join("auth.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }

    drop(isolated);
    assert!(!path.exists());
}

#[test]
fn keyring_only_auth_is_bridged_with_official_service_account_contract() {
    let source = tempfile::tempdir().unwrap();
    let reader = FakeCredentialReader {
        payload: Some(r#"{"auth_mode":"chatgpt","tokens":{"access_token":"secret"}}"#.to_owned()),
        account: Mutex::new(None),
    };

    let isolated = IsolatedCodexHome::create_with(Some(source.path()), &reader).unwrap();

    let account = reader.account.lock().unwrap().clone().unwrap();
    assert!(account.starts_with("cli|"));
    assert_eq!(account.len(), 20);
    assert_eq!(
        std::fs::read_to_string(isolated.path().join("auth.json")).unwrap(),
        r#"{"auth_mode":"chatgpt","tokens":{"access_token":"secret"}}"#
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(isolated.path().join("auth.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
