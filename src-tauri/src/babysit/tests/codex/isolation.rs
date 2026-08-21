use super::*;

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

    let isolated = IsolatedCodexHome::create_from(Some(source.path())).unwrap();
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
