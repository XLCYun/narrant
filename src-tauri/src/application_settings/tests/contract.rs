use super::{ApplicationSettingsApplication, BabysitModelPreference, ThemePreference};

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
    let first_executable = directory.path().join("codex-first.exe");
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

    assert_eq!(migrated.codex_executable, Some(first_executable));
    assert_eq!(repeated, migrated);
}
