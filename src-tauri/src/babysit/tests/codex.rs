use super::*;
use crate::babysit::codex::CODEX_ANALYSIS_TIMEOUT;
use crate::operation::{OperationContext, TestCancellation};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

/// 构造一个只含 before/after 第 1 行的 provider 请求，便于 fixture 保持确定性。
fn request() -> BabysitProviderRequest {
    BabysitProviderRequest {
        path: "src/example.ts".to_owned(),
        locale: "zh-CN".to_owned(),
        workspace_path: "/tmp/narrant-review-workspace".to_owned(),
        before: "const old = 1;".to_owned(),
        after: "const next = 2;".to_owned(),
        targets: vec![
            LineTarget {
                side: FileSide::Before,
                line_number: 1,
                source: "const old = 1;".to_owned(),
            },
            LineTarget {
                side: FileSide::After,
                line_number: 1,
                source: "const next = 2;".to_owned(),
            },
        ],
    }
}

/// 启动确定性 shell JSONL fixture，并将事件写入临时文件供断言读取。
fn provider(mode: &str, timeout: Duration) -> (CodexAppServerProvider, TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let events = temp.path().join("events.log");
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src/babysit/tests/fixtures/fake_app_server.sh");
    let args = vec![
        OsString::from(fixture),
        OsString::from(mode),
        OsString::from(&events),
    ];
    (
        CodexAppServerProvider::with_command_parts(PathBuf::from("sh"), args, timeout),
        temp,
        events,
    )
}

/// 等待 fixture 记录目标 RPC，避免并行测试在 provider 启动前就取消请求。
async fn wait_for_event(path: &Path, expected: &str) {
    for _ in 0..1000 {
        if std::fs::read_to_string(path)
            .map(|events| events.lines().any(|line| line == expected))
            .unwrap_or(false)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("fixture did not record {expected}");
}

#[tokio::test]
async fn app_server_generates_structured_output_and_deletes_thread() {
    let (provider, _temp, events) = provider("complete", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();
    let output = provider.generate(request(), &cancellation).await.unwrap();

    assert_eq!(output.before["1"], "before explanation");
    assert_eq!(output.after["1"], "after explanation");
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "initialize"));
    assert!(events.lines().any(|line| line == "initialize:experimental"));
    assert!(events.lines().any(|line| line == "config/read"));
    assert!(events.lines().any(|line| line == "configRequirements/read"));
    assert!(events.lines().any(|line| line == "thread/start:restricted"));
    assert!(events.lines().any(|line| line == "thread/start"));
    assert!(events.lines().any(|line| line == "turn/start"));
    assert!(events.lines().any(|line| line == "thread/delete"));
}

#[tokio::test]
async fn malformed_structured_output_can_be_repaired_on_the_same_thread() {
    let (provider, _temp, events) = provider("repair", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();
    let request = request();
    let first = provider
        .generate(request.clone(), &cancellation)
        .await
        .unwrap();
    assert!(first.before.is_empty());
    assert!(matches!(
        validate_response(&request, &first),
        Err(ValidationFailure::MissingLine { .. })
    ));

    let repaired = provider
        .repair(
            request,
            first,
            ValidationFailure::MissingLine {
                side: FileSide::Before,
                line_number: 1,
            },
            &cancellation,
        )
        .await
        .unwrap();
    assert_eq!(repaired.after["1"], "after explanation");
    let events = std::fs::read_to_string(events).unwrap();
    assert_eq!(
        events
            .lines()
            .filter(|line| *line == "thread/start")
            .count(),
        1
    );
    assert_eq!(
        events.lines().filter(|line| *line == "turn/start").count(),
        2
    );
    assert_eq!(
        events
            .lines()
            .filter(|line| *line == "thread/delete")
            .count(),
        1
    );
}

#[tokio::test]
async fn abandoning_an_invalid_run_interrupts_and_deletes_its_thread() {
    let (provider, _temp, events) = provider("repair", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();
    let request = request();
    let first = provider
        .generate(request.clone(), &cancellation)
        .await
        .unwrap();
    assert!(validate_response(&request, &first).is_err());

    provider.abandon(&request).await;

    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "turn/interrupt"));
    assert!(events.lines().any(|line| line == "thread/delete:thread-1"));
}

