use super::super::*;
use crate::application_settings::{
    ApplicationSettingsApplication, BabysitProvider, CustomCredentialAdapter,
    CustomProviderConnection, CustomProviderEffort, CustomProviderProtocol,
};
use crate::babysit::{
    BabysitApplication, BabysitFileRequest, BabysitGenerationPolicy, BabysitProgress,
    BabysitProviderCapabilities, ReviewIdentity,
};
use crate::operation::OperationContext;
use crate::review::{FileChangeContent, FileChangeStatus, FileContent};
use httpmock::prelude::*;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::{Arc, Mutex};

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
    let credentials = Arc::new(MemoryCredentials::default());
    let settings =
        ApplicationSettingsApplication::with_credential_adapter(settings_path.into(), credentials);
    settings
        .update_custom_connection(CustomProviderConnection {
            protocol,
            base_url,
            api_key: String::new(),
            model: "fixture-model".to_owned(),
            effort: CustomProviderEffort::High,
        })
        .expect("保存 fixture Custom connection");
    settings
        .update_provider(BabysitProvider::Custom)
        .expect("选择 fixture Custom provider");
    let bundle = Arc::new(CustomCapabilityBundle::new(settings.clone()))
        as Arc<dyn BabysitProviderCapabilities>;
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
            locale: "zh-CN".to_owned(),
            workspace_path: workspace.to_string_lossy().into_owned(),
            detected_language: Some("rust".to_owned()),
            generation_policy: BabysitGenerationPolicy::ReuseCached,
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

/// 共享修复契约：坏的 final_result 必须在同一 provider 会话中完成 repair。
#[tokio::test]
async fn both_custom_protocols_repair_malformed_output_in_one_application_session() {
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
        let repair = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path(endpoint_path(protocol))
                    .matches(|request| {
                        let body = request
                            .body
                            .as_deref()
                            .map(String::from_utf8_lossy)
                            .unwrap_or_default();
                        body.contains("Repair the previous Line Explanation result")
                            && body.contains("before")
                    });
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
        assert_eq!(initial.hits_async().await, 1);
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
        let cancellation = crate::operation::TestCancellation::default();
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

/// 共享冻结快照契约：保存新连接只影响后续分析，in-flight 请求仍使用旧 endpoint。
#[tokio::test]
async fn both_custom_protocols_freeze_connection_while_a_request_is_in_flight() {
    for protocol in [
        CustomProviderProtocol::OpenaiChatCompletions,
        CustomProviderProtocol::AnthropicMessages,
    ] {
        let old_server = MockServer::start_async().await;
        let old_model = old_server
            .mock_async(|when, then| {
                when.method(POST).path(endpoint_path(protocol));
                then.status(200)
                    .delay(std::time::Duration::from_millis(200))
                    .header("content-type", "application/json")
                    .json_body(final_response(protocol));
            })
            .await;
        let new_server = MockServer::start_async().await;
        let new_model = new_server
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
            base_url(protocol, &old_server),
        );
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
                protocol,
                base_url: base_url(protocol, &new_server),
                api_key: String::new(),
                model: "next-model".to_owned(),
                effort: CustomProviderEffort::Low,
            })
            .expect("保存新的 fixture connection");

        first_task
            .await
            .expect("旧连接 task 应正常 join")
            .expect("旧连接 response 应成功");
        assert_eq!(old_model.hits_async().await, 1);

        let (request, content) = fixture_input(workspace.path());
        let next_cancellation = OperationContext::background().cancellation().clone();
        configured
            .application
            .explain_file(request, content, &next_cancellation, |_| Ok(()))
            .await
            .expect("新连接应服务后续分析");
        assert_eq!(new_model.hits_async().await, 1);
    }
}
