use super::*;
use crate::operation::OperationContext;
use crate::review::{FileChangeContent, FileChangeStatus, FileContent};
use std::sync::Arc;
use tokio::sync::Notify;

#[test]
fn model_preference_save_validation_uses_the_catalog_from_the_current_edit_session() {
    let application =
        TestBabysitApplication::with_provider(Arc::new(FakeBabysitProvider::default()));
    application
        .remember_model_catalog(
            crate::application_settings::BabysitProvider::Codex,
            "catalog-1".to_owned(),
            codex::CodexModelCatalog {
                models: vec![codex::CodexModelCapability {
                    id: "catalog-entry".to_owned(),
                    model: "gpt-runtime-model".to_owned(),
                    name: "Catalog Model".to_owned(),
                    is_default: true,
                    hidden: false,
                    supported_reasoning_efforts: vec!["low".to_owned()],
                    default_reasoning_effort: "low".to_owned(),
                }],
            },
        )
        .unwrap();

    assert!(application
        .validate_model_preference(
            crate::application_settings::BabysitProvider::Codex,
            "catalog-1",
            &crate::application_settings::BabysitModelPreference::Explicit {
                model: "catalog-entry".to_owned(),
                reasoning_effort: Some("low".to_owned()),
            }
        )
        .is_ok());
    application
        .remember_model_catalog(
            crate::application_settings::BabysitProvider::Codex,
            "catalog-2".to_owned(),
            codex::CodexModelCatalog {
                models: vec![codex::CodexModelCapability {
                    id: "catalog-entry".to_owned(),
                    model: "gpt-runtime-model".to_owned(),
                    name: "Catalog Model".to_owned(),
                    is_default: true,
                    hidden: false,
                    supported_reasoning_efforts: vec!["low".to_owned()],
                    default_reasoning_effort: "low".to_owned(),
                }],
            },
        )
        .unwrap();
    assert!(application
        .validate_model_preference(
            crate::application_settings::BabysitProvider::Codex,
            "catalog-1",
            &crate::application_settings::BabysitModelPreference::Automatic,
        )
        .is_ok());
    assert!(application
        .validate_model_preference(
            crate::application_settings::BabysitProvider::Codex,
            "catalog-missing",
            &crate::application_settings::BabysitModelPreference::Automatic,
        )
        .is_err());
}

#[test]
fn model_catalog_revision_is_bound_to_its_provider() {
    let application =
        TestBabysitApplication::with_provider(Arc::new(FakeBabysitProvider::default()));
    application
        .remember_model_catalog(
            crate::application_settings::BabysitProvider::ClaudeCode,
            "shared-revision".to_owned(),
            codex::CodexModelCatalog { models: Vec::new() },
        )
        .unwrap();

    assert!(application
        .validate_model_preference(
            crate::application_settings::BabysitProvider::Codex,
            "shared-revision",
            &crate::application_settings::BabysitModelPreference::Automatic,
        )
        .is_err());
}

#[tokio::test]
async fn executable_invalidation_discards_catalog_and_analysis_cache() {
    let provider = Arc::new(FakeBabysitProvider::default());
    let application = TestBabysitApplication::with_provider(provider.clone());
    application
        .remember_model_catalog(
            crate::application_settings::BabysitProvider::Codex,
            "catalog-1".to_owned(),
            codex::CodexModelCatalog { models: Vec::new() },
        )
        .unwrap();
    application.cache.lock().unwrap().insert(
        CacheKey {
            review_id: "review".to_owned(),
            snapshot_id: "snapshot".to_owned(),
            path: "src/lib.rs".to_owned(),
            locale: "zh-CN".to_owned(),
            detected_language: Some("rust".to_owned()),
            model: "model".to_owned(),
            reasoning_effort: "low".to_owned(),
            contract_version: ANALYSIS_CONTRACT_VERSION.to_owned(),
            provider_id: "provider".to_owned(),
            process_generation: "generation".to_owned(),
        },
        BabysitFileExplanations::default(),
    );

    application.invalidate_provider_preferences();

    assert!(application.model_catalogs.lock().unwrap().is_empty());
    assert!(application.cache.lock().unwrap().is_empty());
    assert_eq!(provider.shutdown_calls(), 0);
}

