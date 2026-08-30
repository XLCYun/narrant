use super::{
    ApplicationSettingsApplication, BabysitProvider, BabysitProviderSettings,
    CustomProviderConnection, CustomProviderEffort, CustomProviderProtocol, ThemePreference,
};
use crate::application_settings::{ApplicationSettingsError, CustomCredentialAdapter};
use std::sync::{Arc, Mutex};

#[test]
fn missing_settings_resolve_to_provider_aware_defaults_without_writing() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("settings.json");
    let application = ApplicationSettingsApplication::new(path.clone());
    let settings = application.load().unwrap();
    assert_eq!(settings.version, 1);
    assert_eq!(settings.theme, ThemePreference::System);
    assert_eq!(settings.babysit.provider, BabysitProvider::Custom);
    assert_eq!(settings.babysit.codex, BabysitProviderSettings::default());
    assert_eq!(
        settings.babysit.claude_code,
        BabysitProviderSettings::default()
    );
    assert_eq!(
        settings.babysit.custom,
        super::CustomProviderSettings::default()
    );
    assert!(!path.exists());
}

#[test]
fn settings_v1_round_trip_uses_stable_provider_ids_and_explicit_substructures() {
    let payload = serde_json::json!({"version":1,"theme":"dark","babysit":{"provider":"claude-code","codex":{"executable":null,"model":"gpt-5","reasoningEffort":"high"},"claudeCode":{"executable":null,"model":"sonnet","reasoningEffort":null},"custom":{"protocol":"openai_chat_completions","baseUrl":"https://example.test/v1","model":"fixture-model","effort":"medium","credentialGeneration":0}}});
    let settings: super::ApplicationSettings = serde_json::from_value(payload.clone()).unwrap();
    assert_eq!(settings.babysit.provider, BabysitProvider::ClaudeCode);
    assert_eq!(serde_json::to_value(settings).unwrap(), payload);
}

