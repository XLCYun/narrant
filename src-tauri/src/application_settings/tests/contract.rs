use super::{
    ApplicationSettingsApplication, BabysitProvider, BabysitProviderSettings, ThemePreference,
};

#[test]
fn missing_settings_resolve_to_provider_aware_defaults_without_writing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    let application = ApplicationSettingsApplication::new(path.clone());
    let settings = application.load().unwrap();
    assert_eq!(settings.version, 1);
    assert_eq!(settings.theme, ThemePreference::System);
    assert_eq!(settings.babysit.provider, BabysitProvider::Codex);
    assert_eq!(settings.babysit.codex, BabysitProviderSettings::default());
    assert_eq!(
        settings.babysit.claude_code,
        BabysitProviderSettings::default()
    );
    assert!(!path.exists());
}

#[test]
fn settings_v1_round_trip_uses_stable_provider_ids_and_explicit_substructures() {
    let payload = serde_json::json!({"version":1,"theme":"dark","babysit":{"provider":"claude-code","codex":{"executable":null,"model":"gpt-5","reasoningEffort":"high"},"claudeCode":{"executable":null,"model":"sonnet","reasoningEffort":null}}});
    let settings: super::ApplicationSettings = serde_json::from_value(payload.clone()).unwrap();
    assert_eq!(settings.babysit.provider, BabysitProvider::ClaudeCode);
    assert_eq!(serde_json::to_value(settings).unwrap(), payload);
}

#[test]
fn switching_provider_retains_independent_preferences() {
    let directory = tempfile::tempdir().unwrap();
    let application = ApplicationSettingsApplication::new(directory.path().join("settings.json"));
    application
        .update_provider_settings(BabysitProvider::Codex, preference("gpt-5", Some("high")))
        .unwrap();
    application
        .update_provider_settings(BabysitProvider::ClaudeCode, preference("sonnet", None))
        .unwrap();
    let settings = application
        .update_provider(BabysitProvider::ClaudeCode)
        .unwrap();
    assert_eq!(settings.babysit.codex.model.as_deref(), Some("gpt-5"));
    assert_eq!(
        settings.babysit.claude_code.model.as_deref(),
        Some("sonnet")
    );
}

#[test]
fn full_provider_combination_is_updated_atomically() {
    let directory = tempfile::tempdir().unwrap();
    let application = ApplicationSettingsApplication::new(directory.path().join("settings.json"));
    let combination = preference("new-model", Some("max"));
    let settings = application
        .update_provider_settings(BabysitProvider::ClaudeCode, combination.clone())
        .unwrap();
    assert_eq!(settings.babysit.claude_code, combination);
    assert_eq!(application.load().unwrap(), settings);
}

#[test]
fn failed_atomic_write_retains_previous_snapshot() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    let application = ApplicationSettingsApplication::new(path.clone());
    application.update_theme(ThemePreference::Dark).unwrap();
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert!(application
        .update_provider(BabysitProvider::ClaudeCode)
        .is_err());
    std::fs::remove_dir(&path).unwrap();
    assert_eq!(
        application.load().unwrap().babysit.provider,
        BabysitProvider::Codex
    );
}

#[test]
fn legacy_flat_schema_is_rejected_and_not_replaced() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    let contents = r#"{"version":1,"theme":"system","codexExecutable":null,"babysitModel":{"kind":"automatic"}}"#;
    std::fs::write(&path, contents).unwrap();
    let application = ApplicationSettingsApplication::new(path.clone());
    assert!(application.load().is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), contents);
}

#[test]
fn model_and_reasoning_effort_are_independent_nullable_preferences() {
    let directory = tempfile::tempdir().unwrap();
    let application = ApplicationSettingsApplication::new(directory.path().join("settings.json"));

    let saved = application
        .update_provider_settings(
        BabysitProvider::Codex,
        BabysitProviderSettings {
            executable: None,
            model: None,
            reasoning_effort: Some("high".to_owned()),
        },
    )
        .unwrap();

    assert_eq!(
        saved.babysit.codex,
        BabysitProviderSettings {
            executable: None,
            model: None,
            reasoning_effort: Some("high".to_owned()),
        }
    );
    assert_eq!(application.load().unwrap(), saved);
}

#[test]
fn claude_haiku_accepts_only_default_effort_and_custom_models_are_non_empty() {
    let directory = tempfile::tempdir().unwrap();
    let application = ApplicationSettingsApplication::new(directory.path().join("settings.json"));

    assert!(application
        .update_provider_settings(
            BabysitProvider::ClaudeCode,
            preference("haiku", Some("low")),
        )
        .is_err());
    assert!(application
        .update_provider_settings(BabysitProvider::ClaudeCode, preference("   ", None),)
        .is_err());
    assert!(application
        .update_provider_settings(
            BabysitProvider::ClaudeCode,
            preference("vendor/custom-model", Some("max")),
        )
        .is_ok());
}

fn preference(model: &str, effort: Option<&str>) -> BabysitProviderSettings {
    BabysitProviderSettings {
        executable: None,
        model: Some(model.to_owned()),
        reasoning_effort: effort.map(str::to_owned),
    }
}