/// 在 generate 中暂停的 provider，用于观察偏好变化不会改写已解析请求。
struct InFlightModelProvider {
    resolved: Mutex<BabysitResolvedModel>,
    observed: Mutex<Vec<(String, String)>>,
    started: Notify,
    release: Notify,
}

#[async_trait]
impl BabysitAgentProvider for InFlightModelProvider {
    /// 返回并发测试使用的稳定进程 generation。
    fn process_generation(&self) -> String {
        "generation-1".to_owned()
    }
    /// 记录分析收到的模型组合，并暂停到测试显式释放。
    async fn generate(
        &self,
        request: BabysitProviderRequest,
        _cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        self.observed
            .lock()
            .unwrap()
            .push((request.model.clone(), request.reasoning_effort.clone()));
        self.started.notify_one();
        self.release.notified().await;
        Ok(FakeBabysitProvider::complete_output(&request))
    }
    /// 完整 fixture 输出不应触发 repair。
    async fn repair(
        &self,
        _request: BabysitProviderRequest,
        _previous: BabysitFileExplanations,
        _failures: Vec<ValidationFailure>,
        _cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        unreachable!("完整输出不会进入 repair")
    }
}

#[async_trait]
impl TestBabysitAgentProvider for InFlightModelProvider {
    /// 在每次新分析开始时快照当前解析模型。
    async fn resolve_test_model(&self) -> Result<BabysitResolvedModel, BabysitError> {
        Ok(self.resolved.lock().unwrap().clone())
    }
}

#[path = "claude.rs"]
mod claude;
#[path = "provider_registry.rs"]
mod provider_registry;

/// 构造只包含文本内容的 File Change fixture。
fn text_change(path: &str, before: Option<&str>, after: Option<&str>) -> FileChangeContent {
    fn content(value: Option<&str>) -> Option<FileContent> {
        value.map(|text| FileContent::Text {
            size: text.len(),
            text: text.to_owned(),
        })
    }
    FileChangeContent {
        path: path.to_owned(),
        old_path: None,
        status: FileChangeStatus::Modified,
        before: content(before),
        after: content(after),
    }
}

/// 构造一个固定 Review identity，证明 reviewId 与 snapshotId 都参与缓存隔离。
fn request(review_id: &str, snapshot_id: &str, path: &str) -> BabysitFileRequest {
    request_with_locale(review_id, snapshot_id, path, "zh-CN")
}

/// 构造指定 locale 的 analysis request，用于验证缓存不会复用另一种语言。
fn request_with_locale(
    review_id: &str,
    snapshot_id: &str,
    path: &str,
    locale: &str,
) -> BabysitFileRequest {
    BabysitFileRequest {
        identity: ReviewIdentity {
            review_id: review_id.to_owned(),
            snapshot_id: snapshot_id.to_owned(),
        },
        path: path.to_owned(),
        locale: locale.to_owned(),
        workspace_path: "/tmp/narrant-test-workspace".to_owned(),
        detected_language: None,
        generation_policy: BabysitGenerationPolicy::ReuseCached,
    }
}

#[tokio::test]
async fn fake_provider_generates_all_non_empty_lines_in_one_batch() {
    let provider = Arc::new(FakeBabysitProvider::default());
    let application = TestBabysitApplication::with_provider(provider.clone());
    let mut progress = Vec::new();

    let result = application
        .explain_file(
            request("review-1", "snapshot-1", "src/example.ts"),
            text_change(
                "src/example.ts",
                Some("const old = 1;\n\nreturn old;\n"),
                Some("const next = 2;\nreturn next;\n"),
            ),
            &OperationContext::background().cancellation().clone(),
            |event| {
                progress.push(event);
                Ok(())
            },
        )
        .await
        .unwrap();

    assert_eq!(provider.generate_calls(), 1);
    assert_eq!(provider.repair_calls(), 0);
    assert_eq!(
        progress,
        vec![BabysitProgress::Generating, BabysitProgress::Completed]
    );
    assert_eq!(result.before.len(), 2);
    assert_eq!(result.after.len(), 2);
    assert!(result.before.contains_key("1"));
    assert!(result.before.contains_key("3"));
    assert!(result.after.contains_key("1"));
    assert!(result.after.contains_key("2"));
}

