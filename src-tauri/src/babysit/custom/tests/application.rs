use super::super::*;
use crate::application_settings::{
    ApplicationSettingsApplication, BabysitProvider, CustomCredentialAdapter,
    CustomProviderConnection, CustomProviderEffort, CustomProviderProtocol,
};
use crate::babysit::{
    BabysitApplication, BabysitError, BabysitErrorPayload, BabysitFileRequest,
    BabysitGenerationPolicy, BabysitProgress, BabysitProviderCapabilities, ReviewIdentity,
};
use crate::operation::OperationContext;
use crate::review::{FileChangeContent, FileChangeStatus, FileContent};
use httpmock::prelude::*;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// 仅在应用级 Custom 契约中保存凭据，避免测试读取真实系统钥匙串。
#[derive(Default)]
struct MemoryCredentials(Mutex<Option<String>>);

impl CustomCredentialAdapter for MemoryCredentials {
    /// 返回当前 fixture 凭据。
    fn load(&self) -> Result<Option<String>, String> {
        self.0
            .lock()
            .map(|value| value.clone())
            .map_err(|_| "fixture credential unavailable".to_owned())
    }

    /// 保存当前 fixture 凭据。
    fn save(&self, value: &str) -> Result<(), String> {
        *self
            .0
            .lock()
            .map_err(|_| "fixture credential unavailable".to_owned())? = Some(value.to_owned());
        Ok(())
    }

    /// 删除当前 fixture 凭据。
    fn delete(&self) -> Result<(), String> {
        *self
            .0
            .lock()
            .map_err(|_| "fixture credential unavailable".to_owned())? = None;
        Ok(())
    }
}

/// 构造真正的 Custom capability registry，不绕过 settings 或 provider selection。
struct ConfiguredApplication {
    application: Arc<BabysitApplication>,
    settings: ApplicationSettingsApplication,
}

/// 创建测试用的 settings 与真实 Custom capability，供后续测试修改连接快照。
fn application_for_connection(
    settings_path: impl Into<std::path::PathBuf>,
    protocol: CustomProviderProtocol,
    base_url: String,
) -> ConfiguredApplication {
    application_for_connection_with_options(settings_path, protocol, base_url, String::new(), None)
}

/// 构造带指定 API key 的真实 Custom capability，供 application 脱敏契约使用。
fn application_for_connection_with_api_key(
    settings_path: impl Into<std::path::PathBuf>,
    protocol: CustomProviderProtocol,
    base_url: String,
    api_key: &str,
) -> ConfiguredApplication {
    application_for_connection_with_options(
        settings_path,
        protocol,
        base_url,
        api_key.to_owned(),
        None,
    )
}

/// 构造可选 repository-tool 边界取消的真实 Custom capability。
fn application_for_connection_with_tool_cancellation(
    settings_path: impl Into<std::path::PathBuf>,
    protocol: CustomProviderProtocol,
    base_url: String,
    tool_cancellation: Option<CancellationToken>,
) -> ConfiguredApplication {
    application_for_connection_with_options(
        settings_path,
        protocol,
        base_url,
        String::new(),
        tool_cancellation,
    )
}

/// 统一构造测试设置、凭据边界、Custom registry 与可选 tool 取消 seam。
fn application_for_connection_with_options(
    settings_path: impl Into<std::path::PathBuf>,
    protocol: CustomProviderProtocol,
    base_url: String,
    api_key: String,
    tool_cancellation: Option<CancellationToken>,
) -> ConfiguredApplication {
    let credentials = Arc::new(MemoryCredentials::default());
    let settings =
        ApplicationSettingsApplication::with_credential_adapter(settings_path.into(), credentials);
    settings
        .update_custom_connection(CustomProviderConnection {
            protocol,
            base_url,
            api_key,
            model: "fixture-model".to_owned(),
            effort: CustomProviderEffort::High,
        })
        .expect("保存 fixture Custom connection");
    settings
        .update_provider(BabysitProvider::Custom)
        .expect("选择 fixture Custom provider");
    settings
        .update_repository_exploration(true)
        .expect("既有 Custom application fixture 显式开启 repository exploration");
    let bundle = match tool_cancellation {
        Some(cancellation) => Arc::new(CustomCapabilityBundle::with_tool_boundary_cancellation(
            settings.clone(),
            cancellation,
        )) as Arc<dyn BabysitProviderCapabilities>,
        None => Arc::new(CustomCapabilityBundle::new(settings.clone()))
            as Arc<dyn BabysitProviderCapabilities>,
    };
    ConfiguredApplication {
        application: Arc::new(BabysitApplication::from_registry(
            [(BabysitProvider::Custom, bundle)].into_iter().collect(),
            Some(settings.clone()),
        )),
        settings,
    }
}

/// 构造完整 before/after File Change 输入，验证应用不会按单行拆分请求。
fn fixture_input(workspace: &Path) -> (BabysitFileRequest, FileChangeContent) {
    let path = "src/example.rs".to_owned();
    (
        BabysitFileRequest {
            identity: ReviewIdentity {
                review_id: "review".to_owned(),
                snapshot_id: "snapshot".to_owned(),
            },
            path: path.clone(),
            workspace_path: workspace.to_string_lossy().into_owned(),
            detected_language: Some("rust".to_owned()),
            generation_policy: BabysitGenerationPolicy::ReuseCached,
            changed_ranges: vec![
                crate::babysit::ChangedLineRange {
                    side: crate::babysit::FileSide::Before,
                    start_line_number: 1,
                    end_line_number: 1,
                },
                crate::babysit::ChangedLineRange {
                    side: crate::babysit::FileSide::After,
                    start_line_number: 1,
                    end_line_number: 1,
                },
            ],
        },
        FileChangeContent {
            path,
            old_path: None,
            status: FileChangeStatus::Modified,
            before: Some(FileContent::Text {
                text: "old();".to_owned(),
                size: 6,
            }),
            after: Some(FileContent::Text {
                text: "new();".to_owned(),
                size: 6,
            }),
        },
    )
}

