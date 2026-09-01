use super::super::{
    diagnostics::UpdateDiagnostics, UpdateFailure, UpdateFailureCategory, UpdateOperationId,
};

#[test]
fn persisted_diagnostics_exclude_signature_credentials_and_user_paths() {
    let directory = tempfile::tempdir().unwrap();
    let diagnostics = UpdateDiagnostics::new(directory.path().join("updater.jsonl"));
    let secret = "signature=RW-secret&token=credential";
    let user_path = "/Users/private-person/Downloads/update.exe";
    let failure = UpdateFailure::new(
        UpdateFailureCategory::Verification,
        format!("{secret} at {user_path}"),
    );

    diagnostics.record_failure(UpdateOperationId::test_fixture(9).value(), &failure);

    let rendered = std::fs::read_to_string(diagnostics.path()).unwrap();
    assert!(!rendered.contains(secret));
    assert!(!rendered.contains(user_path));
    assert!(rendered.contains("verification"));
    assert!(rendered.contains("\"operationId\":9"));
}
