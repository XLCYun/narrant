use super::*;
use crate::operation::OperationContext;
use crate::review::{FileChangeContent, FileChangeStatus, FileContent};
use std::sync::Arc;

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
    }
}

#[tokio::test]
async fn fake_provider_generates_all_non_empty_lines_in_one_batch() {
    let provider = Arc::new(FakeBabysitProvider::default());
    let application = BabysitApplication::new(provider.clone());
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
    let application = BabysitApplication::new(provider.clone());
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
async fn cache_does_not_reuse_an_explanation_from_another_locale() {
    let provider = Arc::new(FakeBabysitProvider::default());
    let application = BabysitApplication::new(provider.clone());
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
async fn invalid_output_is_repaired_in_the_same_analysis_run() {
    let provider = Arc::new(FakeBabysitProvider::with_invalid_rounds(1));
    let application = BabysitApplication::new(provider.clone());
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
async fn fake_provider_counts_invalid_generate_and_repair_rounds() {
    let provider = Arc::new(FakeBabysitProvider::with_invalid_rounds(2));
    let application = BabysitApplication::new(provider.clone());
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
    let application = BabysitApplication::new(provider.clone());
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
fn response_validation_rejects_extra_missing_and_unsafe_lines() {
    let request = BabysitProviderRequest {
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
    };

    let mut output = BabysitFileExplanations::default();
    output.before.insert("1".to_owned(), "ok".to_owned());
    output.after.insert("1".to_owned(), "bad\ntext".to_owned());
    assert!(matches!(
        validate_response(&request, &output),
        Err(ValidationFailure::UnsafeExplanation {
            side: FileSide::After,
            line_number: 1
        })
    ));

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
