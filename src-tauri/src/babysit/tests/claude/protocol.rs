use super::*;

/// 协议畸形不得把 Claude stdout 或工作区路径泄漏给调用方。
#[tokio::test]
async fn malformed_result_is_exposed_only_as_a_redacted_provider_error() {
    let (provider, temp, _) = provider("malformed", Duration::from_secs(5));
    let error = provider
        .generate(
            request(temp.path().to_string_lossy().into_owned()),
            OperationContext::background().cancellation(),
        )
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        crate::babysit::BabysitError::Provider { .. }
    ));
    assert!(!error.to_string().contains("not json"));
    assert!(!error
        .to_string()
        .contains(temp.path().to_string_lossy().as_ref()));
}

/// Claude 的稳定认证 subtype 必须映射为 provider-aware setup failure。
#[tokio::test]
async fn authentication_subtype_maps_to_signed_out() {
    let (provider, temp, _) = provider("auth", Duration::from_secs(5));
    let error = provider
        .generate(
            request(temp.path().to_string_lossy().into_owned()),
            OperationContext::background().cancellation(),
        )
        .await
        .unwrap_err();

    assert_eq!(
        error,
        crate::babysit::BabysitError::provider_setup(
            "claude-code",
            BabysitProviderSetupFailure::SignedOut,
        )
    );
}
