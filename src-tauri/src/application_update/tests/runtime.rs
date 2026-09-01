use super::*;

#[test]
fn trusted_manifest_is_bounded_after_builder_parsing() {
    assert!(manifest_within_limit(&serde_json::json!({
        "version": "1.2.0",
        "notes": "small"
    })));
    assert!(!manifest_within_limit(&serde_json::json!({
        "notes": "x".repeat(MAX_MANIFEST_BYTES)
    })));
}
