use super::super::*;
use crate::babysit::{
    BabysitAgentProvider, BabysitError, BabysitFileRequest, BabysitGenerationPolicy,
    BabysitProviderCapabilities, BabysitProviderModelCatalog, BabysitProviderSetupFailure,
    BabysitResolvedModel, PreparedBabysitAnalysis, RepositoryTools, ReviewIdentity,
};
use crate::operation::TestCancellation;
use httpmock::prelude::*;
use rig::completion::Message;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// 将冻结的真实 Custom provider 接入应用 registry 的测试 capability。
struct FrozenCustomCapability {
    provider: Arc<CustomProvider>,
    process_generation: Arc<AtomicUsize>,
}

/// 首次返回前触发取消、随后返回合法 final 的测试 adapter。
struct CancelOnceAdapter {
    connection: CustomConnectionSnapshot,
    calls: Arc<AtomicUsize>,
    first_cancellation: TestCancellation,
}

#[async_trait::async_trait]
impl CustomProtocolAdapter for CancelOnceAdapter {
    /// 返回测试冻结连接。
    fn connection(&self) -> &CustomConnectionSnapshot {
        &self.connection
    }

    /// 首次在返回 Read 调用前触发取消，第二次证明应用没有 dispatch 或复用取消结果。
    async fn complete(
        &self,
        _request: CustomRoundRequest,
        _cancellation: &crate::operation::CancellationSignal,
    ) -> Result<CustomRoundResult, CustomAdapterError> {
        let first_call = self.calls.fetch_add(1, Ordering::AcqRel) == 0;
        if first_call {
            self.first_cancellation.cancel();
        }
        Ok(CustomRoundResult {
            choice: vec![rig::completion::AssistantContent::tool_call(
                "final",
                if first_call {
                    "read"
                } else {
                    FINAL_OUTPUT_TOOL_NAME
                },
                if first_call {
                    json!({"path": "outside-workspace.rs"})
                } else {
                    json!({"before": {"1": "旧行"}, "after": {"1": "新行"}})
                },
            )],
            usage: rig::completion::Usage::new(),
            identity: rig::completion::ResponseIdentity {
                message_id: None,
                response_id: None,
                provider_request_id: None,
            },
            finish_reason: Some(rig::completion::FinishReason::ToolCalls),
            transcript: None,
        })
    }
}

#[async_trait::async_trait]
impl BabysitProviderCapabilities for FrozenCustomCapability {
    /// 返回测试创建时冻结的真实 Custom provider。
    async fn prepare_analysis(
        &self,
        _workspace_path: &std::path::Path,
        _preference: Option<&crate::application_settings::BabysitProviderPreference<'_>>,
    ) -> Result<PreparedBabysitAnalysis, BabysitError> {
        Ok(PreparedBabysitAnalysis {
            provider: self.provider.clone(),
            resolved: BabysitResolvedModel {
                model: "fixture-model".to_owned(),
                reasoning_effort: "medium".to_owned(),
            },
            process_generation: format!(
                "custom-fixture-{}",
                self.process_generation.load(Ordering::Acquire)
            ),
            _lease: None,
        })
    }

    /// 测试 capability 不暴露模型目录。
    async fn model_catalog(&self) -> Result<BabysitProviderModelCatalog, BabysitError> {
        Err(BabysitError::Provider {
            detail: "fixture catalog unavailable".to_owned(),
        })
    }

    /// Built-in provider 不接受 executable。
    async fn validate_executable(
        &self,
        _path: &std::path::Path,
    ) -> Result<std::path::PathBuf, BabysitError> {
        Err(BabysitError::provider_setup(
            PROVIDER_ID,
            BabysitProviderSetupFailure::NotFound,
        ))
    }

    /// 清空测试 runtime 会话。
    async fn shutdown(&self) {
        self.provider.shutdown().await;
    }
}

fn fixture_tools() -> Vec<rig::completion::ToolDefinition> {
    let workspace = tempfile::tempdir().expect("创建 fixture workspace");
    let tools = RepositoryTools::new(workspace.path()).expect("创建 repository tools");
    custom_tool_definitions(&tools)
}

#[test]
fn final_tool_is_schema_bearing_and_tool_set_is_bounded() {
    let tools = fixture_tools();
    assert_eq!(
        tools
            .iter()
            .map(|tool| tool.name.as_str())
            .collect::<Vec<_>>(),
        ["read", "glob", "grep", FINAL_OUTPUT_TOOL_NAME]
    );
    let final_tool = tools
        .iter()
        .find(|tool| tool.name == FINAL_OUTPUT_TOOL_NAME)
        .expect("final tool");
    assert_eq!(final_tool.parameters, line_explanation_schema().to_value());
    assert_eq!(final_tool.parameters["additionalProperties"], json!(false));
}

