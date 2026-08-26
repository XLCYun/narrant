use super::super::*;
use crate::operation::TestCancellation;
use rig::completion::{FinishReason, ResponseIdentity, Usage};
use serde_json::json;
use std::sync::{Arc, Mutex};

/// 只在 runtime 契约测试中使用的可脚本化 provider-neutral adapter。
#[derive(Clone)]
struct FakeAdapter {
    connection: CustomConnectionSnapshot,
    responses: Arc<Mutex<Vec<CustomRoundResult>>>,
    requests: Arc<Mutex<Vec<CustomRoundRequest>>>,
    cancel_before_return: Option<TestCancellation>,
}

impl FakeAdapter {
    /// 按预设顺序返回 round response，同时记录每轮 history。
    fn scripted(responses: Vec<CustomRoundResult>) -> Self {
        Self {
            connection: CustomConnectionSnapshot::new(
                CustomProtocol::OpenaiChatCompletions,
                "https://provider.test/v1",
                "test-secret",
                "test-model",
                CustomEffort::Medium,
                "generation-1",
            )
            .expect("测试连接"),
            responses: Arc::new(Mutex::new(responses)),
            requests: Arc::new(Mutex::new(Vec::new())),
            cancel_before_return: None,
        }
    }

    /// 返回已记录的 provider-neutral round 请求。
    fn requests(&self) -> Vec<CustomRoundRequest> {
        self.requests.lock().expect("请求锁").clone()
    }
}

#[async_trait::async_trait]
impl CustomProtocolAdapter for FakeAdapter {
    /// 返回测试用的冻结连接。
    fn connection(&self) -> &CustomConnectionSnapshot {
        &self.connection
    }

    /// 从脚本取出 response；取消开关用于模拟 late response 边界。
    async fn complete(
        &self,
        request: CustomRoundRequest,
        cancellation: &crate::operation::CancellationSignal,
    ) -> Result<CustomRoundResult, CustomAdapterError> {
        self.requests.lock().expect("请求锁").push(request);
        if cancellation.is_cancelled() {
            return Err(CustomAdapterError::Cancelled);
        }
        if let Some(controller) = &self.cancel_before_return {
            controller.cancel();
        }
        let mut responses = self.responses.lock().expect("响应锁");
        if responses.is_empty() {
            Err(CustomAdapterError::TurnLimitExceeded {
                limit: MAX_MODEL_TURNS,
            })
        } else {
            Ok(responses.remove(0))
        }
    }
}

/// 构造 runtime 所需的最小 File Change request。
fn fixture_request(run_id: u64, workspace_path: &str) -> BabysitProviderRequest {
    BabysitProviderRequest {
        analysis_run_id: run_id,
        path: "src/example.rs".to_owned(),
        locale: "zh-CN".to_owned(),
        workspace_path: workspace_path.to_owned(),
        before: "old();".to_owned(),
        after: "new();".to_owned(),
        targets: vec![
            LineTarget {
                side: FileSide::Before,
                line_number: 1,
                source: "old();".to_owned(),
            },
            LineTarget {
                side: FileSide::After,
                line_number: 1,
                source: "new();".to_owned(),
            },
        ],
        model: "test-model".to_owned(),
        reasoning_effort: "medium".to_owned(),
    }
}

/// 构造一个带 assistant tool call 的 provider-neutral response。
fn tool_response(name: &str, arguments: serde_json::Value) -> CustomRoundResult {
    CustomRoundResult {
        choice: vec![rig::completion::AssistantContent::tool_call(
            "call-1", name, arguments,
        )],
        usage: Usage::new(),
        identity: ResponseIdentity {
            message_id: None,
            response_id: None,
            provider_request_id: None,
        },
        finish_reason: Some(FinishReason::ToolCalls),
    }
}

/// 冻结连接的缓存 identity 只包含非 secret 字段与 credential generation。
#[test]
fn frozen_connection_identity_excludes_key_but_includes_generation_and_connection_fields() {
    let first = CustomConnectionSnapshot::new(
        CustomProtocol::OpenaiChatCompletions,
        "https://model.example/v1",
        "secret-a",
        "review-model",
        CustomEffort::Medium,
        "credential-generation-1",
    )
    .expect("构造冻结连接");
    let same_connection_with_rotated_key = CustomConnectionSnapshot::new(
        CustomProtocol::OpenaiChatCompletions,
        "https://model.example/v1",
        "secret-b",
        "review-model",
        CustomEffort::Medium,
        "credential-generation-1",
    )
    .expect("构造冻结连接");
    let next_generation = CustomConnectionSnapshot::new(
        CustomProtocol::OpenaiChatCompletions,
        "https://model.example/v1",
        "secret-b",
        "review-model",
        CustomEffort::Medium,
        "credential-generation-2",
    )
    .expect("构造冻结连接");

    assert_eq!(
        first.cache_identity(),
        same_connection_with_rotated_key.cache_identity()
    );
    assert_ne!(first.cache_identity(), next_generation.cache_identity());
    assert!(!first.cache_identity().contains("secret-a"));
    assert!(!format!("{first:?}").contains("secret-a"));
}