/// 返回两个协议都能被 Rig Tool output mode 接受的 final_result response。
fn final_response(protocol: CustomProviderProtocol) -> Value {
    match protocol {
        CustomProviderProtocol::OpenaiChatCompletions => json!({
            "id": "fixture-response",
            "object": "chat.completion",
            "created": 1,
            "model": "fixture-model",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call-final",
                        "type": "function",
                        "function": {
                            "name": FINAL_OUTPUT_TOOL_NAME,
                            "arguments": "{\"before\":{\"1\":\"before explanation\"},\"after\":{\"1\":\"after explanation\"}}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        }),
        CustomProviderProtocol::AnthropicMessages => json!({
            "id": "fixture-response",
            "type": "message",
            "role": "assistant",
            "model": "fixture-model",
            "content": [{
                "type": "tool_use",
                "id": "call-final",
                "name": FINAL_OUTPUT_TOOL_NAME,
                "input": {
                    "before": {"1": "before explanation"},
                    "after": {"1": "after explanation"}
                }
            }],
            "stop_reason": "tool_use",
            "stop_sequence": null,
            "usage": {"input_tokens": 1, "output_tokens": 1}
        }),
    }
}

/// 返回达到固定输出上限的协议 response，验证应用层不把截断交给 repair。
fn output_limit_response(protocol: CustomProviderProtocol) -> Value {
    let mut response = final_response(protocol);
    match protocol {
        CustomProviderProtocol::OpenaiChatCompletions => {
            response["choices"][0]["finish_reason"] = json!("length");
        }
        CustomProviderProtocol::AnthropicMessages => {
            response["stop_reason"] = json!("max_tokens");
        }
    }
    response
}

/// 为缓存隔离契约返回可区分来源的完整 final_result。
fn response_with_explanations(
    protocol: CustomProviderProtocol,
    before: &str,
    after: &str,
) -> Value {
    let mut response = final_response(protocol);
    let output = json!({"before": {"1": before}, "after": {"1": after}});
    match protocol {
        CustomProviderProtocol::OpenaiChatCompletions => {
            response["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"] =
                Value::String(serde_json::to_string(&output).expect("序列化 fixture output"));
        }
        CustomProviderProtocol::AnthropicMessages => {
            response["content"][0]["input"] = output;
        }
    }
    response
}

/// 根据协议构造 schema 合法但缺少目标行的输出，让应用层 validation 进入同一会话 repair。
fn malformed_response(protocol: CustomProviderProtocol) -> Value {
    match protocol {
        CustomProviderProtocol::OpenaiChatCompletions => json!({
            "id": "fixture-response",
            "object": "chat.completion",
            "created": 1,
            "model": "fixture-model",
            "choices": [{
                "index": 0,
                "message": {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call-final",
                        "type": "function",
                        "function": {
                            "name": FINAL_OUTPUT_TOOL_NAME,
                            "arguments": "{\"before\":{},\"after\":{}}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        }),
        CustomProviderProtocol::AnthropicMessages => json!({
            "id": "fixture-response",
            "type": "message",
            "role": "assistant",
            "model": "fixture-model",
            "content": [{
                "type": "tool_use",
                "id": "call-final",
                "name": FINAL_OUTPUT_TOOL_NAME,
                "input": {"before": {}, "after": {}}
            }],
            "stop_reason": "tool_use",
            "stop_sequence": null,
            "usage": {"input_tokens": 1, "output_tokens": 1}
        }),
    }
}

/// 返回要求读取 fixture.rs 的协议级工具调用。
fn read_tool_response(protocol: CustomProviderProtocol) -> Value {
    match protocol {
        CustomProviderProtocol::OpenaiChatCompletions => json!({
            "id": "fixture-read", "object": "chat.completion", "created": 1,
            "model": "fixture-model", "choices": [{"index": 0, "message": {
                "role": "assistant", "tool_calls": [{"id": "call-read", "type": "function",
                    "function": {"name": "read", "arguments": "{\"path\":\"fixture.rs\",\"offset\":0}"}}]
            }, "finish_reason": "tool_calls"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
        }),
        CustomProviderProtocol::AnthropicMessages => json!({
            "id": "fixture-read", "type": "message", "role": "assistant",
            "model": "fixture-model", "content": [{"type": "tool_use", "id": "call-read",
                "name": "read", "input": {"path": "fixture.rs", "offset": 0}}],
            "stop_reason": "tool_use", "stop_sequence": null,
            "usage": {"input_tokens": 1, "output_tokens": 1}
        }),
    }
}

/// 区分首轮请求与已经携带 repository tool result 的后续请求。
fn omits_fixture_repository_result(request: &HttpMockRequest) -> bool {
    request
        .body
        .as_deref()
        .is_some_and(|body| !String::from_utf8_lossy(body).contains("fixture repository context"))
}

/// 解析真实模型请求并确认 Custom 只注册 Read、Glob、Grep 与 final_result。
fn has_exact_custom_tools(request: &HttpMockRequest) -> bool {
    let Some(body) = request.body.as_deref() else {
        return false;
    };
    let Ok(body) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    let Some(tools) = body["tools"].as_array() else {
        return false;
    };
    let names = tools
        .iter()
        .filter_map(|tool| {
            tool["name"]
                .as_str()
                .or_else(|| tool["function"]["name"].as_str())
        })
        .collect::<std::collections::BTreeSet<_>>();
    tools.len() == 4
        && names == std::collections::BTreeSet::from(["final_result", "glob", "grep", "read"])
}

/// 解析 repair wire history，确认首轮 assistant 的 Read 调用与 tool-result 都被保留。
fn has_read_tool_round_trip(protocol: CustomProviderProtocol, request: &HttpMockRequest) -> bool {
    let Some(body) = request.body.as_deref() else {
        return false;
    };
    let Ok(body) = serde_json::from_slice::<Value>(body) else {
        return false;
    };
    let Some(messages) = body["messages"].as_array() else {
        return false;
    };

    match protocol {
        CustomProviderProtocol::OpenaiChatCompletions => {
            let assistant_call = messages.iter().any(|message| {
                message["role"] == "assistant"
                    && message["tool_calls"].as_array().is_some_and(|calls| {
                        calls.iter().any(|call| {
                            call["id"] == "call-read" && call["function"]["name"] == "read"
                        })
                    })
            });
            let tool_result = messages.iter().any(|message| {
                message["role"] == "tool"
                    && message["tool_call_id"] == "call-read"
                    && message["content"]
                        .as_str()
                        .is_some_and(|content| content.contains("fixture repository context"))
            });
            assistant_call && tool_result
        }
        CustomProviderProtocol::AnthropicMessages => {
            let assistant_call = messages.iter().any(|message| {
                message["role"] == "assistant"
                    && message["content"].as_array().is_some_and(|content| {
                        content.iter().any(|part| {
                            part["type"] == "tool_use"
                                && part["id"] == "call-read"
                                && part["name"] == "read"
                        })
                    })
            });
            let tool_result = messages.iter().any(|message| {
                message["role"] == "user"
                    && message["content"].as_array().is_some_and(|content| {
                        content.iter().any(|part| {
                            part["type"] == "tool_result"
                                && part["tool_use_id"] == "call-read"
                                && part.to_string().contains("fixture repository context")
                        })
                    })
            });
            assistant_call && tool_result
        }
    }
}

/// 提供无捕获的 OpenAI history matcher，满足 httpmock 的函数指针 seam。
fn has_openai_read_tool_round_trip(request: &HttpMockRequest) -> bool {
    has_read_tool_round_trip(CustomProviderProtocol::OpenaiChatCompletions, request)
}

/// 提供无捕获的 Anthropic history matcher，满足 httpmock 的函数指针 seam。
fn has_anthropic_read_tool_round_trip(request: &HttpMockRequest) -> bool {
    has_read_tool_round_trip(CustomProviderProtocol::AnthropicMessages, request)
}

/// 为指定协议计算 Rig 使用的 completion endpoint。
fn endpoint_path(protocol: CustomProviderProtocol) -> &'static str {
    match protocol {
        CustomProviderProtocol::OpenaiChatCompletions => "/v1/chat/completions",
        CustomProviderProtocol::AnthropicMessages => "/v1/messages",
    }
}

/// 将本地 fixture server URL 投影为协议需要的 base URL。
fn base_url(protocol: CustomProviderProtocol, server: &MockServer) -> String {
    match protocol {
        CustomProviderProtocol::OpenaiChatCompletions => format!("{}/v1", server.url("")),
        CustomProviderProtocol::AnthropicMessages => server.base_url(),
    }
}

/// 共享应用契约：两个显式协议都必须经 CustomCapabilityBundle 完成生成并只缓存完成结果。
#[tokio::test]
async fn both_custom_protocols_run_through_registry_and_completed_cache() {
    for protocol in [
        CustomProviderProtocol::OpenaiChatCompletions,
        CustomProviderProtocol::AnthropicMessages,
    ] {
        let server = MockServer::start_async().await;
        let model = server
            .mock_async(|when, then| {
                when.method(POST).path(endpoint_path(protocol));
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(final_response(protocol));
            })
            .await;
        let workspace = tempfile::tempdir().expect("创建 fixture workspace");
        let settings = tempfile::tempdir().expect("创建 fixture settings");
        let configured = application_for_connection(
            settings.path().join("settings.json"),
            protocol,
            base_url(protocol, &server),
        );
        let (request, content) = fixture_input(workspace.path());
        let cancellation = OperationContext::background().cancellation().clone();
        let mut progress = Vec::new();

        let first = configured
            .application
            .explain_file(request.clone(), content.clone(), &cancellation, |event| {
                progress.push(event);
                Ok(())
            })
            .await
            .expect("Custom fixture should complete");
        let cached = configured
            .application
            .explain_file(request, content, &cancellation, |event| {
                progress.push(event);
                Ok(())
            })
            .await
            .expect("completed Custom fixture should be reusable");

        assert_eq!(first, cached);
        assert_eq!(
            first.before.get("1"),
            Some(&"before explanation".to_owned())
        );
        assert_eq!(first.after.get("1"), Some(&"after explanation".to_owned()));
        assert_eq!(model.hits_async().await, 1);
        assert_eq!(
            progress,
            vec![
                BabysitProgress::Generating,
                BabysitProgress::Completed,
                BabysitProgress::Completed,
            ]
        );
    }
}

/// 共享输出上限契约：两个协议都返回 distinct OutputLimit，跳过 repair 且失败不缓存。
#[tokio::test]
async fn both_custom_protocols_surface_output_limit_without_repair_or_cache() {
    for protocol in [
        CustomProviderProtocol::OpenaiChatCompletions,
        CustomProviderProtocol::AnthropicMessages,
    ] {
        let server = MockServer::start_async().await;
        let model = server
            .mock_async(|when, then| {
                when.method(POST).path(endpoint_path(protocol));
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(output_limit_response(protocol));
            })
            .await;
        let workspace = tempfile::tempdir().expect("创建 fixture workspace");
        let settings = tempfile::tempdir().expect("创建 fixture settings");
        let configured = application_for_connection(
            settings.path().join("settings.json"),
            protocol,
            base_url(protocol, &server),
        );
        let (request, content) = fixture_input(workspace.path());
        let cancellation = OperationContext::background().cancellation().clone();
        let mut first_progress = Vec::new();

        let first = configured
            .application
            .explain_file(request.clone(), content.clone(), &cancellation, |event| {
                first_progress.push(event);
                Ok(())
            })
            .await;

        assert_eq!(
            first,
            Err(BabysitError::OutputLimit {
                limit: MAX_OUTPUT_TOKENS,
            })
        );
        assert_eq!(first_progress, vec![BabysitProgress::Generating]);

        // ReuseCached 仍须重新发起请求，证明 output-limit failure 没有进入 completed cache。
        let mut retry_progress = Vec::new();
        let retry = configured
            .application
            .explain_file(request, content, &cancellation, |event| {
                retry_progress.push(event);
                Ok(())
            })
            .await;
        assert_eq!(
            retry,
            Err(BabysitError::OutputLimit {
                limit: MAX_OUTPUT_TOKENS,
            })
        );
        assert_eq!(retry_progress, vec![BabysitProgress::Generating]);
        assert_eq!(model.hits_async().await, 2);
    }
}

/// 跨协议切换不能复用先前协议的 completed result，即使请求 identity 完全相同。
#[tokio::test]
async fn changing_custom_protocol_does_not_reuse_a_completed_result() {
    let server = MockServer::start_async().await;
    let openai = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(response_with_explanations(
                    CustomProviderProtocol::OpenaiChatCompletions,
                    "openai before",
                    "openai after",
                ));
        })
        .await;
    let anthropic = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/messages");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(response_with_explanations(
                    CustomProviderProtocol::AnthropicMessages,
                    "anthropic before",
                    "anthropic after",
                ));
        })
        .await;
    let workspace = tempfile::tempdir().expect("创建 fixture workspace");
    let settings = tempfile::tempdir().expect("创建 fixture settings");
    let configured = application_for_connection(
        settings.path().join("settings.json"),
        CustomProviderProtocol::OpenaiChatCompletions,
        format!("{}/v1", server.url("")),
    );
    let (request, content) = fixture_input(workspace.path());
    let cancellation = OperationContext::background().cancellation().clone();

    let openai_result = configured
        .application
        .explain_file(request.clone(), content.clone(), &cancellation, |_| Ok(()))
        .await
        .expect("OpenAI fixture should complete");

    configured
        .settings
        .update_custom_connection(CustomProviderConnection {
            protocol: CustomProviderProtocol::AnthropicMessages,
            // Anthropic 会规范化末尾的 `/v1`，而 OpenAI 会保留它；相同 base URL 可让断言聚焦协议 identity。
            base_url: format!("{}/v1", server.url("")),
            api_key: String::new(),
            model: "fixture-model".to_owned(),
            effort: CustomProviderEffort::High,
        })
        .expect("保存 Anthropic fixture connection");

    let anthropic_result = configured
        .application
        .explain_file(request, content, &cancellation, |_| Ok(()))
        .await
        .expect("Anthropic fixture should complete");

    assert_eq!(
        openai_result.before.get("1"),
        Some(&"openai before".to_owned())
    );
    assert_eq!(
        anthropic_result.before.get("1"),
        Some(&"anthropic before".to_owned())
    );
    assert_eq!(openai.hits_async().await, 1);
    assert_eq!(anthropic.hits_async().await, 1);
}

/// application 层 transport 失败必须完全移除 URL、secret、内容与旧 origin/hash 诊断。
#[tokio::test]
async fn both_custom_protocols_redact_transport_diagnostics_at_application_boundary() {
    const API_KEY_MARKER: &str = "application-api-key-secret";
    const REQUEST_MARKER: &str = "application-request-body-secret";
    const RESPONSE_MARKER: &str = "application-response-body-secret";
    const PROMPT_MARKER: &str = "application-prompt-secret";
    const SOURCE_MARKER: &str = "application-source-secret";
    const EXPLANATION_MARKER: &str = "application-explanation-secret";
    const TOOL_RESULT_MARKER: &str = "application-tool-result-secret";
    const ORIGIN_HASH_MARKER: &str = "application-origin-hash-secret";

    for protocol in [
        CustomProviderProtocol::OpenaiChatCompletions,
        CustomProviderProtocol::AnthropicMessages,
    ] {
        let server = MockServer::start_async().await;
        let endpoint_marker = format!("{ORIGIN_HASH_MARKER}-{protocol:?}");
        let expected_path = match protocol {
            CustomProviderProtocol::OpenaiChatCompletions => {
                format!("/v1/{endpoint_marker}/chat/completions")
            }
            CustomProviderProtocol::AnthropicMessages => {
                format!("/{endpoint_marker}/v1/messages")
            }
        };
        let response_body = format!(
            "{RESPONSE_MARKER} {REQUEST_MARKER} {PROMPT_MARKER} {SOURCE_MARKER} \
             {EXPLANATION_MARKER} {TOOL_RESULT_MARKER}"
        );
        let failed = server
            .mock_async(|when, then| {
                when.method(POST).path(expected_path);
                then.status(500)
                    .header("content-type", "application/json")
                    .json_body(json!({
                        "error": {
                            "type": "fixture_failure",
                            "message": response_body,
                        }
                    }));
            })
            .await;
        let base_url = match protocol {
            CustomProviderProtocol::OpenaiChatCompletions => {
                format!("{}/v1/{endpoint_marker}", server.url(""))
            }
            CustomProviderProtocol::AnthropicMessages => {
                format!("{}/{endpoint_marker}", server.base_url())
            }
        };
        let workspace = tempfile::tempdir().expect("创建 fixture workspace");
        let settings = tempfile::tempdir().expect("创建 fixture settings");
        let configured = application_for_connection_with_api_key(
            settings.path().join("settings.json"),
            protocol,
            base_url,
            API_KEY_MARKER,
        );
        let (mut request, mut content) = fixture_input(workspace.path());
        request.path = format!("{}-{PROMPT_MARKER}", request.path);
        content.path = request.path.clone();
        if let Some(FileContent::Text { text, .. }) = content.before.as_mut() {
            *text = SOURCE_MARKER.to_owned();
        }
        let cancellation = OperationContext::background().cancellation().clone();

        let error = configured
            .application
            .explain_file(request, content, &cancellation, |_| Ok(()))
            .await
            .expect_err("fixture transport failure should reach application boundary");
        let payload = BabysitErrorPayload::from(error.clone());
        let diagnostic = format!(
            "{error} {error:?} {}",
            serde_json::to_string(&payload).expect("序列化脱敏 application payload")
        );
        for forbidden in [
            endpoint_marker,
            API_KEY_MARKER.to_owned(),
            REQUEST_MARKER.to_owned(),
            RESPONSE_MARKER.to_owned(),
            PROMPT_MARKER.to_owned(),
            SOURCE_MARKER.to_owned(),
            EXPLANATION_MARKER.to_owned(),
            TOOL_RESULT_MARKER.to_owned(),
            ORIGIN_HASH_MARKER.to_owned(),
        ] {
            assert!(
                !diagnostic.contains(&forbidden),
                "application diagnostic leaked marker {forbidden}: {diagnostic}"
            );
        }
        let diagnostic_lower = diagnostic.to_ascii_lowercase();
        assert!(!diagnostic_lower.contains("origin"));
        assert!(!diagnostic_lower.contains("hash"));
        assert!(matches!(error, BabysitError::ProviderDiagnostic { .. }));
        failed.assert_async().await;
    }
}

/// 共享 wire 契约：registry 选出的两个协议都执行真实 Read 循环且只暴露四个工具。
#[tokio::test]
async fn both_custom_protocols_preserve_wire_contract_through_repository_tool_loop() {
    for protocol in [
        CustomProviderProtocol::OpenaiChatCompletions,
        CustomProviderProtocol::AnthropicMessages,
    ] {
        let server = MockServer::start_async().await;
        let authentication_header = match protocol {
            CustomProviderProtocol::OpenaiChatCompletions => "authorization",
            CustomProviderProtocol::AnthropicMessages => "x-api-key",
        };
        let forbidden_auth = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(endpoint_path(protocol))
                    .header_exists(authentication_header);
                then.status(401);
            })
            .await;
        let read = server
            .mock_async(|when, then| {
                let when = when
                    .method(POST)
                    .path(endpoint_path(protocol))
                    .matches(has_exact_custom_tools)
                    .body_contains(r#""name":"read""#)
                    .body_contains(r#""name":"glob""#)
                    .body_contains(r#""name":"grep""#)
                    .body_contains(r#""name":"final_result""#)
                    .matches(omits_fixture_repository_result);
                match protocol {
                    CustomProviderProtocol::OpenaiChatCompletions => {
                        when.body_contains(r#""reasoning_effort":"high""#);
                    }
                    CustomProviderProtocol::AnthropicMessages => {
                        when.body_contains(r#""thinking":{"type":"adaptive"}"#)
                            .body_contains(r#""output_config":{"effort":"high"}"#);
                    }
                }
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(read_tool_response(protocol));
            })
            .await;
        let final_result = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(endpoint_path(protocol))
                    .body_contains("fixture repository context");
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(final_response(protocol));
            })
            .await;
        let workspace = tempfile::tempdir().expect("创建 fixture workspace");
        std::fs::write(
            workspace.path().join("fixture.rs"),
            "fixture repository context\n",
        )
        .expect("写入 repository fixture");
        let settings = tempfile::tempdir().expect("创建 fixture settings");
        let configured = application_for_connection(
            settings.path().join("settings.json"),
            protocol,
            base_url(protocol, &server),
        );
        let (request, content) = fixture_input(workspace.path());
        let cancellation = OperationContext::background().cancellation().clone();

        let result = configured
            .application
            .explain_file(request, content, &cancellation, |_| Ok(()))
            .await;

        assert_eq!(forbidden_auth.hits_async().await, 0);
        assert_eq!(read.hits_async().await, 1);
        assert_eq!(final_result.hits_async().await, 1);
        result.expect("repository tool loop 应完成");
    }
}

/// 共享工具取消契约：在 Read execute 内取消后不得继续模型请求或写 completed cache。
#[tokio::test]
async fn both_custom_protocols_cancel_inside_repository_tool_without_cache() {
    for protocol in [
        CustomProviderProtocol::OpenaiChatCompletions,
        CustomProviderProtocol::AnthropicMessages,
    ] {
        let server = MockServer::start_async().await;
        let read = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(endpoint_path(protocol))
                    .matches(omits_fixture_repository_result);
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(read_tool_response(protocol));
            })
            .await;
        let final_result = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(endpoint_path(protocol))
                    .body_contains("fixture repository context");
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(final_response(protocol));
            })
            .await;
        let workspace = tempfile::tempdir().expect("创建 fixture workspace");
        std::fs::write(
            workspace.path().join("fixture.rs"),
            "fixture repository context\n",
        )
        .expect("写入 repository fixture");
        let settings = tempfile::tempdir().expect("创建 fixture settings");
        let cancellation = CancellationToken::new();
        let configured = application_for_connection_with_tool_cancellation(
            settings.path().join("settings.json"),
            protocol,
            base_url(protocol, &server),
            Some(cancellation.clone()),
        );
        let (request, content) = fixture_input(workspace.path());

        let cancelled = configured
            .application
            .explain_file(request.clone(), content.clone(), &cancellation, |_| Ok(()))
            .await;
        assert_eq!(cancelled, Err(crate::babysit::BabysitError::Cancelled));
        assert_eq!(read.hits_async().await, 1);
        assert_eq!(final_result.hits_async().await, 0);

        let retry_cancellation = OperationContext::background().cancellation().clone();
        configured
            .application
            .explain_file(request, content, &retry_cancellation, |_| Ok(()))
            .await
            .expect("tool 边界取消不应缓存，使用新 signal 的重试应完成");
        assert_eq!(read.hits_async().await, 2);
        assert_eq!(final_result.hits_async().await, 1);
    }
}

/// 共享修复契约：坏的 final_result 必须在同一 provider 会话中完成 repair。
#[tokio::test]
async fn both_custom_protocols_repair_malformed_output_in_one_application_session() {
    for protocol in [
        CustomProviderProtocol::OpenaiChatCompletions,
        CustomProviderProtocol::AnthropicMessages,
    ] {
        let server = MockServer::start_async().await;
        let initial_read = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(endpoint_path(protocol))
                    .matches(|request| {
                        let body = request
                            .body
                            .as_deref()
                            .map(String::from_utf8_lossy)
                            .unwrap_or_default();
                        body.contains("Review this complete file change")
                            && !body.contains("Repair the previous Line Explanation result")
                            && !body.contains("fixture repository context")
                    });
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(read_tool_response(protocol));
            })
            .await;
        let initial_final = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(endpoint_path(protocol))
                    .matches(|request| {
                        let body = request
                            .body
                            .as_deref()
                            .map(String::from_utf8_lossy)
                            .unwrap_or_default();
                        body.contains("Review this complete file change")
                            && body.contains("fixture repository context")
                            && !body.contains("Repair the previous Line Explanation result")
                    });
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(malformed_response(protocol));
            })
            .await;
        let repair = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(endpoint_path(protocol))
                    .body_contains("Repair the previous Line Explanation result")
                    .body_contains("before")
                    .matches(match protocol {
                        CustomProviderProtocol::OpenaiChatCompletions => {
                            has_openai_read_tool_round_trip
                        }
                        CustomProviderProtocol::AnthropicMessages => {
                            has_anthropic_read_tool_round_trip
                        }
                    });
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(final_response(protocol));
            })
            .await;
        let workspace = tempfile::tempdir().expect("创建 fixture workspace");
        std::fs::write(
            workspace.path().join("fixture.rs"),
            "fixture repository context\n",
        )
        .expect("写入 repository fixture");
        let settings = tempfile::tempdir().expect("创建 fixture settings");
        let configured = application_for_connection(
            settings.path().join("settings.json"),
            protocol,
            base_url(protocol, &server),
        );
        let (request, content) = fixture_input(workspace.path());
        let cancellation = OperationContext::background().cancellation().clone();
        let mut progress = Vec::new();

        let output = configured
            .application
            .explain_file(request, content, &cancellation, |event| {
                progress.push(event);
                Ok(())
            })
            .await
            .expect("malformed Custom output should be repaired");

        assert_eq!(
            output.before.get("1"),
            Some(&"before explanation".to_owned())
        );
        assert_eq!(output.after.get("1"), Some(&"after explanation".to_owned()));
        assert_eq!(initial_read.hits_async().await, 1);
        assert_eq!(initial_final.hits_async().await, 1);
        assert_eq!(repair.hits_async().await, 1);
        assert_eq!(
            progress,
            vec![
                BabysitProgress::Generating,
                BabysitProgress::Repairing { attempt: 1 },
                BabysitProgress::Completed,
            ]
        );
    }
}