#[test]
fn round_request_rejects_turns_after_the_shared_budget() {
    let error = CustomRoundRequest::new(
        Message::user("review"),
        Vec::new(),
        MAX_MODEL_TURNS + 1,
        fixture_tools(),
    )
    .expect_err("超过预算的请求应在本地拒绝");
    assert_eq!(
        error,
        CustomAdapterError::TurnLimitExceeded {
            limit: MAX_MODEL_TURNS
        }
    );
}

#[tokio::test]
async fn openai_fixture_uses_rig_chat_path_model_effort_auth_and_final_schema() {
    let server = MockServer::start_async().await;
    let expected_tools = fixture_tools();
    let mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/chat/completions")
                .header("authorization", "Bearer fixture-key")
                .body_contains("\"name\":\"read\"")
                .body_contains("\"name\":\"glob\"")
                .body_contains("\"name\":\"grep\"")
                .body_contains("\"name\":\"final_result\"")
                .json_body_partial(
                    serde_json::to_string(&json!({
                        "model": "fixture-model",
                        "reasoning_effort": "high",
                    }))
                    .expect("序列化请求断言"),
                );
            then.status(200).json_body(json!({
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
            }));
        })
        .await;

    let connection = CustomConnectionSnapshot::new(
        CustomProtocol::OpenaiChatCompletions,
        format!("{}/v1", server.url("")),
        "fixture-key",
        "fixture-model",
        CustomEffort::High,
        "credential-1",
    )
    .expect("冻结连接");
    let workspace = tempfile::tempdir().expect("创建 workspace");
    let adapter =
        OpenAiChatCompletionsAdapter::new(connection, workspace.path()).expect("创建 adapter");
    let request = CustomRoundRequest::new(Message::user("review"), Vec::new(), 1, expected_tools)
        .expect("构造请求");
    let response = adapter
        .complete(request, &TestCancellation::default())
        .await
        .expect("fixture response");

    assert_eq!(response.tool_calls().len(), 1);
    assert_eq!(
        response.tool_calls()[0].function.name,
        FINAL_OUTPUT_TOOL_NAME
    );
    mock.assert_async().await;
}

#[tokio::test]
async fn empty_key_omits_the_authorization_header() {
    let server = MockServer::start_async().await;
    let forbidden = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/v1/chat/completions")
                .header_exists("authorization");
            then.status(401);
        })
        .await;
    let success = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(200).json_body(json!({
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
            }));
        })
        .await;
    let connection = CustomConnectionSnapshot::new(
        CustomProtocol::OpenaiChatCompletions,
        format!("{}/v1", server.url("")),
        "",
        "fixture-model",
        CustomEffort::Low,
        "credential-1",
    )
    .expect("冻结连接");
    let workspace = tempfile::tempdir().expect("创建 workspace");
    let adapter =
        OpenAiChatCompletionsAdapter::new(connection, workspace.path()).expect("创建 adapter");
    let request = CustomRoundRequest::new(Message::user("review"), Vec::new(), 1, fixture_tools())
        .expect("构造请求");

    adapter
        .complete(request, &TestCancellation::default())
        .await
        .expect("无认证请求");
    assert_eq!(forbidden.hits_async().await, 0);
    assert_eq!(success.hits_async().await, 1);
}

