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
        "id": "hidden", "name": "Hidden", "hidden": true, "isDefault": false,
        "supportedReasoningEfforts": ["low"], "defaultReasoningEffort": "low"
    }]), Some("page-2")).append_to(&mut models);
    assert_eq!(cursor.as_deref(), Some("page-2"));
    page(json!([{
        "id": "current", "name": "Current", "isDefault": true,
        "supportedReasoningEfforts": ["medium", "high"], "defaultReasoningEffort": "medium"
    }]), None).append_to(&mut models);

    let catalog = CodexModelCatalog::from_pages(models).unwrap();
    let resolved = catalog.resolve(&BabysitModelPreference::Automatic).unwrap();

    assert_eq!(catalog.models.len(), 1);
    assert_eq!(resolved.model, "current");
    assert_eq!(resolved.reasoning_effort, "medium");
}

#[test]
fn explicit_model_rejects_missing_or_unsupported_selection_without_fallback() {
    let mut models = Vec::new();
    page(json!([{
        "id": "current", "name": "Current", "isDefault": true,
        "supportedReasoningEfforts": ["low"], "defaultReasoningEffort": "low"
    }]), None).append_to(&mut models);
    let catalog = CodexModelCatalog::from_pages(models).unwrap();

    assert!(catalog.resolve(&BabysitModelPreference::Explicit {
        model: "removed".to_owned(), reasoning_effort: None
    }).is_err());
    assert!(catalog.resolve(&BabysitModelPreference::Explicit {
        model: "current".to_owned(), reasoning_effort: Some("high".to_owned())
    }).is_err());
}

#[test]
fn malformed_or_ambiguous_default_catalog_is_rejected() {
    assert!(ModelListPage::decode(json!({"data": [{"id": "incomplete"}]})).is_err());
    let mut models = Vec::new();
    page(json!([{
        "id": "a", "name": "A", "isDefault": false,
        "supportedReasoningEfforts": ["low"], "defaultReasoningEffort": "low"
    }]), None).append_to(&mut models);
    assert!(CodexModelCatalog::from_pages(models).is_err());
}