/// 共享修复上限契约：三次 repair 后必须失败且绝不能把不完整结果写入缓存。
#[tokio::test]
async fn both_custom_protocols_exhaust_repairs_without_completed_cache() {
    for protocol in [
        CustomProviderProtocol::OpenaiChatCompletions,
        CustomProviderProtocol::AnthropicMessages,
    ] {
        let server = MockServer::start_async().await;
        let initial = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(endpoint_path(protocol))
                    .matches(|request| {
                        let body = request
                            .body
                            .as_deref()
                            .map(String::from_utf8_lossy)
                            .unwrap_or_default();
                        body.contains("Review this complete file change")
                            && !body.contains("Repair the previous Line Explanation result")
                    });
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(malformed_response(protocol));
            })
            .await;
        let repairs = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(endpoint_path(protocol))
                    .matches(|request| {
                        request
                            .body
                            .as_deref()
                            .map(String::from_utf8_lossy)
                            .is_some_and(|body| {
                                body.contains("Repair the previous Line Explanation result")
                            })
                    });
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(malformed_response(protocol));
            })
            .await;
        let workspace = tempfile::tempdir().expect("创建 fixture workspace");
        let settings = tempfile::tempdir().expect("创建 fixture settings");
        let configured = application_for_connection(
            settings.path().join("settings.json"),
            protocol,
            base_url(protocol, &server),
        );
        let (request, content) = fixture_input(workspace.path());
        let cancellation = OperationContext::background().cancellation().clone();

        let error = configured
            .application
            .explain_file(request.clone(), content.clone(), &cancellation, |_| Ok(()))
            .await
            .expect_err("超过 repair 上限后应失败");
        assert!(matches!(
            error,
            crate::babysit::BabysitError::RepairExhausted { .. }
        ));
        assert_eq!(initial.hits_async().await, 1);
        assert_eq!(repairs.hits_async().await, 3);

        // 同一 identity 再次调用仍须发起模型请求，证明失败结果没有进入 completed cache。
        let _ = configured
            .application
            .explain_file(request, content, &cancellation, |_| Ok(()))
            .await
            .expect_err("重复请求仍应在 repair 上限失败");
        assert_eq!(initial.hits_async().await, 2);
        assert_eq!(repairs.hits_async().await, 6);
    }
}