#[tokio::test]
async fn runtime_replays_repository_tool_results_in_one_memory_session() {
    let workspace = tempfile::tempdir().expect("创建 workspace");
    std::fs::create_dir_all(workspace.path().join("src")).expect("创建目录");
    std::fs::write(workspace.path().join("src/example.rs"), "fn example() {}\n")
        .expect("写入 fixture");
    let adapter = FakeAdapter::scripted(vec![
        tool_response("read", json!({"path": "src/example.rs"})),
        tool_response(
            FINAL_OUTPUT_TOOL_NAME,
            json!({"before": {"1": "before explanation"}, "after": {"1": "after explanation"}}),
        ),
    ]);
    let requests = adapter.requests.clone();
    let runtime = CustomRuntime::new();
    let output = runtime
        .generate(
            Arc::new(adapter),
            fixture_request(41, &workspace.path().to_string_lossy()),
            &TestCancellation::default(),
        )
        .await
        .expect("runtime 输出");

    assert_eq!(
        output.before.get("1"),
        Some(&"before explanation".to_owned())
    );
    assert_eq!(output.after.get("1"), Some(&"after explanation".to_owned()));
    let requests = requests.lock().expect("请求锁");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].history().len(), 2);
}

#[tokio::test]
async fn runtime_rejects_late_response_after_cancellation_without_next_tool_or_final() {
    let workspace = tempfile::tempdir().expect("创建 workspace");
    let cancellation = TestCancellation::default();
    let mut adapter = FakeAdapter::scripted(vec![tool_response(
        FINAL_OUTPUT_TOOL_NAME,
        json!({"before": {}, "after": {}}),
    )]);
    adapter.cancel_before_return = Some(cancellation.clone());
    let requests = adapter.requests.clone();
    let runtime = CustomRuntime::new();
    let result = runtime
        .generate(
            Arc::new(adapter),
            fixture_request(42, &workspace.path().to_string_lossy()),
            &cancellation,
        )
        .await;

    assert_eq!(result, Err(BabysitError::Cancelled));
    assert_eq!(requests.lock().expect("请求锁").len(), 1);
}

#[tokio::test]
async fn runtime_stops_after_shared_sixteen_turn_budget() {
    let workspace = tempfile::tempdir().expect("创建 workspace");
    let adapter = FakeAdapter::scripted(
        (0..MAX_MODEL_TURNS)
            .map(|_| CustomRoundResult {
                choice: vec![rig::completion::AssistantContent::text("继续")],
                usage: Usage::new(),
                identity: ResponseIdentity {
                    message_id: None,
                    response_id: None,
                    provider_request_id: None,
                },
                finish_reason: Some(FinishReason::Length),
            })
            .collect(),
    );
    let requests = adapter.requests.clone();
    let runtime = CustomRuntime::new();
    let result = runtime
        .generate(
            Arc::new(adapter),
            fixture_request(43, &workspace.path().to_string_lossy()),
            &TestCancellation::default(),
        )
        .await
        .expect("预算耗尽仍返回上层可 repair 的默认结果");

    assert!(result.before.is_empty() && result.after.is_empty());
    assert_eq!(requests.lock().expect("请求锁").len(), MAX_MODEL_TURNS);
}

#[tokio::test]
async fn malformed_final_output_is_repaired_in_the_same_memory_session() {
    let workspace = tempfile::tempdir().expect("创建 workspace");
    let adapter = FakeAdapter::scripted(vec![
        tool_response(FINAL_OUTPUT_TOOL_NAME, json!({"unexpected": true})),
        tool_response(
            FINAL_OUTPUT_TOOL_NAME,
            json!({"before": {"1": "旧行"}, "after": {"1": "新行"}}),
        ),
    ]);
    let requests = adapter.requests.clone();
    let runtime = CustomRuntime::new();
    let request = fixture_request(44, &workspace.path().to_string_lossy());
    let malformed = runtime
        .generate(
            Arc::new(adapter),
            request.clone(),
            &TestCancellation::default(),
        )
        .await
        .expect("坏 final 交给上层校验");
    assert!(malformed.before.is_empty() && malformed.after.is_empty());

    let repaired = runtime
        .repair(
            request,
            malformed,
            vec![ValidationFailure::MissingLine {
                side: FileSide::Before,
                line_number: 1,
            }],
            &TestCancellation::default(),
        )
        .await
        .expect("同一会话 repair");

    assert_eq!(repaired.before.get("1"), Some(&"旧行".to_owned()));
    let requests = requests.lock().expect("请求锁");
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[1].history().len(), 1);
}

#[tokio::test]
async fn runtime_rejects_a_fourth_repair_attempt() {
    let workspace = tempfile::tempdir().expect("创建 workspace");
    let responses = (0..=MAX_REPAIR_ATTEMPTS)
        .map(|_| tool_response(FINAL_OUTPUT_TOOL_NAME, json!({"before": {}, "after": {}})))
        .collect();
    let runtime = CustomRuntime::new();
    let request = fixture_request(45, &workspace.path().to_string_lossy());
    let empty = runtime
        .generate(
            Arc::new(FakeAdapter::scripted(responses)),
            request.clone(),
            &TestCancellation::default(),
        )
        .await
        .expect("首轮结果");
    for _ in 0..MAX_REPAIR_ATTEMPTS {
        runtime
            .repair(
                request.clone(),
                empty.clone(),
                Vec::new(),
                &TestCancellation::default(),
            )
            .await
            .expect("前三次 repair");
    }
    let exhausted = runtime
        .repair(request, empty, Vec::new(), &TestCancellation::default())
        .await;
    assert!(matches!(
        exhausted,
        Err(BabysitError::RepairExhausted { .. })
    ));
}
