use super::{ApplicationSettingsApplication, BabysitModelPreference, ThemePreference};
/// 创建能稳定响应 `codex --version` 的普通可执行文件。
fn version_fixture(directory: &std::path::Path, name: &str, succeeds: bool) -> std::path::PathBuf {
    #[cfg(windows)]
    {
        let path = directory.join(format!("{name}.cmd"));
        let exit = if succeeds { 0 } else { 2 };
        std::fs::write(&path, format!("@echo fixture output\r\n@exit /b {exit}\r\n")).unwrap();
        return path;
    }
    #[cfg(unix)]
    {
    use std::os::unix::fs::PermissionsExt;
    let path = directory.join(name);
    let exit = if succeeds { 0 } else { 2 };
    std::fs::write(
        &path,
        format!("#!/bin/sh\nprintf 'fixture output\\n'\nexit {exit}\n"),
    )
    .unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    permissions.set_mode(0o700);
    std::fs::set_permissions(&path, permissions).unwrap();
    path
    }
}

#[test]
fn missing_settings_resolve_to_device_defaults_without_creating_a_file() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let application = ApplicationSettingsApplication::new(
        settings_path.clone(),
        directory.path().join("babysit-settings.json"),
    );

    let settings = application.load().unwrap();

    assert_eq!(settings.version, 1);
    assert_eq!(settings.theme, ThemePreference::System);
    assert_eq!(settings.babysit_model, BabysitModelPreference::Automatic);
    assert_eq!(settings.codex_executable, None);
    assert!(!settings_path.exists());
}

#[test]
fn theme_update_is_persisted_for_a_new_application_instance() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let legacy_path = directory.path().join("babysit-settings.json");
    let application =
        ApplicationSettingsApplication::new(settings_path.clone(), legacy_path.clone());

    let updated = application.update_theme(ThemePreference::Dark).unwrap();
    let reopened = ApplicationSettingsApplication::new(settings_path, legacy_path)
        .load()
        .unwrap();

    assert_eq!(updated.theme, ThemePreference::Dark);
    assert_eq!(reopened, updated);
}

#[tokio::test]
async fn executable_update_canonicalizes_and_preserves_other_application_settings() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let legacy_path = directory.path().join("babysit-settings.json");
    let executable = version_fixture(directory.path(), "codex", true);
    let application = ApplicationSettingsApplication::new(settings_path, legacy_path);
    application.update_theme(ThemePreference::Dark).unwrap();

    let updated = application
        .update_codex_executable(Some(executable.clone()))
        .await
        .unwrap();

    assert_eq!(updated.theme, ThemePreference::Dark);
    assert_eq!(updated.codex_executable, Some(executable.canonicalize().unwrap()));
    assert_eq!(application.load().unwrap(), updated);
}

#[tokio::test]
async fn relative_executable_is_rejected_without_replacing_the_old_preference() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let application = ApplicationSettingsApplication::new(
        settings_path,
        directory.path().join("babysit-settings.json"),
    );
    let old = version_fixture(directory.path(), "codex-old", true);
    application
        .update_codex_executable(Some(old.clone()))
        .await
        .unwrap();

    assert!(application
        .update_codex_executable(Some("relative/codex".into()))
        .await
        .is_err());
    assert_eq!(
        application.load().unwrap().codex_executable,
        Some(old.canonicalize().unwrap())
    );
}

#[tokio::test]
async fn failed_version_probe_does_not_replace_the_old_preference() {
    let directory = tempfile::tempdir().unwrap();
    let application = ApplicationSettingsApplication::new(
        directory.path().join("settings.json"),
        directory.path().join("babysit-settings.json"),
    );
    let old = version_fixture(directory.path(), "codex-old", true);
    let rejected = version_fixture(directory.path(), "not-codex", false);
    application
        .update_codex_executable(Some(old.clone()))
        .await
        .unwrap();

    assert!(application
        .update_codex_executable(Some(rejected))
        .await
        .is_err());
    assert_eq!(
        application.load().unwrap().codex_executable,
        Some(old.canonicalize().unwrap())
    );
}

#[tokio::test]
async fn clearing_executable_preserves_the_unified_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let application = ApplicationSettingsApplication::new(
        directory.path().join("settings.json"),
        directory.path().join("babysit-settings.json"),
    );
    let executable = version_fixture(directory.path(), "codex", true);
    application.update_theme(ThemePreference::Dark).unwrap();
    application
        .update_codex_executable(Some(executable))
        .await
        .unwrap();

    let cleared = application.update_codex_executable(None).await.unwrap();

    assert_eq!(cleared.codex_executable, None);
    assert_eq!(cleared.theme, ThemePreference::Dark);
    assert_eq!(application.load().unwrap(), cleared);
}

