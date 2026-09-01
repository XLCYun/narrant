use super::super::{
    AnthropicAdapter, AnthropicAdapterConfig, AnthropicAdapterError, AnthropicAdapterRequest,
};
use crate::application_settings::CustomProviderEffort;
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
                .matches(|request| {
                    let body: serde_json::Value = serde_json::from_slice(
                        request.body.as_deref().expect("request should have a body"),
                    )
                    .expect("request body should be JSON");
                    body["max_tokens"] == json!(65_536)
                })
                .body_contains(r#""thinking":{"type":"adaptive"}"#)
                .body_contains(r#""output_config":{"effort":"low"}"#)
                .body_contains(r#""name":"read""#)
                .body_contains(r#""name":"glob""#)
                .body_contains(r#""name":"grep""#)
                .body_contains(r#""name":"final_result""#)
                .matches(|request| {
                    let body: serde_json::Value = serde_json::from_slice(
                        request.body.as_deref().expect("request should have a body"),
                    )
                    .expect("request body should be JSON");
                    let names = body["tools"]
                        .as_array()
                        .expect("tools should be an array")
                        .iter()
                        .map(|tool| tool["name"].as_str().unwrap())
                        .collect::<std::collections::BTreeSet<_>>();
                    names
                        == std::collections::BTreeSet::from([
                            "final_result",
                            "glob",
                            "grep",
                            "read",
                        ])
                        && body["output_config"].get("format").is_none()
                });
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
        CustomProviderEffort::Low,
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
    assert!(!result.conversation.is_empty());
}

/// Provider 默认值保留 Anthropic tools，但不发送 Narrant 的 thinking/effort 覆盖。
#[tokio::test]
async fn adapter_provider_default_omits_thinking_configuration() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/messages").matches(|request| {
                let body: serde_json::Value =
                    serde_json::from_slice(request.body.as_deref().expect("请求应包含 JSON body"))
                        .expect("请求 body 应为 JSON");
                body.get("thinking").is_none()
                    && body.get("output_config").is_none()
                    && body["tools"]
                        .as_array()
                        .is_some_and(|tools| !tools.is_empty())
            });
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({
                    "id": "msg_fixture",
                    "type": "message",
                    "role": "assistant",
                    "model": "fixture-model",
                    "content": [{
                        "type": "tool_use",
                        "id": "final_fixture",
                        "name": "final_result",
                        "input": {"before": {}, "after": {}}
                    }],
                    "stop_reason": "tool_use",
                    "stop_sequence": null,
                    "usage": {"input_tokens": 1, "output_tokens": 1}
                }));
        })
        .await;
    let workspace = tempfile::tempdir().expect("创建 fixture workspace");
    let repository_tools = RepositoryTools::new(workspace.path()).expect("绑定 repository tools");
    let adapter = AnthropicAdapter::new(AnthropicAdapterConfig::new(
        server.base_url(),
        Some("fixture-key".to_owned()),
        "fixture-model",
        CustomProviderEffort::ProviderDefault,
    ))
    .expect("创建 adapter");

    adapter
        .complete(AnthropicAdapterRequest::new(
            "review",
            Vec::new(),
            repository_tools,
            CancellationSignal::default(),
        ))
        .await
        .expect("Provider 默认值请求应成功");

    mock.assert_async().await;
}

/// 达到 Anthropic 输出预算时不得把截断结果当作可修复的 final output。
#[tokio::test]
async fn adapter_rejects_anthropic_max_tokens_stop_reason() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/messages");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({
                    "id": "length-limited",
                    "type": "message",
                    "role": "assistant",
                    "model": "fixture-model",
                    "content": [{
                        "type": "tool_use",
                        "id": "final_fixture",
                        "name": "final_result",
                        "input": {"before": {}, "after": {}}
                    }],
                    "stop_reason": "max_tokens",
                    "stop_sequence": null,
                    "usage": {"input_tokens": 1, "output_tokens": 65_536}
                }));
        })
        .await;
    let workspace = tempfile::tempdir().expect("创建 fixture workspace");
    let adapter = AnthropicAdapter::new(AnthropicAdapterConfig::new(
        server.base_url(),
        Some("fixture-key".to_owned()),
        "fixture-model",
        CustomProviderEffort::Medium,
    ))
    .expect("创建 adapter");

    let result = adapter
        .complete(AnthropicAdapterRequest::new(
            "review",
            Vec::new(),
            RepositoryTools::new(workspace.path()).expect("绑定 repository tools"),
            CancellationSignal::default(),
        ))
        .await;

    mock.assert_async().await;
    assert!(matches!(
        result,
        Err(AnthropicAdapterError::OutputLimit { limit: 65_536 })
    ));
}