/// 共享 repair 取消契约：Repairing 进度边界取消后不发下一轮请求也不写缓存。
#[tokio::test]
async fn both_custom_protocols_cancel_before_repair_dispatch_without_cache() {
    for protocol in [
        CustomProviderProtocol::OpenaiChatCompletions,
        CustomProviderProtocol::AnthropicMessages,
    ] {
        let server = MockServer::start_async().await;
        let initial = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(endpoint_path(protocol))
                    .matches(|request| {
                        request.body.as_deref().is_some_and(|body| {
                            !String::from_utf8_lossy(body)
                                .contains("Repair the previous Line Explanation result")
                        })
                    });
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(malformed_response(protocol));
            })
            .await;
        let repair = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(endpoint_path(protocol))
                    .body_contains("Repair the previous Line Explanation result");
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(final_response(protocol));
            })
            .await;
        let workspace = tempfile::tempdir().expect("创建 fixture workspace");
        let settings = tempfile::tempdir().expect("创建 fixture settings");
        let configured = application_for_connection(
            settings.path().join("settings.json"),
            protocol,
            base_url(protocol, &server),
        );
        let (request, content) = fixture_input(workspace.path());
        let cancellation = CancellationToken::new();
        let cancellation_at_repair = cancellation.clone();

        let cancelled = configured
            .application
            .explain_file(
                request.clone(),
                content.clone(),
                &cancellation,
                |progress| {
                    if matches!(progress, BabysitProgress::Repairing { attempt: 1 }) {
                        cancellation_at_repair.cancel();
                    }
                    Ok(())
                },
            )
            .await;
        assert_eq!(cancelled, Err(crate::babysit::BabysitError::Cancelled));
        assert_eq!(initial.hits_async().await, 1);
        assert_eq!(repair.hits_async().await, 0);

        let retry_cancellation = OperationContext::background().cancellation().clone();
        configured
            .application
            .explain_file(request, content, &retry_cancellation, |_| Ok(()))
            .await
            .expect("取消的 repair 不应缓存，重试应完成");
        assert_eq!(initial.hits_async().await, 2);
        assert_eq!(repair.hits_async().await, 1);
    }
}

