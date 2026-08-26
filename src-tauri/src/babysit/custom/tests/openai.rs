use super::super::*;
use crate::operation::TestCancellation;
use httpmock::prelude::*;
use rig::completion::Message;
use serde_json::json;

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
                .json_body_partial(
                    serde_json::to_string(&json!({
                        "model": "fixture-model",
                        "reasoning_effort": "high",
                        "tools": expected_tools.iter().map(|tool| json!({
                            "type": "function",
                            "function": {
                                "name": tool.name,
                                "description": tool.description,
                                "parameters": tool.parameters,
                            }
                        })).collect::<Vec<_>>(),
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
    let adapter = OpenAiChatCompletionsAdapter::new(connection).expect("创建 adapter");
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
