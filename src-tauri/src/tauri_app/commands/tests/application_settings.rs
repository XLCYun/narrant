use super::{update_babysit_preferences, update_babysit_provider};
use crate::application_settings::{
    ApplicationSettingsApplication, BabysitProvider, BabysitProviderSettings,
};
use crate::babysit::{BabysitApplication, FakeBabysitProvider, TestBabysitApplication};
use std::sync::Arc;

/// 构造不连接外部 provider 的 typed command 测试边界。
fn command_seam() -> (
    tempfile::TempDir,
    ApplicationSettingsApplication,
    BabysitApplication,
) {
    let directory = tempfile::tempdir().unwrap();
    let settings = ApplicationSettingsApplication::new(directory.path().join("settings.json"));
    let babysit = TestBabysitApplication::with_provider(Arc::new(FakeBabysitProvider::default()));
    (directory, settings, babysit)
}

#[tokio::test]
async fn codex_explicit_model_requires_a_provider_bound_catalog_revision_before_write() {
    let (_directory, settings, babysit) = command_seam();
    let preference = BabysitProviderSettings {
        executable: None,
        model: Some("gpt-5".to_owned()),
        reasoning_effort: Some("high".to_owned()),
    };

    assert!(update_babysit_preferences(
        &settings,
        &babysit,
        BabysitProvider::Codex,
        preference,
        None,
    )
    .await
    .is_err());
    assert_eq!(
        settings.load().unwrap(),
        crate::application_settings::ApplicationSettings::default()
    );
}

#[tokio::test]
async fn claude_valid_combination_is_saved_as_one_complete_value() {
    let (_directory, settings, babysit) = command_seam();
    let preference = BabysitProviderSettings {
        executable: None,
        model: Some("sonnet".to_owned()),
        reasoning_effort: Some("high".to_owned()),
    };

    let updated = update_babysit_preferences(
        &settings,
        &babysit,
        BabysitProvider::ClaudeCode,
        preference.clone(),
        None,
    )
    .await
    .unwrap();

    assert_eq!(updated.babysit.claude_code, preference);
}

#[tokio::test]
async fn invalid_claude_executable_is_rejected_before_settings_change() {
    let (directory, settings, babysit) = command_seam();
    let preference = BabysitProviderSettings {
        executable: Some(directory.path().join("missing-claude")),
        model: Some("sonnet".to_owned()),
        reasoning_effort: None,
    };

    assert!(update_babysit_preferences(
        &settings,
        &babysit,
        BabysitProvider::ClaudeCode,
        preference,
        None,
    )
    .await
    .is_err());
    assert_eq!(
        settings.load().unwrap(),
        crate::application_settings::ApplicationSettings::default()
    );
}

#[test]
fn provider_command_switches_the_authoritative_snapshot() {
    let (_directory, settings, babysit) = command_seam();

    let updated =
        update_babysit_provider(&settings, &babysit, BabysitProvider::ClaudeCode).unwrap();

    assert_eq!(updated.babysit.provider, BabysitProvider::ClaudeCode);
    assert_eq!(settings.load().unwrap(), updated);
}