#[tokio::test]
async fn third_invalid_repair_deletes_the_thread_without_partial_success() {
    let (provider, _temp, events) = provider("repair-fail", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();
    let request = request();
    let mut output = provider
        .generate(request.clone(), &cancellation)
        .await
        .unwrap();

    for _ in 0..MAX_REPAIR_ATTEMPTS {
        let failure = validate_response(&request, &output).unwrap_err();
        output = provider
            .repair(request.clone(), output, failure, &cancellation)
            .await
            .unwrap();
    }

    assert!(validate_response(&request, &output).is_err());
    let events = std::fs::read_to_string(events).unwrap();
    assert_eq!(
        events
            .lines()
            .filter(|line| *line == "thread/start")
            .count(),
        1
    );
    assert_eq!(
        events.lines().filter(|line| *line == "turn/start").count(),
        4
    );
    assert_eq!(
        events
            .lines()
            .filter(|line| *line == "thread/delete")
            .count(),
        1
    );
}

#[tokio::test]
async fn process_exit_is_redacted_and_does_not_return_partial_output() {
    let (provider, _temp, events) = provider("process-fail", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();
    let error = provider
        .generate(request(), &cancellation)
        .await
        .unwrap_err();

    assert!(matches!(error, BabysitError::Provider { .. }));
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "thread/start"));
    assert!(events.lines().any(|line| line == "turn/start"));
    assert!(!events.lines().any(|line| line == "thread/delete"));
}

#[tokio::test]
async fn cancellation_interrupts_and_deletes_the_active_thread() {
    let (provider, _temp, events) = provider("hang", CODEX_ANALYSIS_TIMEOUT);
    let provider = Arc::new(provider);
    let request = request();
    let cancellation = TestCancellation::default();
    let worker_cancellation = cancellation.clone();
    let worker_provider = provider.clone();
    let worker = tokio::spawn(async move {
        worker_provider
            .generate(request, &worker_cancellation)
            .await
    });
    wait_for_event(&events, "turn/start").await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    cancellation.cancel();
    let error = worker.await.unwrap().unwrap_err();
    assert!(matches!(error, BabysitError::Cancelled));
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "turn/interrupt"));
    assert!(events.lines().any(|line| line == "thread/delete"));
    let interrupted = events
        .lines()
        .position(|line| line == "emit:turn/completed:interrupted")
        .unwrap();
    let deleted = events
        .lines()
        .position(|line| line == "thread/delete:thread-1")
        .unwrap();
    assert!(interrupted < deleted);
}

#[tokio::test]
async fn dropping_the_generate_future_cleans_up_the_active_thread() {
    let (provider, _temp, events) = provider("hang", CODEX_ANALYSIS_TIMEOUT);
    let provider = Arc::new(provider);
    let worker_provider = provider.clone();
    let worker = tokio::spawn(async move {
        let cancellation = OperationContext::background().cancellation().clone();
        worker_provider.generate(request(), &cancellation).await
    });
    wait_for_event(&events, "turn/start").await;
    worker.abort();
    let _ = worker.await;

    wait_for_event(&events, "thread/delete:thread-1").await;
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "turn/interrupt"));
}

#[tokio::test]
async fn total_deadline_fails_and_cleans_up_without_partial_output() {
    let (provider, _temp, events) = provider("hang", Duration::from_secs(2));
    let cancellation = OperationContext::background().cancellation().clone();
    let worker = tokio::spawn(async move { provider.generate(request(), &cancellation).await });
    wait_for_event(&events, "turn/start").await;
    let error = worker.await.unwrap().unwrap_err();
    assert!(matches!(error, BabysitError::Provider { .. }));
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "turn/interrupt"));
    assert!(events.lines().any(|line| line == "thread/delete"));
}