/// provider 拒绝固定预算时必须直接失败，不能改用较小预算重试。
#[tokio::test]
async fn adapter_propagates_max_tokens_rejection_without_retrying() {
    let server = MockServer::start_async().await;
    let rejected = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/messages").matches(|request| {
                let body: serde_json::Value =
                    serde_json::from_slice(request.body.as_deref().expect("请求应包含 JSON body"))
                        .expect("请求 body 应为 JSON");
                body["max_tokens"] == json!(65_536)
            });
            then.status(400).json_body(json!({
                "type": "error",
                "error": {
                    "type": "invalid_request_error",
                    "message": "fixture rejects max_tokens"
                }
            }));
        })
        .await;
    let unexpected_fallback = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/messages");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({
                    "id": "unexpected-fallback",
                    "type": "message",
                    "role": "assistant",
                    "model": "fixture-model",
                    "content": [{
                        "type": "tool_use",
                        "id": "final_fixture",
                        "name": "final_result",
                        "input": {"before": {}, "after": {}}
                    }],
                    "stop_reason": "tool_use",
                    "stop_sequence": null,
                    "usage": {"input_tokens": 1, "output_tokens": 1}
                }));
        })
        .await;
    let workspace = tempfile::tempdir().expect("创建 fixture workspace");
    let adapter = AnthropicAdapter::new(AnthropicAdapterConfig::new(
        server.base_url(),
        Some("fixture-key".to_owned()),
        "fixture-model",
        CustomProviderEffort::Medium,
    ))
    .expect("创建 adapter");

    let result = adapter
        .complete(AnthropicAdapterRequest::new(
            "review",
            Vec::new(),
            RepositoryTools::new(workspace.path()).expect("绑定 repository tools"),
            CancellationSignal::default(),
        ))
        .await;

    assert!(matches!(result, Err(AnthropicAdapterError::Transport(_))));
    assert_eq!(rejected.hits_async().await, 1);
    assert_eq!(unexpected_fallback.hits_async().await, 0);
}

