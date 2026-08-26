use super::super::{
    AnthropicAdapter, AnthropicAdapterConfig, AnthropicAdapterError, AnthropicAdapterRequest,
};
use crate::babysit::repository_tools::RepositoryTools;
use crate::operation::CancellationSignal;
use httpmock::prelude::*;
use serde_json::json;

#[tokio::test]
async fn adapter_uses_messages_endpoint_and_tool_output_with_adaptive_effort() {
    let server = MockServer::start_async().await;
    let response = json!({
        "id": "msg_fixture",
        "type": "message",
        "role": "assistant",
        "model": "user-entered-model",
        "content": [{
            "type": "tool_use",
            "id": "final_fixture",
            "name": "final_result",
            "input": {
                "before": {"1": "Explain the before line."},
                "after": {"1": "Explain the after line."}
            }
        }],
        "stop_reason": "tool_use",
        "stop_sequence": null,
        "usage": {"input_tokens": 3, "output_tokens": 5}
    });
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/messages")
                .header("x-api-key", "fixture-key")
                .body_contains(r#""model":"user-entered-model""#)
                .body_contains(r#""thinking":{"type":"adaptive"}"#)
                .body_contains(r#""output_config":{"effort":"low"}"#)
                .body_contains(r#""name":"read""#)
                .body_contains(r#""name":"glob""#)
                .body_contains(r#""name":"grep""#)
                .body_contains(r#""name":"final_result""#);
            then.status(200)
                .header("content-type", "application/json")
                .json_body(response);
        })
        .await;

    let workspace = tempfile::tempdir().expect("fixture workspace should be created");
    let repository_tools = RepositoryTools::new(workspace.path())
        .expect("fixture workspace should bind repository tools");
    let adapter = AnthropicAdapter::new(AnthropicAdapterConfig::new(
        server.base_url(),
        Some("fixture-key".to_owned()),
        "user-entered-model",
        "low",
    ))
    .expect("fixture adapter should build");

    let result = adapter
        .complete(AnthropicAdapterRequest::new(
            "Explain this changed line.",
            Vec::new(),
            repository_tools,
            CancellationSignal::default(),
        ))
        .await
        .expect("fixture response should complete");

    mock.assert_async().await;
    assert_eq!(
        result.output,
        json!({
            "before": {"1": "Explain the before line."},
            "after": {"1": "Explain the after line."}
        })
    );
}

#[tokio::test]
async fn adapter_keeps_an_empty_optional_key_and_maps_every_effort() {
    for effort in ["low", "medium", "high"] {
        let server = MockServer::start_async().await;
        let response = json!({
            "id": "msg_fixture", "type": "message", "role": "assistant", "model": "model",
            "content": [{"type": "tool_use", "id": "final", "name": "final_result",
                "input": {"before": {}, "after": {}}}],
            "stop_reason": "tool_use", "stop_sequence": null,
            "usage": {"input_tokens": 1, "output_tokens": 1}
        });
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/messages")
                    .header("x-api-key", "")
                    .body_contains(&format!(r#""output_config":{{"effort":"{effort}"}}"#));
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(response);
            })
            .await;
        let workspace = tempfile::tempdir().expect("fixture workspace should be created");
        let adapter = AnthropicAdapter::new(AnthropicAdapterConfig::new(
            server.base_url(),
            None,
            "model",
            effort,
        ))
        .expect("fixture adapter should build");
        adapter
            .complete(AnthropicAdapterRequest::new(
                "prompt",
                Vec::new(),
                RepositoryTools::new(workspace.path()).unwrap(),
                CancellationSignal::default(),
            ))
            .await
            .expect("effort fixture should complete");
        mock.assert_async().await;
    }
}

#[tokio::test]
async fn adapter_rejects_early_cancellation_without_contacting_anthropic() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/messages");
            then.status(500);
        })
        .await;
    let workspace = tempfile::tempdir().expect("fixture workspace should be created");
    let cancellation = crate::operation::TestCancellation::default();
    cancellation.cancel();
    let adapter = AnthropicAdapter::new(AnthropicAdapterConfig::new(
        server.base_url(),
        None,
        "model",
        "medium",
    ))
    .unwrap();
    let result = adapter
        .complete(AnthropicAdapterRequest::new(
            "secret prompt",
            Vec::new(),
            RepositoryTools::new(workspace.path()).unwrap(),
            (*cancellation).clone(),
        ))
        .await;
    assert!(matches!(result, Err(AnthropicAdapterError::Cancelled)));
    assert_eq!(mock.hits_async().await, 0);
}

#[tokio::test]
async fn adapter_redacts_http_and_protocol_error_content() {
    let server = MockServer::start_async().await;
    let secret = "never-print-this-secret";
    server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/messages");
            then.status(500).body(secret);
        })
        .await;
    let workspace = tempfile::tempdir().expect("fixture workspace should be created");
    let adapter = AnthropicAdapter::new(AnthropicAdapterConfig::new(
        server.base_url(),
        Some(secret.to_owned()),
        "model",
        "high",
    ))
    .unwrap();
    let error = adapter
        .complete(AnthropicAdapterRequest::new(
            secret,
            Vec::new(),
            RepositoryTools::new(workspace.path()).unwrap(),
            CancellationSignal::default(),
        ))
        .await
        .expect_err("HTTP failure should be redacted");
    let diagnostic = format!("{error} {error:?}");
    assert!(!diagnostic.contains(secret));
    assert!(!diagnostic.contains(&server.base_url()));
}