#[test]
fn custom_settings_require_an_explicit_credential_generation() {
    let payload = serde_json::json!({"version":1,"theme":"dark","babysit":{"provider":"custom","codex":{"executable":null,"model":null,"reasoningEffort":null},"claudeCode":{"executable":null,"model":null,"reasoningEffort":null},"custom":{"protocol":"openai_chat_completions","baseUrl":"https://example.test/v1","model":"fixture-model","effort":"medium"}}});

    assert!(serde_json::from_value::<super::ApplicationSettings>(payload).is_err());
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
        BabysitProvider::Custom
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

#[test]
fn custom_connection_round_trip_keeps_key_only_in_memory() {
    let credentials = Arc::new(MemoryCredentials::default());
    let directory = tempfile::tempdir().unwrap();
    let application = ApplicationSettingsApplication::with_credential_adapter(
        directory.path().join("settings.json"),
        credentials.clone(),
    );
    let connection = custom_connection(Some("secret-key"));

    let saved = application
        .update_custom_connection(connection.clone())
        .unwrap();

    assert_eq!(saved, connection);
    assert_eq!(application.load_custom_connection().unwrap(), connection);
    let settings_json = std::fs::read_to_string(directory.path().join("settings.json")).unwrap();
    assert!(!settings_json.contains("secret-key"));
    assert!(!settings_json.contains("apiKey"));
    assert_eq!(credentials.value(), Some("secret-key".to_owned()));
}

#[test]
fn empty_custom_key_deletes_existing_credential() {
    let credentials = Arc::new(MemoryCredentials::with_value(Some("old-key")));
    let directory = tempfile::tempdir().unwrap();
    let application = ApplicationSettingsApplication::with_credential_adapter(
        directory.path().join("settings.json"),
        credentials.clone(),
    );
    application
        .update_custom_connection(custom_connection(Some("old-key")))
        .unwrap();

    let mut connection = custom_connection(Some(""));
    connection.model = "replacement-model".to_owned();
    let saved = application.update_custom_connection(connection).unwrap();

    assert_eq!(saved.api_key, "");
    assert_eq!(credentials.value(), None);
    assert_eq!(application.load_custom_connection().unwrap().api_key, "");
    assert_eq!(
        application.load().unwrap().babysit.custom.model,
        "replacement-model"
    );
}

#[test]
fn custom_key_replacement_updates_credential_after_non_secret_settings() {
    let credentials = Arc::new(MemoryCredentials::with_value(Some("old-key")));
    let directory = tempfile::tempdir().unwrap();
    let application = ApplicationSettingsApplication::with_credential_adapter(
        directory.path().join("settings.json"),
        credentials.clone(),
    );
    application
        .update_custom_connection(custom_connection(Some("old-key")))
        .unwrap();

    let mut next = custom_connection(Some("new-key"));
    next.base_url = "http://localhost:9000/v1".to_owned();
    application.update_custom_connection(next).unwrap();

    assert_eq!(credentials.value(), Some("new-key".to_owned()));
    assert_eq!(
        application.load().unwrap().babysit.custom.base_url,
        "http://localhost:9000/v1"
    );
}

#[test]
fn failed_custom_credential_mutation_restores_preceding_settings_snapshot() {
    let credentials = Arc::new(MemoryCredentials::with_value(Some("old-key")));
    let directory = tempfile::tempdir().unwrap();
    let application = ApplicationSettingsApplication::with_credential_adapter(
        directory.path().join("settings.json"),
        credentials.clone(),
    );
    application
        .update_custom_connection(custom_connection(Some("old-key")))
        .unwrap();
    credentials
        .fail_mutation
        .store(true, std::sync::atomic::Ordering::Relaxed);
    let mut attempted = custom_connection(Some("new-key"));
    attempted.model = "new-model".to_owned();

    let error = application.update_custom_connection(attempted).unwrap_err();

    assert!(matches!(error, ApplicationSettingsError::Credential(_)));
    let restored = application.load_custom_connection().unwrap();
    assert_eq!(restored.model, "fixture-model");
    assert_eq!(restored.api_key, "old-key");
}

#[test]
fn custom_connection_rejects_missing_url_or_model_and_invalid_effort() {
    let credentials = Arc::new(MemoryCredentials::default());
    let directory = tempfile::tempdir().unwrap();
    let application = ApplicationSettingsApplication::with_credential_adapter(
        directory.path().join("settings.json"),
        credentials,
    );

    let mut missing_url = custom_connection(None);
    missing_url.base_url = "  ".to_owned();
    assert!(matches!(
        application.update_custom_connection(missing_url),
        Err(ApplicationSettingsError::InvalidCustomConnection)
    ));
    let mut missing_model = custom_connection(None);
    missing_model.model = "\n".to_owned();
    assert!(matches!(
        application.update_custom_connection(missing_model),
        Err(ApplicationSettingsError::InvalidCustomConnection)
    ));
}

fn custom_connection(api_key: Option<&str>) -> CustomProviderConnection {
    CustomProviderConnection {
        protocol: CustomProviderProtocol::OpenaiChatCompletions,
        base_url: "https://example.test/v1".to_owned(),
        api_key: api_key.unwrap_or_default().to_owned(),
        model: "fixture-model".to_owned(),
        effort: CustomProviderEffort::Medium,
    }
}

#[derive(Default)]
struct MemoryCredentials {
    value: Mutex<Option<String>>,
    fail_mutation: std::sync::atomic::AtomicBool,
}

impl MemoryCredentials {
    fn with_value(value: Option<&str>) -> Self {
        Self {
            value: Mutex::new(value.map(str::to_owned)),
            fail_mutation: std::sync::atomic::AtomicBool::new(false),
        }
    }

    fn value(&self) -> Option<String> {
        self.value.lock().unwrap().clone()
    }
}

impl CustomCredentialAdapter for MemoryCredentials {
    fn load(&self) -> Result<Option<String>, String> {
        Ok(self.value())
    }

    fn save(&self, value: &str) -> Result<(), String> {
        if self
            .fail_mutation
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return Err("fixture credential failure".to_owned());
        }
        *self.value.lock().unwrap() = Some(value.to_owned());
        Ok(())
    }

    fn delete(&self) -> Result<(), String> {
        if self
            .fail_mutation
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return Err("fixture credential failure".to_owned());
        }
        *self.value.lock().unwrap() = None;
        Ok(())
    }
}

fn preference(model: &str, effort: Option<&str>) -> BabysitProviderSettings {
    BabysitProviderSettings {
        executable: None,
        model: Some(model.to_owned()),
        reasoning_effort: effort.map(str::to_owned),
    }
}