/// 共享取消契约：取消中的 HTTP late response 不能完成、继续本地流程或写入缓存。
#[tokio::test]
async fn both_custom_protocols_reject_late_responses_and_regenerate_after_cancellation() {
    for protocol in [
        CustomProviderProtocol::OpenaiChatCompletions,
        CustomProviderProtocol::AnthropicMessages,
    ] {
        let server = MockServer::start_async().await;
        let late = server
            .mock_async(|when, then| {
                when.method(POST).path(endpoint_path(protocol));
                then.status(200)
                    .delay(std::time::Duration::from_secs(2))
                    .header("content-type", "application/json")
                    .json_body(final_response(protocol));
            })
            .await;
        let workspace = tempfile::tempdir().expect("创建 fixture workspace");
        let settings = tempfile::tempdir().expect("创建 fixture settings");
        let configured = application_for_connection(
            settings.path().join("settings.json"),
            protocol,
            base_url(protocol, &server),
        );
        let (request, content) = fixture_input(workspace.path());
        let cancellation = CancellationToken::new();
        let task = {
            let application = configured.application.clone();
            let request = request.clone();
            let content = content.clone();
            let cancellation = cancellation.clone();
            // 仅把不可变应用引用与测试取消信号带入 task，不复制或记录 fixture 内容。
            tokio::spawn(async move {
                application
                    .explain_file(request, content, &cancellation, |_| Ok(()))
                    .await
            })
        };
        for _ in 0..100 {
            if late.hits_async().await == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(late.hits_async().await, 1);
        cancellation.cancel();
        let result = tokio::time::timeout(std::time::Duration::from_secs(1), task)
            .await
            .expect("取消应在 late response 前结束")
            .expect("取消 task 应正常 join");
        assert_eq!(result, Err(crate::babysit::BabysitError::Cancelled));

        // 移除延迟响应后用一次成功请求证明取消结果未进入 completed cache。
        late.delete_async().await;
        let success = server
            .mock_async(|when, then| {
                when.method(POST).path(endpoint_path(protocol));
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(final_response(protocol));
            })
            .await;
        let (_, content) = fixture_input(workspace.path());
        let retry_cancellation = OperationContext::background().cancellation().clone();
        configured
            .application
            .explain_file(
                fixture_input(workspace.path()).0,
                content,
                &retry_cancellation,
                |_| Ok(()),
            )
            .await
            .expect("取消后应允许重新生成");
        assert_eq!(success.hits_async().await, 1);
    }
}

/// 共享冻结快照契约：in-flight wire 请求使用旧完整连接，后续 run 使用新完整连接。
#[tokio::test]
async fn both_custom_protocols_freeze_connection_while_a_request_is_in_flight() {
    for (old_protocol, new_protocol) in [
        (
            CustomProviderProtocol::OpenaiChatCompletions,
            CustomProviderProtocol::AnthropicMessages,
        ),
        (
            CustomProviderProtocol::AnthropicMessages,
            CustomProviderProtocol::OpenaiChatCompletions,
        ),
    ] {
        let old_server = MockServer::start_async().await;
        let old_model = old_server
            .mock_async(|when, then| {
                match old_protocol {
                    CustomProviderProtocol::OpenaiChatCompletions => when
                        .method(POST)
                        .path(endpoint_path(old_protocol))
                        .body_contains(r#""model":"fixture-model""#)
                        .header("authorization", "Bearer old-key")
                        .body_contains(r#""reasoning_effort":"high""#),
                    CustomProviderProtocol::AnthropicMessages => when
                        .method(POST)
                        .path(endpoint_path(old_protocol))
                        .body_contains(r#""model":"fixture-model""#)
                        .header("x-api-key", "old-key")
                        .body_contains(r#""output_config":{"effort":"high"}"#),
                };
                then.status(200)
                    .delay(std::time::Duration::from_millis(200))
                    .header("content-type", "application/json")
                    .json_body(response_with_explanations(
                        old_protocol,
                        "old before",
                        "old after",
                    ));
            })
            .await;
        let new_server = MockServer::start_async().await;
        let new_model = new_server
            .mock_async(|when, then| {
                match new_protocol {
                    CustomProviderProtocol::OpenaiChatCompletions => when
                        .method(POST)
                        .path(endpoint_path(new_protocol))
                        .body_contains(r#""model":"next-model""#)
                        .header("authorization", "Bearer new-key")
                        .body_contains(r#""reasoning_effort":"low""#),
                    CustomProviderProtocol::AnthropicMessages => when
                        .method(POST)
                        .path(endpoint_path(new_protocol))
                        .body_contains(r#""model":"next-model""#)
                        .header("x-api-key", "new-key")
                        .body_contains(r#""output_config":{"effort":"low"}"#),
                };
                then.status(200)
                    .header("content-type", "application/json")
                    .json_body(response_with_explanations(
                        new_protocol,
                        "new before",
                        "new after",
                    ));
            })
            .await;
        let workspace = tempfile::tempdir().expect("创建 fixture workspace");
        let settings = tempfile::tempdir().expect("创建 fixture settings");
        let configured = application_for_connection_with_api_key(
            settings.path().join("settings.json"),
            old_protocol,
            base_url(old_protocol, &old_server),
            "old-key",
        );
        let (_, old_generation) = configured
            .settings
            .load_custom_connection_with_generation()
            .expect("读取旧连接 generation");
        let (request, content) = fixture_input(workspace.path());
        let first_cancellation = OperationContext::background().cancellation().clone();
        let first_task = {
            let application = configured.application.clone();
            let request = request.clone();
            let content = content.clone();
            let cancellation = first_cancellation.clone();
            tokio::spawn(async move {
                application
                    .explain_file(request, content, &cancellation, |_| Ok(()))
                    .await
            })
        };
        for _ in 0..100 {
            if old_model.hits_async().await == 1 {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        assert_eq!(old_model.hits_async().await, 1);

        configured
            .settings
            .update_custom_connection(CustomProviderConnection {
                protocol: new_protocol,
                base_url: base_url(new_protocol, &new_server),
                api_key: "new-key".to_owned(),
                model: "next-model".to_owned(),
                effort: CustomProviderEffort::Low,
            })
            .expect("保存新的 fixture connection");
        let (_, new_generation) = configured
            .settings
            .load_custom_connection_with_generation()
            .expect("读取新连接 generation");
        assert_eq!(new_generation, old_generation.saturating_add(1));

        let first_output = first_task
            .await
            .expect("旧连接 task 应正常 join")
            .expect("旧连接 response 应成功");
        assert_eq!(first_output.before.get("1"), Some(&"old before".to_owned()));
        assert_eq!(old_model.hits_async().await, 1);

        let (request, content) = fixture_input(workspace.path());
        let next_cancellation = OperationContext::background().cancellation().clone();
        let next_output = configured
            .application
            .explain_file(request, content, &next_cancellation, |_| Ok(()))
            .await
            .expect("新连接应服务后续分析");
        assert_eq!(next_output.before.get("1"), Some(&"new before".to_owned()));
        assert_eq!(new_model.hits_async().await, 1);
    }
}