#[tokio::test]
async fn custom_registry_http_generation_populates_and_reuses_completed_cache() {
    use crate::operation::OperationContext;
    use crate::review::{FileChangeContent, FileChangeStatus, FileContent};

    let server = MockServer::start_async().await;
    let model = server
        .mock_async(|when, then| {
            when.method(POST).path("/v1/chat/completions");
            then.status(200).json_body(json!({
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
                                "arguments": "{\"before\":{\"1\":\"旧行\"},\"after\":{\"1\":\"新行\"}}"
                            }
                        }]
                    },
                    "finish_reason": "tool_calls"
                }],
                "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
            }));
        })
        .await;
    let workspace = tempfile::tempdir().expect("创建 workspace");
    let settings = tempfile::tempdir().expect("创建 settings 目录");
    let snapshot = CustomConnectionSnapshot::new(
        CustomProtocol::OpenaiChatCompletions,
        format!("{}/v1", server.url("")),
        "",
        "fixture-model",
        CustomEffort::Medium,
        "generation-1",
    )
    .expect("冻结连接");
    let generation = snapshot.cache_identity();
    let adapter = Arc::new(
        OpenAiChatCompletionsAdapter::new(snapshot, workspace.path()).expect("创建 adapter"),
    );
    let runtime = Arc::new(CustomRuntime::new());
    let provider = Arc::new(CustomProvider::new(runtime, adapter, generation.clone()));
    let process_generation = Arc::new(AtomicUsize::new(1));
    let capability = Arc::new(FrozenCustomCapability {
        provider,
        process_generation: process_generation.clone(),
    }) as Arc<dyn BabysitProviderCapabilities>;
    let application = crate::babysit::TestBabysitApplication::with_capabilities_at(
        settings.path().join("settings.json"),
        [(
            crate::application_settings::BabysitProvider::Custom,
            capability,
        )],
    );
    let request = BabysitFileRequest {
        identity: ReviewIdentity {
            review_id: "review".to_owned(),
            snapshot_id: "snapshot".to_owned(),
        },
        path: "src/example.rs".to_owned(),
        locale: "zh-CN".to_owned(),
        workspace_path: workspace.path().to_string_lossy().into_owned(),
        detected_language: Some("rust".to_owned()),
        generation_policy: BabysitGenerationPolicy::ReuseCached,
    };
    let content = FileChangeContent {
        path: request.path.clone(),
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
    };
    let cancellation = OperationContext::background().cancellation().clone();

    let first = application
        .explain_file(request.clone(), content.clone(), &cancellation, |_| Ok(()))
        .await
        .expect("首次 Custom 生成");
    let cached = application
        .explain_file(request, content, &cancellation, |_| Ok(()))
        .await
        .expect("复用 completed cache");
    process_generation.store(2, Ordering::Release);
    let isolated = application
        .explain_file(
            BabysitFileRequest {
                identity: ReviewIdentity {
                    review_id: "review".to_owned(),
                    snapshot_id: "snapshot".to_owned(),
                },
                path: "src/example.rs".to_owned(),
                locale: "zh-CN".to_owned(),
                workspace_path: workspace.path().to_string_lossy().into_owned(),
                detected_language: Some("rust".to_owned()),
                generation_policy: BabysitGenerationPolicy::ReuseCached,
            },
            FileChangeContent {
                path: "src/example.rs".to_owned(),
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
            &cancellation,
            |_| Ok(()),
        )
        .await
        .expect("连接 identity 变化后重新生成");

    assert_eq!(first, cached);
    assert_eq!(first, isolated);
    assert_eq!(model.hits_async().await, 2);
}

#[tokio::test]
async fn custom_registry_cancels_before_tool_dispatch_and_does_not_cache() {
    use crate::review::{FileChangeContent, FileChangeStatus, FileContent};

    let workspace = tempfile::tempdir().expect("创建 workspace");
    let settings = tempfile::tempdir().expect("创建 settings 目录");
    let first_cancellation = TestCancellation::default();
    let calls = Arc::new(AtomicUsize::new(0));
    let connection = CustomConnectionSnapshot::new(
        CustomProtocol::OpenaiChatCompletions,
        "https://provider.test/v1",
        "",
        "fixture-model",
        CustomEffort::Medium,
        "generation-1",
    )
    .expect("冻结连接");
    let adapter = Arc::new(CancelOnceAdapter {
        connection,
        calls: calls.clone(),
        first_cancellation: first_cancellation.clone(),
    });
    let provider = Arc::new(CustomProvider::new(
        Arc::new(CustomRuntime::new()),
        adapter,
        "custom-fixture-1".to_owned(),
    ));
    let capability = Arc::new(FrozenCustomCapability {
        provider,
        process_generation: Arc::new(AtomicUsize::new(1)),
    }) as Arc<dyn BabysitProviderCapabilities>;
    let application = crate::babysit::TestBabysitApplication::with_capabilities_at(
        settings.path().join("settings.json"),
        [(
            crate::application_settings::BabysitProvider::Custom,
            capability,
        )],
    );
    let request = BabysitFileRequest {
        identity: ReviewIdentity {
            review_id: "cancel-review".to_owned(),
            snapshot_id: "snapshot".to_owned(),
        },
        path: "src/example.rs".to_owned(),
        locale: "zh-CN".to_owned(),
        workspace_path: workspace.path().to_string_lossy().into_owned(),
        detected_language: Some("rust".to_owned()),
        generation_policy: BabysitGenerationPolicy::ReuseCached,
    };
    let content = FileChangeContent {
        path: request.path.clone(),
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
    };

    let cancelled = application
        .explain_file(
            request.clone(),
            content.clone(),
            &first_cancellation,
            |_| Ok(()),
        )
        .await;
    assert_eq!(cancelled, Err(BabysitError::Cancelled));
    application
        .explain_file(request, content, &TestCancellation::default(), |_| Ok(()))
        .await
        .expect("取消结果未缓存，第二次重新生成");
    assert_eq!(calls.load(Ordering::Acquire), 2);
}
