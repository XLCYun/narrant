use super::*;

#[test]
fn explicit_executable_is_authoritative_over_discovery_fallbacks() {
    assert_eq!(
        candidate_paths(
            Some("selected-claude".into()),
            Some("environment-claude".into()),
        ),
        vec![std::path::PathBuf::from("selected-claude")],
    );
}

#[test]
fn fallback_candidates_are_deduplicated() {
    assert_eq!(
        candidate_paths(None, Some("claude".into())),
        vec![std::path::PathBuf::from("claude")],
    );
}
