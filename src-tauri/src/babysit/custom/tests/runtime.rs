use super::super::*;
use crate::babysit::{
    BabysitError, BabysitProviderRequest, FileSide, LineTarget, ValidationFailure,
};
use crate::operation::TestCancellation;
use rig::completion::{Message, ResponseIdentity};
use rig::message::UserContent;
use serde_json::json;
use std::sync::{Arc, Mutex};

/// 只在 runtime 契约测试中使用的可脚本化 provider-neutral adapter。
#[derive(Clone)]
struct FakeAdapter {
    responses: Arc<Mutex<Vec<CustomRunResult>>>,
    requests: Arc<Mutex<Vec<CustomRunRequest>>>,
    cancel_before_return: Option<TestCancellation>,
}

impl FakeAdapter {
    /// 按预设顺序返回 agent run response，同时记录每次 run 的 history。
    fn scripted(responses: Vec<CustomRunResult>) -> Self {
        Self {
            responses: Arc::new(Mutex::new(responses)),
            requests: Arc::new(Mutex::new(Vec::new())),
            cancel_before_return: None,
        }
    }
}

#[async_trait::async_trait]
impl CustomProtocolAdapter for FakeAdapter {
    /// 从脚本取出 response；取消开关用于模拟 late response 边界。
    async fn run_agent(
        &self,
        request: CustomRunRequest,
        cancellation: &crate::operation::CancellationSignal,
    ) -> Result<CustomRunResult, CustomAdapterError> {
        self.requests.lock().expect("请求锁").push(request);
        if cancellation.is_cancelled() {
            return Err(CustomAdapterError::Cancelled);
        }
        if let Some(controller) = &self.cancel_before_return {
            controller.cancel();
        }
        let mut responses = self.responses.lock().expect("响应锁");
        if responses.is_empty() {
            Err(CustomAdapterError::InvalidConnection)
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
fn tool_response(name: &str, arguments: serde_json::Value) -> CustomRunResult {
    CustomRunResult {
        choice: vec![rig::completion::AssistantContent::tool_call(
            "call-1", name, arguments,
        )],
        identity: ResponseIdentity {
            message_id: None,
            response_id: None,
            provider_request_id: None,
        },
        transcript: None,
    }
}

/// 构造携带 Rig 当前 run transcript 的 response，模拟真实 adapter 返回值。
fn tool_response_with_transcript(
    name: &str,
    arguments: serde_json::Value,
    transcript: Vec<Message>,
) -> CustomRunResult {
    let mut response = tool_response(name, arguments);
    response.transcript = Some(transcript);
    response
}

/// 从测试捕获的 provider-neutral user message 中读取唯一文本块。
fn user_message_text(message: &Message) -> &str {
    let Message::User { content } = message else {
        panic!("预期 user message");
    };
    let [UserContent::Text(text)] = content.as_slice() else {
        panic!("预期唯一文本块");
    };
    &text.text
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

/// 冻结连接的 Debug 只保留可安全诊断的字段，不暴露 endpoint 或 secret。
#[test]
fn frozen_connection_debug_excludes_endpoint_fingerprint_and_secret() {
    let connection = CustomConnectionSnapshot::new(
        CustomProtocol::OpenaiChatCompletions,
        "https://secret.invalid/v1?token=secret-url-token",
        "secret-api-key",
        "review-model",
        CustomEffort::Medium,
        "credential-generation-1",
    )
    .expect("构造冻结连接");

    let debug = format!("{connection:?}");
    assert!(!debug.contains("hash"));
    assert!(!debug.contains("endpoint"));
    assert!(!debug.contains("https://secret.invalid"));
    assert!(!debug.contains("secret-url-token"));
    assert!(!debug.contains("secret-api-key"));
    assert!(debug.contains("api_key"));
    assert!(debug.contains("<redacted>"));
}

#[tokio::test]
async fn runtime_delegates_tool_dispatch_without_a_second_narrant_loop() {
    let workspace = tempfile::tempdir().expect("创建 workspace");
    std::fs::write(workspace.path().join("example.rs"), "fn example() {}\n").expect("写入 fixture");
    let adapter = FakeAdapter::scripted(vec![tool_response("read", json!({"path": "example.rs"}))]);
    let requests = adapter.requests.clone();
    let runtime = CustomRuntime::new();

    let output = runtime
        .generate(
            Arc::new(adapter),
            fixture_request(401, &workspace.path().to_string_lossy()),
            &TestCancellation::default(),
        )
        .await
        .expect("adapter 已经负责完整 Rig agent loop");

    assert!(output.before.is_empty() && output.after.is_empty());
    let requests = requests.lock().expect("请求锁");
    assert_eq!(requests.len(), 1);
    let prompt = user_message_text(requests[0].prompt());
    assert!(prompt.contains("Do not explore the repository by default"));
    assert!(prompt.contains("one specific unresolved symbol or behavior"));
    assert!(prompt.contains("do not repeat or broaden a query"));
    assert!(prompt.contains("one or two short sentences"));
    assert!(prompt.contains("at most 240 Unicode characters"));
}

#[tokio::test]
async fn runtime_preserves_rig_transcript_across_generation_and_repairs() {
    let workspace = tempfile::tempdir().expect("创建 workspace");
    let result = json!({"before": {"1": "旧行"}, "after": {"1": "新行"}});
    let generation_transcript = vec![
        Message::user("generation prompt"),
        Message::Assistant {
            id: Some("generation-assistant".to_owned()),
            content: vec![rig::completion::AssistantContent::text("generation result")],
        },
    ];
    let first_repair_transcript = vec![
        Message::user("repair prompt"),
        Message::Assistant {
            id: Some("repair-assistant".to_owned()),
            content: vec![rig::completion::AssistantContent::text("repair result")],
        },
    ];
    let second_repair_transcript = vec![
        Message::user("second repair prompt"),
        Message::Assistant {
            id: Some("second-repair-assistant".to_owned()),
            content: vec![rig::completion::AssistantContent::text(
                "second repair result",
            )],
        },
    ];
    let adapter = FakeAdapter::scripted(vec![
        tool_response_with_transcript(
            FINAL_OUTPUT_TOOL_NAME,
            result.clone(),
            generation_transcript.clone(),
        ),
        tool_response_with_transcript(
            FINAL_OUTPUT_TOOL_NAME,
            result.clone(),
            first_repair_transcript,
        ),
        tool_response_with_transcript(FINAL_OUTPUT_TOOL_NAME, result, second_repair_transcript),
    ]);
    let requests = adapter.requests.clone();
    let runtime = CustomRuntime::new();
    let request = fixture_request(402, &workspace.path().to_string_lossy());
    let output = runtime
        .generate(
            Arc::new(adapter),
            request.clone(),
            &TestCancellation::default(),
        )
        .await
        .expect("首轮结果");
    runtime
        .repair(
            request.clone(),
            output.clone(),
            Vec::new(),
            &TestCancellation::default(),
        )
        .await
        .expect("第一次 repair");
    runtime
        .repair(request, output, Vec::new(), &TestCancellation::default())
        .await
        .expect("第二次 repair");

    let requests = requests.lock().expect("请求锁");
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].history(), generation_transcript.as_slice());
    assert_eq!(requests[2].history().len(), 4);
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
async fn runtime_delegates_turn_budget_to_the_adapter() {
    let workspace = tempfile::tempdir().expect("创建 workspace");
    let adapter = FakeAdapter::scripted(vec![CustomRunResult {
        choice: vec![rig::completion::AssistantContent::text("继续")],
        identity: ResponseIdentity {
            message_id: None,
            response_id: None,
            provider_request_id: None,
        },
        transcript: None,
    }]);
    let requests = adapter.requests.clone();
    let runtime = CustomRuntime::new();
    let result = runtime
        .generate(
            Arc::new(adapter),
            fixture_request(43, &workspace.path().to_string_lossy()),
            &TestCancellation::default(),
        )
        .await
        .expect("adapter 已经拥有 Rig 的 turn budget");

    assert!(result.before.is_empty() && result.after.is_empty());
    assert_eq!(requests.lock().expect("请求锁").len(), 1);
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
    assert_eq!(requests[1].history().len(), 2);
    let repair_prompt = user_message_text(requests[1].prompt());
    assert!(repair_prompt.contains("zh-CN"));
    assert!(repair_prompt.contains("Do not explore the repository by default"));
    assert!(repair_prompt.contains("do not repeat or broaden a query"));
    assert!(repair_prompt.contains("preferably within 240 Unicode characters"));
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