#[tokio::test]
async fn startup_removes_only_marked_orphan_threads() {
    let (provider, _temp, events) = provider("orphan", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();
    let _ = provider.generate(request(), &cancellation).await.unwrap();
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "thread/list:unfiltered"));
    assert!(!events
        .lines()
        .any(|line| line == "thread/list:source-filtered"));
    assert!(events
        .lines()
        .any(|line| line == "thread/delete:orphan-thread"));
    assert!(!events
        .lines()
        .any(|line| line == "thread/delete:other-thread"));
}

#[tokio::test]
async fn insecure_thread_policy_fails_closed_before_starting_a_turn() {
    let (provider, _temp, events) = provider("insecure-policy", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();
    let error = provider
        .generate(request(), &cancellation)
        .await
        .unwrap_err();

    assert!(matches!(error, BabysitError::Provider { .. }));
    let events = std::fs::read_to_string(events).unwrap();
    assert!(!events.lines().any(|line| line == "turn/start"));
    assert!(events.lines().any(|line| line == "thread/delete:thread-1"));
}

#[tokio::test]
async fn personal_model_instruction_file_fails_closed_before_thread_start() {
    let (provider, _temp, events) = provider("personal-model-instructions", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();
    let error = provider
        .generate(request(), &cancellation)
        .await
        .unwrap_err();

    assert!(matches!(error, BabysitError::Provider { .. }));
    let events = std::fs::read_to_string(events).unwrap();
    assert!(!events.lines().any(|line| line == "thread/start"));
}

#[tokio::test]
async fn managed_instruction_requirement_fails_closed_before_thread_start() {
    let (provider, _temp, events) = provider("managed-instructions", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();
    let error = provider
        .generate(request(), &cancellation)
        .await
        .unwrap_err();

    assert!(matches!(error, BabysitError::Provider { .. }));
    let events = std::fs::read_to_string(events).unwrap();
    assert!(!events.lines().any(|line| line == "thread/start"));
}

#[tokio::test]
async fn stale_item_and_nested_turn_notifications_are_ignored() {
    let (provider, _temp, _events) = provider("stale", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();
    let output = provider.generate(request(), &cancellation).await.unwrap();

    assert_eq!(output.before["1"], "before explanation");
    assert_eq!(output.after["1"], "after explanation");
}

#[tokio::test]
async fn cancellation_after_thread_start_response_still_deletes_the_thread() {
    let (provider, _temp, events) = provider("delayed-thread-start", CODEX_ANALYSIS_TIMEOUT);
    let provider = Arc::new(provider);
    let cancellation = TestCancellation::default();
    let worker_cancellation = cancellation.clone();
    let worker_provider = provider.clone();
    let worker = tokio::spawn(async move {
        worker_provider
            .generate(request(), &worker_cancellation)
            .await
    });
    wait_for_event(&events, "thread/start").await;
    cancellation.cancel();

    let error = worker.await.unwrap().unwrap_err();
    assert!(matches!(error, BabysitError::Cancelled));
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "thread/delete:thread-1"));
}

#[tokio::test]
async fn blocked_stdin_writer_obeys_the_analysis_deadline() {
    let (provider, _temp, events) = provider("block-writer", Duration::from_secs(2));
    let cancellation = OperationContext::background().cancellation().clone();
    let mut request = request();
    request.before = "x".repeat(2 * 1024 * 1024);
    request.after = "y".repeat(2 * 1024 * 1024);
    let started = std::time::Instant::now();

    let error = provider.generate(request, &cancellation).await.unwrap_err();

    assert!(matches!(error, BabysitError::Provider { .. }));
    assert!(started.elapsed() < Duration::from_secs(5));
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "thread/start"));
}