#[tokio::test]
async fn cache_reuses_only_the_exact_review_snapshot_identity() {
    let provider = Arc::new(FakeBabysitProvider::default());
    let application = TestBabysitApplication::with_provider(provider.clone());
    let cancellation = OperationContext::background().cancellation().clone();
    let content = text_change(
        "src/example.ts",
        Some("const value = 1;"),
        Some("const value = 2;"),
    );

    application
        .explain_file(
            request("review-1", "snapshot-1", "src/example.ts"),
            content.clone(),
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();
    application
        .explain_file(
            request("review-1", "snapshot-1", "src/example.ts"),
            content.clone(),
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();
    application
        .explain_file(
            request("review-1", "snapshot-2", "src/example.ts"),
            content,
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(provider.generate_calls(), 2);
}

#[tokio::test]
async fn cache_isolated_by_resolved_model_and_reasoning_effort() {
    let provider = Arc::new(FakeBabysitProvider::default());
    let application = TestBabysitApplication::with_provider(provider.clone());
    let cancellation = OperationContext::background().cancellation().clone();
    let content = text_change(
        "src/example.ts",
        Some("const value = 1;"),
        Some("const value = 2;"),
    );

    application
        .explain_file(
            request("review-1", "snapshot-1", "src/example.ts"),
            content.clone(),
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();
    provider.set_resolved_model("test-model", "high");
    application
        .explain_file(
            request("review-1", "snapshot-1", "src/example.ts"),
            content.clone(),
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();
    provider.set_resolved_model("another-model", "high");
    application
        .explain_file(
            request("review-1", "snapshot-1", "src/example.ts"),
            content,
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(provider.generate_calls(), 3);
}

#[tokio::test]
async fn preference_change_during_generate_keeps_the_active_resolved_model() {
    let provider = Arc::new(InFlightModelProvider {
        resolved: Mutex::new(BabysitResolvedModel {
            model: "model-before".to_owned(),
            reasoning_effort: "low".to_owned(),
        }),
        observed: Mutex::new(Vec::new()),
        started: Notify::new(),
        release: Notify::new(),
    });
    let application = Arc::new(TestBabysitApplication::with_provider(provider.clone()));
    let task = tokio::spawn({
        let application = application.clone();
        async move {
            application
                .explain_file(
                    request("review-1", "snapshot-1", "src/example.ts"),
                    text_change("src/example.ts", Some("old();"), Some("next();")),
                    &OperationContext::background().cancellation().clone(),
                    |_| Ok(()),
                )
                .await
        }
    });
    provider.started.notified().await;
    *provider.resolved.lock().unwrap() = BabysitResolvedModel {
        model: "model-after".to_owned(),
        reasoning_effort: "high".to_owned(),
    };
    provider.release.notify_one();

    task.await.unwrap().unwrap();
    assert_eq!(
        provider.observed.lock().unwrap().as_slice(),
        &[("model-before".to_owned(), "low".to_owned())]
    );
}

#[test]
fn settings_persistence_failure_has_stable_structured_payload() {
    let payload = BabysitErrorPayload::from(BabysitError::SettingsPersistenceFailed);

    assert_eq!(payload.code, BabysitErrorCode::SettingsPersistenceFailed);
    assert_eq!(payload.detail, "宝宝模式模型设置保存失败；原设置保持不变");
    assert_eq!(payload.recovery, None);
}

#[tokio::test]
async fn regeneration_bypasses_and_replaces_the_exact_cached_result() {
    let provider = Arc::new(FakeBabysitProvider::default());
    let application = TestBabysitApplication::with_provider(provider.clone());
    let cancellation = OperationContext::background().cancellation().clone();
    let content = text_change(
        "src/example.ts",
        Some("const value = 1;"),
        Some("const value = 2;"),
    );

    application
        .explain_file(
            request("review-1", "snapshot-1", "src/example.ts"),
            content.clone(),
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();
    let mut regeneration = request("review-1", "snapshot-1", "src/example.ts");
    regeneration.generation_policy = BabysitGenerationPolicy::Regenerate;
    application
        .explain_file(regeneration, content.clone(), &cancellation, |_| Ok(()))
        .await
        .unwrap();
    application
        .explain_file(
            request("review-1", "snapshot-1", "src/example.ts"),
            content,
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(provider.generate_calls(), 2);
}

#[tokio::test]
async fn cache_does_not_reuse_an_explanation_from_another_locale() {
    let provider = Arc::new(FakeBabysitProvider::default());
    let application = TestBabysitApplication::with_provider(provider.clone());
    let cancellation = OperationContext::background().cancellation().clone();
    let content = text_change(
        "src/example.ts",
        Some("const value = 1;"),
        Some("const value = 2;"),
    );

    let chinese = application
        .explain_file(
            request_with_locale("review-1", "snapshot-1", "src/example.ts", "zh-CN"),
            content.clone(),
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();
    let english = application
        .explain_file(
            request_with_locale("review-1", "snapshot-1", "src/example.ts", "en"),
            content,
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(provider.generate_calls(), 2);
    assert_ne!(chinese.after.get("1"), english.after.get("1"));
    assert!(english.after["1"].starts_with("Explains"));
}

#[tokio::test]
async fn clearing_snapshot_and_review_cache_forces_only_the_requested_regeneration() {
    let provider = Arc::new(FakeBabysitProvider::default());
    let application = TestBabysitApplication::with_provider(provider.clone());
    let cancellation = OperationContext::background().cancellation().clone();
    let content = text_change(
        "src/example.ts",
        Some("const value = 1;"),
        Some("const value = 2;"),
    );

    application
        .explain_file(
            request("review-1", "snapshot-1", "src/example.ts"),
            content.clone(),
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();
    application
        .explain_file(
            request("review-1", "snapshot-2", "src/example.ts"),
            content.clone(),
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();

    application
        .clear_snapshot("review-1", "snapshot-1")
        .unwrap();
    application
        .explain_file(
            request("review-1", "snapshot-1", "src/example.ts"),
            content.clone(),
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(provider.generate_calls(), 3);

    application.clear_review("review-1").unwrap();
    application
        .explain_file(
            request("review-1", "snapshot-2", "src/example.ts"),
            content,
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(provider.generate_calls(), 4);
}

#[tokio::test]
async fn shutdown_clears_cache_and_notifies_the_provider_once() {
    let provider = Arc::new(FakeBabysitProvider::default());
    let application = TestBabysitApplication::with_provider(provider.clone());
    let cancellation = OperationContext::background().cancellation().clone();
    let content = text_change(
        "src/example.ts",
        Some("const value = 1;"),
        Some("const value = 2;"),
    );

    application
        .explain_file(
            request("review-1", "snapshot-1", "src/example.ts"),
            content.clone(),
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();
    application.shutdown().await.unwrap();
    application
        .explain_file(
            request("review-1", "snapshot-1", "src/example.ts"),
            content,
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(provider.generate_calls(), 2);
    assert_eq!(provider.shutdown_calls(), 1);
}

#[tokio::test]
async fn invalid_output_is_repaired_in_the_same_analysis_run() {
    let provider = Arc::new(FakeBabysitProvider::with_invalid_rounds(1));
    let application = TestBabysitApplication::with_provider(provider.clone());
    let mut progress = Vec::new();
    let cancellation = OperationContext::background().cancellation().clone();

    application
        .explain_file(
            request("review-1", "snapshot-1", "src/example.ts"),
            text_change("src/example.ts", None, Some("const value = 1;")),
            &cancellation,
            |event| {
                progress.push(event);
                Ok(())
            },
        )
        .await
        .unwrap();

    assert_eq!(provider.generate_calls(), 1);
    assert_eq!(provider.repair_calls(), 1);
    assert_eq!(
        progress,
        vec![
            BabysitProgress::Generating,
            BabysitProgress::Repairing { attempt: 1 },
            BabysitProgress::Completed,
        ]
    );
}

#[tokio::test]
async fn all_missing_lines_are_repaired_by_one_partial_patch() {
    let provider = Arc::new(FakeBabysitProvider::with_invalid_rounds(1));
    let application = TestBabysitApplication::with_provider(provider.clone());
    let cancellation = OperationContext::background().cancellation().clone();

    let output = application
        .explain_file(
            request("review-many", "snapshot-many", "src/example.ts"),
            text_change(
                "src/example.ts",
                None,
                Some("const first = 1;\nconst second = 2;\nconst third = 3;"),
            ),
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(provider.generate_calls(), 1);
    assert_eq!(provider.repair_calls(), 1);
    assert_eq!(output.after.len(), 3);
    assert!(output
        .after
        .keys()
        .all(|line| line.parse::<usize>().is_ok()));
}

#[tokio::test]
async fn fake_provider_counts_invalid_generate_and_repair_rounds() {
    let provider = Arc::new(FakeBabysitProvider::with_invalid_rounds(2));
    let application = TestBabysitApplication::with_provider(provider.clone());
    let cancellation = OperationContext::background().cancellation().clone();

    application
        .explain_file(
            request("review-1", "snapshot-1", "src/example.ts"),
            text_change("src/example.ts", None, Some("const value = 1;")),
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();

    assert_eq!(provider.generate_calls(), 1);
    assert_eq!(provider.repair_calls(), 2);
}

#[tokio::test]
async fn incomplete_output_fails_after_three_repairs_and_is_not_cached() {
    let provider = Arc::new(FakeBabysitProvider::with_invalid_rounds(5));
    let application = TestBabysitApplication::with_provider(provider.clone());
    let cancellation = OperationContext::background().cancellation().clone();
    let content = text_change("src/example.ts", None, Some("const value = 1;"));

    let failure = application
        .explain_file(
            request("review-1", "snapshot-1", "src/example.ts"),
            content.clone(),
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap_err();
    assert!(matches!(failure, BabysitError::RepairExhausted { .. }));
    assert_eq!(provider.repair_calls(), 3);

    application
        .explain_file(
            request("review-1", "snapshot-1", "src/example.ts"),
            content,
            &cancellation,
            |_| Ok(()),
        )
        .await
        .unwrap();
    assert_eq!(provider.generate_calls(), 2);
}

#[test]
fn response_validation_rejects_extra_and_missing_but_accepts_soft_text_limits() {
    let request = BabysitProviderRequest {
        analysis_run_id: 1,
        path: "src/example.ts".to_owned(),
        locale: "zh-CN".to_owned(),
        workspace_path: "/tmp/narrant-test-workspace".to_owned(),
        before: "const value = 1;".to_owned(),
        after: "const value = 2;".to_owned(),
        targets: vec![
            LineTarget {
                side: FileSide::Before,
                line_number: 1,
                source: "const value = 1;".to_owned(),
            },
            LineTarget {
                side: FileSide::After,
                line_number: 1,
                source: "const value = 2;".to_owned(),
            },
        ],
        model: "test-model".to_owned(),
        reasoning_effort: "test-effort".to_owned(),
    };

    let mut output = BabysitFileExplanations::default();
    output.before.insert("1".to_owned(), "ok".to_owned());
    output.after.insert("1".to_owned(), "bad\ntext".to_owned());
    assert!(validate_response(&request, &output).is_ok());
    output.after.insert("1".to_owned(), "很长".repeat(200));
    assert!(validate_response(&request, &output).is_ok());

    output.after.insert("2".to_owned(), "extra".to_owned());
    assert!(matches!(
        validate_response(&request, &output),
        Err(ValidationFailure::UnexpectedLine {
            side: FileSide::After,
            ..
        })
    ));
    output.after.remove("2");
    output.before.remove("1");
    assert!(matches!(
        validate_response(&request, &output),
        Err(ValidationFailure::MissingLine {
            side: FileSide::Before,
            line_number: 1
        })
    ));
}

#[test]
fn provider_output_canonicalizes_zero_padded_requested_line_keys_before_repair() {
    let request = BabysitProviderRequest {
        analysis_run_id: 1,
        path: "src/example.ts".to_owned(),
        locale: "zh-CN".to_owned(),
        workspace_path: "/tmp/narrant-test-workspace".to_owned(),
        before: "const value = 1;".to_owned(),
        after: "const value = 2;".to_owned(),
        targets: vec![
            LineTarget {
                side: FileSide::Before,
                line_number: 1,
                source: "const value = 1;".to_owned(),
            },
            LineTarget {
                side: FileSide::After,
                line_number: 1,
                source: "const value = 2;".to_owned(),
            },
        ],
        model: "test-model".to_owned(),
        reasoning_effort: "provider_default".to_owned(),
    };
    let mut output = BabysitFileExplanations::default();
    output
        .before
        .insert("0001".to_owned(), "旧行解释".to_owned());
    output
        .after
        .insert("0001".to_owned(), "新行解释".to_owned());

    let failures = normalize_and_collect_repairs(&request, &mut output);

    assert!(failures.is_empty());
    assert_eq!(output.before.get("1"), Some(&"旧行解释".to_owned()));
    assert_eq!(output.after.get("1"), Some(&"新行解释".to_owned()));
    assert!(validate_response(&request, &output).is_ok());
}

#[test]
fn provider_json_parser_rejects_duplicate_keys() {
    let duplicate_line = r#"{"before":{"1":"first","1":"second"},"after":{}}"#;
    assert!(matches!(
        parse_provider_response(duplicate_line),
        Err(BabysitError::InvalidProviderOutput { .. })
    ));
    let duplicate_side = r#"{"before":{},"before":{},"after":{}}"#;
    assert!(matches!(
        parse_provider_response(duplicate_side),
        Err(BabysitError::InvalidProviderOutput { .. })
    ));
    assert!(matches!(
        parse_provider_response("{}"),
        Err(BabysitError::InvalidProviderOutput { .. })
    ));
}

#[test]
fn error_payload_serializes_a_stable_code_and_detail_shape() {
    let payload = BabysitErrorPayload::from(BabysitError::UnsupportedFile {
        reason: "lock file".to_owned(),
    });

    assert_eq!(
        serde_json::to_value(payload).unwrap(),
        serde_json::json!({
            "code": "unsupported_file",
            "detail": "此文件不支持宝宝模式：lock file"
        })
    );
}

#[test]
fn custom_transport_payload_serializes_only_whitelisted_diagnostics() {
    let payload = BabysitErrorPayload::from(BabysitError::ProviderDiagnostic {
        detail: "脱敏 provider 失败".to_owned(),
        diagnostic: BabysitProviderDiagnostic::custom_transport(
            "openai_chat_completions",
            "model",
            "client",
            "unknown",
            "0123456789abcdef",
        ),
    });

    assert_eq!(
        serde_json::to_value(payload).unwrap(),
        serde_json::json!({
            "code": "provider",
            "detail": "宝宝模式 provider 失败：脱敏 provider 失败",
            "diagnostic": {
                "providerId": "custom",
                "protocol": "openai_chat_completions",
                "phase": "model",
                "status": "client",
                "reason": "unknown",
                "errorCategory": "transport",
                "originHash": "0123456789abcdef"
            }
        })
    );
}

#[test]
fn provider_setup_payload_carries_identity_without_exposing_runtime_details() {
    let payload = BabysitErrorPayload::from(BabysitError::provider_setup(
        "claude-code",
        BabysitProviderSetupFailure::NotFound,
    ));

    assert_eq!(
        serde_json::to_value(payload).unwrap(),
        serde_json::json!({
            "code": "provider_not_found",
            "detail": "未找到 claude-code provider 可执行文件",
            "providerId": "claude-code",
            "recovery": "open_application_settings"
        })
    );
}

#[test]
fn provider_setup_failure_categories_are_shared_by_installed_adapters() {
    let cases = [
        (
            BabysitError::provider_setup("claude-code", BabysitProviderSetupFailure::SignedOut),
            "provider_signed_out",
            "login_provider",
        ),
        (
            BabysitError::provider_setup("claude-code", BabysitProviderSetupFailure::Incompatible),
            "provider_incompatible",
            "upgrade_provider",
        ),
        (
            BabysitError::provider_setup(
                "claude-code",
                BabysitProviderSetupFailure::InitializationFailed,
            ),
            "provider_initialization_failed",
            "retry",
        ),
        (
            BabysitError::provider_setup(
                "claude-code",
                BabysitProviderSetupFailure::ExecutableInvalid,
            ),
            "provider_executable_invalid",
            "open_application_settings",
        ),
    ];

    for (error, code, recovery) in cases {
        let value = serde_json::to_value(BabysitErrorPayload::from(error)).unwrap();
        assert_eq!(value["code"], code);
        assert_eq!(value["providerId"], "claude-code");
        assert_eq!(value["recovery"], recovery);
    }
}

#[test]
fn codex_setup_failures_use_provider_codes_and_provider_recovery_actions() {
    let cases = [
        (
            BabysitProviderSetupFailure::NotFound,
            "provider_not_found",
            Some("open_application_settings"),
        ),
        (
            BabysitProviderSetupFailure::ExecutableInvalid,
            "provider_executable_invalid",
            Some("open_application_settings"),
        ),
        (
            BabysitProviderSetupFailure::SignedOut,
            "provider_signed_out",
            Some("login_provider"),
        ),
        (
            BabysitProviderSetupFailure::Incompatible,
            "provider_incompatible",
            Some("upgrade_provider"),
        ),
        (
            BabysitProviderSetupFailure::InitializationFailed,
            "provider_initialization_failed",
            Some("retry"),
        ),
    ];

    for (failure, code, recovery) in cases {
        let value = serde_json::to_value(BabysitErrorPayload::from(BabysitError::provider_setup(
            "codex", failure,
        )))
        .unwrap();
        assert_eq!(value["code"], code);
        assert_eq!(value["providerId"], "codex");
        assert_eq!(
            value.get("recovery").and_then(|value| value.as_str()),
            recovery
        );
    }
}

/// 只记录 application lifecycle 调用的 provider-aware runtime fake。
struct FakeInstalledRuntime {
    status: Mutex<BabysitProviderRuntimeStatus>,
    ready_calls: AtomicUsize,
    shutdown_calls: AtomicUsize,
}

impl FakeInstalledRuntime {
    /// 构造一个尚未探测的 installed provider runtime。
    fn new() -> Self {
        Self {
            status: Mutex::new(BabysitProviderRuntimeStatus::Unknown),
            ready_calls: AtomicUsize::new(0),
            shutdown_calls: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl InstalledProviderRuntime for FakeInstalledRuntime {
    /// 返回 fake runtime 保存的 provider-independent 状态。
    fn status(&self) -> BabysitProviderRuntimeStatus {
        self.status
            .lock()
            .map(|status| *status)
            .unwrap_or(BabysitProviderRuntimeStatus::InitFailed)
    }

    /// 记录一次 readiness probe，并模拟成功初始化。
    async fn ensure_ready(&self, _workspace_path: &Path) -> Result<(), BabysitError> {
        self.ready_calls.fetch_add(1, Ordering::AcqRel);
        if let Ok(mut status) = self.status.lock() {
            *status = BabysitProviderRuntimeStatus::Ready;
        }
        Ok(())
    }

    /// 记录应用退出时的 runtime shutdown。
    async fn shutdown(&self) {
        self.shutdown_calls.fetch_add(1, Ordering::AcqRel);
    }
}

#[tokio::test]
async fn application_delegates_installed_provider_lifecycle_through_one_runtime_seam() {
    let provider = Arc::new(FakeBabysitProvider::default());
    let runtime = Arc::new(FakeInstalledRuntime::new());
    let application = TestBabysitApplication::with_provider_runtime(provider, runtime.clone());

    application
        .explain_file(
            request("review-runtime", "snapshot-runtime", "src/runtime.rs"),
            text_change("src/runtime.rs", None, Some("fn runtime() {}\n")),
            &OperationContext::background().cancellation().clone(),
            |_| Ok(()),
        )
        .await
        .unwrap();
    application.shutdown().await.unwrap();

    assert_eq!(runtime.ready_calls.load(Ordering::Acquire), 1);
    assert_eq!(runtime.status(), BabysitProviderRuntimeStatus::Ready);
    assert_eq!(runtime.shutdown_calls.load(Ordering::Acquire), 1);
}

#[test]
fn input_error_payload_redacts_paths_source_tokens_and_process_output() {
    let payload = BabysitErrorPayload::input_unavailable(
        "/Users/alice/repo/src/example.ts source=const secret='token'; stderr=raw output",
    );
    let serialized = serde_json::to_string(&payload).unwrap();
    assert!(!serialized.contains("/Users/alice"));
    assert!(!serialized.contains("secret"));
    assert!(!serialized.contains("token"));
    assert!(!serialized.contains("raw output"));
    assert_eq!(payload.code, BabysitErrorCode::InputUnavailable);
}

#[test]
fn eligibility_rejects_non_code_and_generated_files() {
    let text = text_change("README.md", Some("# docs"), Some("# docs"));
    assert!(matches!(
        ensure_eligible("README.md", &text, None),
        Err(BabysitError::UnsupportedFile { .. })
    ));
    let lock = text_change("Cargo.lock", Some("version = 3"), Some("version = 4"));
    assert!(matches!(
        ensure_eligible("Cargo.lock", &lock, None),
        Err(BabysitError::UnsupportedFile { .. })
    ));
    let vendored = text_change(
        "vendor/helper.ts",
        Some("export const x = 1;"),
        Some("export const x = 2;"),
    );
    assert!(matches!(
        ensure_eligible("vendor/helper.ts", &vendored, None),
        Err(BabysitError::UnsupportedFile { .. })
    ));

    for path in [
        "dist/generated.ts",
        "src/bundle.min.js",
        "tests/example.snap",
        "src/data.json",
    ] {
        let content = text_change(path, Some("const value = 1;"), Some("const value = 2;"));
        assert!(
            matches!(
                ensure_eligible(path, &content, None),
                Err(BabysitError::UnsupportedFile { .. })
            ),
            "expected {path} to be ineligible"
        );
    }

    let binary = FileChangeContent {
        path: "src/image.ts".to_owned(),
        old_path: None,
        status: FileChangeStatus::Modified,
        before: Some(FileContent::Binary { size: 1 }),
        after: Some(FileContent::Binary { size: 2 }),
    };
    assert!(matches!(
        ensure_eligible("src/image.ts", &binary, Some("typescript")),
        Err(BabysitError::UnsupportedFile { .. })
    ));

    let symlink = FileChangeContent {
        path: "src/link.ts".to_owned(),
        old_path: None,
        status: FileChangeStatus::Modified,
        before: Some(FileContent::Symlink {
            size: 1,
            target: "target.ts".to_owned(),
        }),
        after: Some(FileContent::Symlink {
            size: 1,
            target: "target.ts".to_owned(),
        }),
    };
    assert!(matches!(
        ensure_eligible("src/link.ts", &symlink, Some("typescript")),
        Err(BabysitError::UnsupportedFile { .. })
    ));

    let submodule = FileChangeContent {
        path: "src/module.ts".to_owned(),
        old_path: None,
        status: FileChangeStatus::Modified,
        before: Some(FileContent::Submodule {
            commit_id: "old".to_owned(),
        }),
        after: Some(FileContent::Submodule {
            commit_id: "new".to_owned(),
        }),
    };
    assert!(matches!(
        ensure_eligible("src/module.ts", &submodule, Some("typescript")),
        Err(BabysitError::UnsupportedFile { .. })
    ));
}

#[test]
fn backend_eligibility_accepts_extensionless_detected_language() {
    let content = text_change("bin/tool", Some("print('ok')"), Some("print('ok')"));
    assert_eq!(
        eligibility("bin/tool", &content, Some("python")),
        BabysitEligibility {
            eligible: true,
            reason: None
        }
    );
    assert_eq!(
        eligibility("bin/tool", &content, None).reason,
        Some(BabysitEligibilityReason::UnsupportedLanguage)
    );
}

/// Node 与 TypeScript 的模块扩展名应与普通 JS/TS 文件使用相同 eligibility。
#[test]
fn backend_eligibility_accepts_mjs_and_mts() {
    for path in ["src/module.mjs", "src/module.mts"] {
        let content = text_change(
            path,
            Some("export const old = 1;"),
            Some("export const next = 2;"),
        );
        assert!(
            eligibility(path, &content, None).eligible,
            "expected {path} to be eligible"
        );
    }
}