#[tokio::test]
async fn adapter_drops_an_in_flight_http_future_when_cancelled() {
    let server = MockServer::start_async().await;
    let mock = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/messages");
            then.status(200)
                .delay(std::time::Duration::from_secs(2))
                .header("content-type", "application/json")
                .json_body(json!({
                    "id": "late", "type": "message", "role": "assistant", "model": "model",
                    "content": [{"type": "tool_use", "id": "late", "name": "final_result",
                        "input": {"before": {}, "after": {}}}],
                    "stop_reason": "tool_use", "stop_sequence": null,
                    "usage": {"input_tokens": 1, "output_tokens": 1}
                }));
        })
        .await;
    let workspace = tempfile::tempdir().expect("fixture workspace should be created");
    let cancellation = crate::operation::TestCancellation::default();
    let adapter = AnthropicAdapter::new(AnthropicAdapterConfig::new(
        server.base_url(),
        None,
        "model",
        CustomProviderEffort::Medium,
    ))
    .unwrap();
    let request = AnthropicAdapterRequest::new(
        "prompt",
        Vec::new(),
        RepositoryTools::new(workspace.path()).unwrap(),
        (*cancellation).clone(),
    );
    let task = tokio::spawn(async move { adapter.complete(request).await });
    for _ in 0..50 {
        if mock.hits_async().await == 1 {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert_eq!(mock.hits_async().await, 1);
    cancellation.cancel();
    let started = std::time::Instant::now();
    assert!(matches!(
        task.await.expect("adapter task should join"),
        Err(AnthropicAdapterError::Cancelled),
    ));
    assert!(started.elapsed() < std::time::Duration::from_millis(500));
}

#[tokio::test]
async fn adapter_converts_anthropic_tool_use_and_tool_result_round_trip() {
    let server = MockServer::start_async().await;
    let first = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/messages").matches(|request| {
                let Some(body) = request.body.as_deref() else {
                    return false;
                };
                let Ok(json) = serde_json::from_slice::<serde_json::Value>(body) else {
                    return false;
                };
                json["max_tokens"] == json!(65_536)
                    && !String::from_utf8_lossy(body).contains(r#""type":"tool_result""#)
            });
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({
                    "id": "read_request", "type": "message", "role": "assistant", "model": "model",
                    "content": [{"type": "tool_use", "id": "toolu_read_fixture", "name": "read",
                        "input": {"path": "fixture.rs", "offset": 0}}],
                    "stop_reason": "tool_use", "stop_sequence": null,
                    "usage": {"input_tokens": 1, "output_tokens": 1}
                }));
        })
        .await;
    let second = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/messages")
                .matches(|request| {
                    let body: serde_json::Value = serde_json::from_slice(
                        request.body.as_deref().expect("请求应包含 JSON body"),
                    )
                    .expect("请求 body 应为 JSON");
                    body["max_tokens"] == json!(65_536)
                })
                .body_contains(r#""type":"tool_result""#)
                .body_contains(r#""tool_use_id":"toolu_read_fixture""#)
                .body_contains("fixture source");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({
                    "id": "final_response", "type": "message", "role": "assistant", "model": "model",
                    "content": [{"type": "tool_use", "id": "final_fixture", "name": "final_result",
                        "input": {"before": {}, "after": {"1": "Uses fixture source."}}}],
                    "stop_reason": "tool_use", "stop_sequence": null,
                    "usage": {"input_tokens": 2, "output_tokens": 2}
                }));
        })
        .await;
    let workspace = tempfile::tempdir().expect("fixture workspace should be created");
    std::fs::write(workspace.path().join("fixture.rs"), "fixture source\n")
        .expect("fixture file should be written");
    let adapter = AnthropicAdapter::new(AnthropicAdapterConfig::new(
        server.base_url(),
        Some("fixture-key".to_owned()),
        "model",
        CustomProviderEffort::Medium,
    ))
    .unwrap();
    let result = adapter
        .complete(AnthropicAdapterRequest::new(
            "inspect fixture",
            Vec::new(),
            RepositoryTools::new(workspace.path()).unwrap(),
            CancellationSignal::default(),
        ))
        .await
        .expect("tool loop should complete");
    first.assert_async().await;
    second.assert_async().await;
    assert_eq!(result.output["after"]["1"], "Uses fixture source.");
    let conversation = serde_json::to_string(&result.conversation).unwrap();
    assert!(conversation.contains("toolu_read_fixture"));
    assert!(conversation.contains("fixture source"));
}

#[tokio::test]
async fn adapter_keeps_an_empty_optional_key_and_maps_every_effort() {
    for effort in [
        CustomProviderEffort::Low,
        CustomProviderEffort::Medium,
        CustomProviderEffort::High,
    ] {
        let effort_text = super::super::CustomEffort::from(effort).as_str();
        let server = MockServer::start_async().await;
        let response = json!({
            "id": "msg_fixture", "type": "message", "role": "assistant", "model": "model",
            "content": [{"type": "tool_use", "id": "final", "name": "final_result",
                "input": {"before": {}, "after": {}}}],
            "stop_reason": "tool_use", "stop_sequence": null,
            "usage": {"input_tokens": 1, "output_tokens": 1}
        });
        let forbidden = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/messages")
                    .header_exists("x-api-key");
                then.status(401);
            })
            .await;
        let mock = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/messages")
                    .body_contains(&format!(r#""output_config":{{"effort":"{effort_text}"}}"#));
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
        assert_eq!(forbidden.hits_async().await, 0);
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
        CustomProviderEffort::Medium,
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
        CustomProviderEffort::High,
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

#[tokio::test]
async fn adapter_redacts_a_malformed_success_response() {
    let server = MockServer::start_async().await;
    let secret = "malformed-never-print-this-secret";
    server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/messages");
            then.status(200)
                .header("content-type", "application/json")
                .body(format!(r#"{{"content":"{secret}"}}"#));
        })
        .await;
    let workspace = tempfile::tempdir().expect("fixture workspace should be created");
    let adapter = AnthropicAdapter::new(AnthropicAdapterConfig::new(
        server.base_url(),
        Some("key".to_owned()),
        "model",
        CustomProviderEffort::Low,
    ))
    .unwrap();
    let error = adapter
        .complete(AnthropicAdapterRequest::new(
            "prompt",
            Vec::new(),
            RepositoryTools::new(workspace.path()).unwrap(),
            CancellationSignal::default(),
        ))
        .await
        .expect_err("malformed success response should fail");
    let diagnostic = format!("{error} {error:?}");
    assert!(!diagnostic.contains(secret));
    assert!(!diagnostic.contains(&server.base_url()));
}
