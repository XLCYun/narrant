use super::{CodexModelCatalog, ModelListPage};
use crate::application_settings::BabysitModelPreference;
use serde_json::json;

fn page(models: serde_json::Value, cursor: Option<&str>) -> ModelListPage {
    ModelListPage::decode(json!({"data": models, "nextCursor": cursor})).unwrap()
}

#[test]
fn paginated_catalog_filters_hidden_models_and_resolves_server_default() {
    let mut models = Vec::new();
    let cursor = page(json!([{
        "id": "hidden-entry", "model": "hidden-runtime", "displayName": "Hidden", "hidden": true, "isDefault": false,
        "supportedReasoningEfforts": ["low"], "defaultReasoningEffort": "low"
    }]), Some("page-2")).append_to(&mut models);
    assert_eq!(cursor.as_deref(), Some("page-2"));
    page(json!([{
        "id": "current-entry", "model": "current-runtime", "displayName": "Current", "isDefault": true,
        "supportedReasoningEfforts": ["medium", "high"], "defaultReasoningEffort": "medium"
    }]), None).append_to(&mut models);

    let catalog = CodexModelCatalog::from_pages(models).unwrap();
    let resolved = catalog.resolve(&BabysitModelPreference::Automatic).unwrap();

    assert_eq!(catalog.models.len(), 1);
    assert_eq!(catalog.models[0].id, "current-entry");
    assert_eq!(resolved.model, "current-runtime");
    assert_eq!(resolved.reasoning_effort, "medium");
}

#[test]
fn explicit_model_rejects_missing_or_unsupported_selection_without_fallback() {
    let mut models = Vec::new();
    page(json!([{
        "id": "current", "model": "current-runtime", "displayName": "Current", "isDefault": true,
        "supportedReasoningEfforts": ["low"], "defaultReasoningEffort": "low"
    }]), None).append_to(&mut models);
    let catalog = CodexModelCatalog::from_pages(models).unwrap();

    assert!(catalog
        .resolve(&BabysitModelPreference::Explicit {
            model: "removed".to_owned(),
            reasoning_effort: None
        })
        .is_err());
    assert!(catalog
        .resolve(&BabysitModelPreference::Explicit {
            model: "current".to_owned(),
            reasoning_effort: Some("high".to_owned())
        })
        .is_err());
}

#[test]
fn current_app_server_shape_keeps_catalog_id_separate_from_runtime_model() {
    let mut models = Vec::new();
    page(json!([{
        "id": "catalog-entry", "model": "provider-runtime-model", "displayName": "Provider Model",
        "isDefault": true, "hidden": false,
        "supportedReasoningEfforts": [{"reasoningEffort": "low", "description": "Fast"}],
        "defaultReasoningEffort": "low"
    }]), None).append_to(&mut models);

    let catalog = CodexModelCatalog::from_pages(models).unwrap();
    let resolved = catalog
        .resolve(&BabysitModelPreference::Explicit {
            model: "catalog-entry".to_owned(),
            reasoning_effort: None,
        })
        .unwrap();

    assert_eq!(catalog.models[0].id, "catalog-entry");
    assert_eq!(resolved.model, "provider-runtime-model");
}

#[test]
fn malformed_or_ambiguous_default_catalog_is_rejected() {
    assert!(ModelListPage::decode(json!({"data": [{"id": "incomplete"}]})).is_err());
    let mut models = Vec::new();
    page(
        json!([{
            "id": "a", "model": "runtime-a", "displayName": "A", "isDefault": false,
            "supportedReasoningEfforts": ["low"], "defaultReasoningEffort": "low"
        }]),
        None,
    )
    .append_to(&mut models);
    assert!(CodexModelCatalog::from_pages(models).is_err());
}