#[test]
fn malformed_settings_are_reported_without_being_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    std::fs::write(&settings_path, "{not-json").unwrap();
    let application = ApplicationSettingsApplication::new(
        settings_path.clone(),
        directory.path().join("babysit-settings.json"),
    );

    assert!(matches!(
        application.load(),
        Err(super::ApplicationSettingsError::Malformed(_))
    ));
    assert_eq!(std::fs::read_to_string(settings_path).unwrap(), "{not-json");
}

#[test]
fn unsupported_settings_version_is_rejected_without_replacing_the_file() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let contents = r#"{
        "version": 2,
        "theme": "system",
        "codexExecutable": null,
        "babysitModel": { "mode": "automatic" }
    }"#;
    std::fs::write(&settings_path, contents).unwrap();
    let application = ApplicationSettingsApplication::new(
        settings_path.clone(),
        directory.path().join("babysit-settings.json"),
    );

    assert!(matches!(
        application.load(),
        Err(super::ApplicationSettingsError::UnsupportedVersion(2))
    ));
    assert_eq!(std::fs::read_to_string(settings_path).unwrap(), contents);
}

#[test]
fn failed_theme_write_leaves_the_previous_default_authoritative() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    std::fs::create_dir(&settings_path).unwrap();
    let application = ApplicationSettingsApplication::new(
        settings_path.clone(),
        directory.path().join("babysit-settings.json"),
    );

    assert!(application.update_theme(ThemePreference::Dark).is_err());
    std::fs::remove_dir(settings_path).unwrap();
    assert_eq!(application.load().unwrap().theme, ThemePreference::System);
}

#[test]
fn legacy_executable_is_migrated_once_and_unified_settings_remain_authoritative() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let legacy_path = directory.path().join("babysit-settings.json");
    let first_executable = version_fixture(directory.path(), "codex-first", true);
    let later_executable = directory.path().join("codex-later.exe");
    std::fs::write(
        &legacy_path,
        serde_json::json!({ "executable": first_executable }).to_string(),
    )
    .unwrap();
    let application =
        ApplicationSettingsApplication::new(settings_path.clone(), legacy_path.clone());

    let migrated = application.load().unwrap();
    std::fs::write(
        legacy_path,
        serde_json::json!({ "executable": later_executable }).to_string(),
    )
    .unwrap();
    let repeated = ApplicationSettingsApplication::new(
        settings_path,
        directory.path().join("babysit-settings.json"),
    )
    .load()
    .unwrap();

    assert_eq!(
        migrated.codex_executable,
        Some(first_executable.canonicalize().unwrap())
    );
    assert_eq!(repeated, migrated);
}

#[test]
fn invalid_legacy_executable_is_not_migrated_into_unified_settings() {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("settings.json");
    let legacy_path = directory.path().join("babysit-settings.json");
    let rejected = version_fixture(directory.path(), "not-codex", false);
    std::fs::write(
        &legacy_path,
        serde_json::json!({ "executable": rejected }).to_string(),
    )
    .unwrap();
    let application = ApplicationSettingsApplication::new(settings_path.clone(), legacy_path);

    assert!(matches!(
        application.load(),
        Err(super::ApplicationSettingsError::InvalidCodexExecutable)
    ));
    assert!(!settings_path.exists());
}

#[test]
fn explicit_babysit_model_update_preserves_other_application_settings() {
    let directory = tempfile::tempdir().unwrap();
    let application = ApplicationSettingsApplication::new(
        directory.path().join("settings.json"),
        directory.path().join("legacy.json"),
    );
    application.update_theme(ThemePreference::Dark).unwrap();

    let updated = application
        .update_babysit_model(BabysitModelPreference::Explicit {
            model: "server-model".to_owned(),
            reasoning_effort: Some("high".to_owned()),
        })
        .unwrap();

    assert_eq!(updated.theme, ThemePreference::Dark);
    assert_eq!(
        updated.babysit_model,
        BabysitModelPreference::Explicit {
            model: "server-model".to_owned(),
            reasoning_effort: Some("high".to_owned()),
        }
    );
}

#[test]
fn babysit_model_preference_uses_public_camel_case_dto_and_reads_legacy_settings() {
    let current = BabysitModelPreference::Explicit {
        model: "server-model".to_owned(),
        reasoning_effort: Some("high".to_owned()),
    };
    assert_eq!(
        serde_json::to_value(&current).unwrap(),
        serde_json::json!({
            "kind": "explicit",
            "modelId": "server-model",
            "reasoningEffort": "high"
        })
    );

    let legacy: BabysitModelPreference = serde_json::from_value(serde_json::json!({
        "mode": "explicit",
        "model": "legacy-model",
        "reasoning_effort": null
    }))
    .unwrap();
    assert_eq!(
        legacy,
        BabysitModelPreference::Explicit {
            model: "legacy-model".to_owned(),
            reasoning_effort: None,
        }
    );
}
