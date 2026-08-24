use super::{BabysitAgentProvider, ClaudeCodeProvider, ClaudeModelCatalog, ResolvedClaudeModel};
use crate::application_settings::BabysitProviderSettings;

#[test]
fn claude_code_uses_the_stable_provider_identity() {
    assert_eq!(ClaudeCodeProvider::new("claude").provider_id(), "claude-code");
}

#[test]
fn offline_catalog_exposes_only_supported_aliases_and_custom() {
    let catalog = ClaudeModelCatalog::offline();
    let ids = catalog
        .models
        .iter()
        .map(|model| model.id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        ids,
        ["default", "best", "fable", "opus", "sonnet", "haiku", "custom"]
    );
    assert_eq!(catalog.models[1].name, "Best（使用最强的可用模型）");
    assert!(!ids
        .iter()
        .any(|id| matches!(*id, "opusplan" | "ultracode") || id.contains("[1m]")));
}

#[test]
fn known_aliases_enforce_their_offline_effort_capabilities() {
    let catalog = ClaudeModelCatalog::offline();
    let haiku = catalog
        .models
        .iter()
        .find(|model| model.id == "haiku")
        .unwrap();
    let custom = catalog
        .models
        .iter()
        .find(|model| model.id == "custom")
        .unwrap();

    assert_eq!(haiku.supported_reasoning_efforts, [None]);
    assert_eq!(
        custom.supported_reasoning_efforts,
        vec![
            None,
            Some("low".to_owned()),
            Some("medium".to_owned()),
            Some("high".to_owned()),
            Some("xhigh".to_owned()),
            Some("max".to_owned())
        ]
    );
    assert!(catalog
        .resolve(&BabysitProviderSettings {
            model: Some("haiku".to_owned()),
            reasoning_effort: Some("low".to_owned()),
            ..BabysitProviderSettings::default()
        })
        .is_err());
}

#[test]
fn resolver_omits_null_overrides_and_preserves_explicit_values() {
    let catalog = ClaudeModelCatalog::offline();

    assert_eq!(
        catalog
            .resolve(&BabysitProviderSettings::default())
            .unwrap(),
        ResolvedClaudeModel {
            model: None,
            reasoning_effort: None,
        }
    );
    assert_eq!(
        catalog
            .resolve(&BabysitProviderSettings {
                model: None,
                reasoning_effort: Some("high".to_owned()),
                ..BabysitProviderSettings::default()
            })
            .unwrap(),
        ResolvedClaudeModel {
            model: None,
            reasoning_effort: Some("high".to_owned()),
        }
    );
    assert_eq!(
        catalog
            .resolve(&BabysitProviderSettings {
                model: Some("sonnet".to_owned()),
                reasoning_effort: None,
                ..BabysitProviderSettings::default()
            })
            .unwrap(),
        ResolvedClaudeModel {
            model: Some("sonnet".to_owned()),
            reasoning_effort: None,
        }
    );
}

#[test]
fn custom_model_requires_non_empty_input_and_preserves_cloud_identifier() {
    let catalog = ClaudeModelCatalog::offline();
    let cloud_model = "  arn:aws:bedrock:region:account:inference-profile/example  ";

    assert_eq!(
        catalog
            .resolve(&BabysitProviderSettings {
                model: Some(cloud_model.to_owned()),
                reasoning_effort: Some("max".to_owned()),
                ..BabysitProviderSettings::default()
            })
            .unwrap(),
        ResolvedClaudeModel {
            model: Some(cloud_model.to_owned()),
            reasoning_effort: Some("max".to_owned()),
        }
    );
    assert!(catalog
        .resolve(&BabysitProviderSettings {
            model: Some("   ".to_owned()),
            reasoning_effort: None,
            ..BabysitProviderSettings::default()
        })
        .is_err());
}
