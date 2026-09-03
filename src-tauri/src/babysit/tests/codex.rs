use super::*;
use crate::babysit::codex::CODEX_ANALYSIS_TIMEOUT;
use crate::operation::{OperationContext, TestCancellation};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn codex_deadlines_match_the_runtime_policy() {
    assert_eq!(CODEX_ANALYSIS_TIMEOUT, Duration::from_secs(10 * 60));
    assert_eq!(CODEX_FOREGROUND_CLEANUP_TIMEOUT, Duration::from_secs(10));
    assert_eq!(CODEX_SHUTDOWN_CLEANUP_TIMEOUT, Duration::from_secs(15));
    assert_eq!(CODEX_PROCESS_REAP_TIMEOUT, Duration::from_secs(5));
    assert_eq!(CODEX_INITIALIZATION_TIMEOUT, Duration::from_secs(30));
}

/// 断言 Codex adapter 通过共享的 installed-provider setup 契约返回指定失败类别。
fn assert_codex_setup_failure(error: &BabysitError, expected: BabysitProviderSetupFailure) {
    assert!(matches!(
        error,
        BabysitError::ProviderSetup { provider_id, failure }
            if provider_id == "codex" && *failure == expected
    ));
}

/// 构造一个只含 before/after 第 1 行的 provider 请求，便于 fixture 保持确定性。
fn request() -> BabysitProviderRequest {
    BabysitProviderRequest {
        analysis_run_id: 1,
        path: "src/example.ts".to_owned(),
        locale: "zh-CN".to_owned(),
        workspace_path: "/tmp/narrant-review-workspace".to_owned(),
        repository_exploration: true,
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
        model: "gpt-5.6-luna".to_owned(),
        reasoning_effort: "low".to_owned(),
    }
}

/// 严格输出 schema 应枚举本次请求的目标行，避免 Codex 拒绝开放对象结构。
#[test]
fn structured_output_schema_requires_exact_target_lines() {
    let schema = crate::babysit::codex::structured_output_schema(&request());

    assert_eq!(schema["required"], serde_json::json!(["before", "after"]));
    for side in ["before", "after"] {
        assert_eq!(
            schema["properties"][side]["required"],
            serde_json::json!(["1"])
        );
        assert_eq!(schema["properties"][side]["additionalProperties"], false);
        assert!(schema["properties"][side]["properties"]["1"]["maxLength"].is_null());
    }
}

#[test]
fn codex_generation_and_repair_prompts_discourage_repository_exploration() {
    let request = request();
    let generation = CodexAppServerProvider::generation_prompt(&request);
    assert!(generation.contains("Do not explore the repository by default"));
    assert!(generation.contains("one specific unresolved symbol or behavior"));
    assert!(generation.contains("do not repeat or broaden a query"));

    let repair = CodexAppServerProvider::repair_prompt(
        &request,
        &[ValidationFailure::MissingLine {
            side: FileSide::Before,
            line_number: 1,
        }],
        false,
    );
    let repair = repair.to_ascii_lowercase();
    assert!(repair.contains("do not explore the repository by default"));
    assert!(repair.contains("do not repeat or broaden a query"));
}

#[test]
fn codex_prompt_forbids_repository_exploration_when_disabled() {
    let mut request = request();
    request.repository_exploration = false;

    let generation = CodexAppServerProvider::generation_prompt(&request);
    assert!(generation.contains("Do not call tools or explore the repository"));
    assert!(!generation.contains("Explore only when"));
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

/// 等待 fixture 记录同一事件多次，覆盖 generate 后进入 repair 的活动状态。
async fn wait_for_event_count(path: &Path, expected: &str, count: usize) {
    for _ in 0..1000 {
        let observed = std::fs::read_to_string(path)
            .map(|events| events.lines().filter(|line| *line == expected).count())
            .unwrap_or_default();
        if observed >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("fixture did not record {expected} {count} times");
}

/// 等待客户端完成 active thread 登记，避免 fixture 写日志早于 provider ownership。
async fn wait_for_active_thread(provider: &CodexAppServerProvider) {
    for _ in 0..1000 {
        if provider
            .active_threads
            .lock()
            .map(|active| !active.is_empty())
            .unwrap_or(false)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("provider did not register an active thread");
}

#[tokio::test]
async fn app_server_generates_structured_output_and_deletes_thread() {
    let (provider, _temp, events) = provider("complete", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();
    let output = provider.generate(request(), &cancellation).await.unwrap();

    assert_eq!(output.before["1"], "before explanation");
    assert_eq!(output.after["1"], "after explanation");
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events
        .lines()
        .any(|line| line == "numbered-content-in-input"));
    assert!(events.lines().any(|line| line == "account/read"));
    assert!(events.lines().any(|line| line == "initialize"));
    assert!(events.lines().any(|line| line == "initialize:experimental"));
    assert!(events.lines().any(|line| line == "thread/start:restricted"));
    assert!(events.lines().any(|line| line == "thread/start"));
    assert!(events.lines().any(|line| line == "turn/start"));
    assert!(events.lines().any(|line| line == "thread/delete"));
}

#[tokio::test]
async fn shutdown_releases_the_owned_app_server_after_a_completed_run() {
    let (provider, _temp, _events) = provider("complete", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();
    provider.generate(request(), &cancellation).await.unwrap();

    provider.shutdown().await;

    assert!(provider.process.lock().await.is_none());
    assert!(provider.active_threads.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cancelled_generate_is_cleaned_before_provider_shutdown() {
    let (provider, _temp, events) = provider("hang", CODEX_ANALYSIS_TIMEOUT);
    let provider = Arc::new(provider);
    let cancellation = TestCancellation::default();
    let worker_provider = provider.clone();
    let worker_cancellation = cancellation.clone();
    let worker = tokio::spawn(async move {
        worker_provider
            .generate(request(), &worker_cancellation)
            .await
    });

    wait_for_event(&events, "turn/start:real").await;
    wait_for_active_thread(&provider).await;
    cancellation.cancel();
    provider.shutdown().await;

    let error = worker.await.unwrap().unwrap_err();
    assert!(matches!(error, BabysitError::Cancelled));
    let events = std::fs::read_to_string(&events).unwrap();
    let interrupted = events
        .lines()
        .position(|line| line == "emit:turn/completed:interrupted")
        .unwrap();
    let deleted = events
        .lines()
        .position(|line| line == "thread/delete:thread-1")
        .unwrap();
    assert!(interrupted < deleted);
    assert!(provider.process.lock().await.is_none());
    assert!(provider.active_threads.lock().unwrap().is_empty());
}

#[tokio::test]
async fn cancelled_repair_is_cleaned_before_provider_shutdown() {
    let (provider, _temp, events) = provider("repair-hang", CODEX_ANALYSIS_TIMEOUT);
    let provider = Arc::new(provider);
    let cancellation = TestCancellation::default();
    let first = provider
        .generate(request(), &cancellation)
        .await
        .expect("first scripted response should be invalid but transport-successful");
    assert!(validate_response(&request(), &first).is_err());

    let worker_provider = provider.clone();
    let worker_cancellation = cancellation.clone();
    let worker = tokio::spawn(async move {
        worker_provider
            .repair(
                request(),
                first,
                vec![ValidationFailure::MissingLine {
                    side: FileSide::Before,
                    line_number: 1,
                }],
                &worker_cancellation,
            )
            .await
    });
    wait_for_event_count(&events, "turn/start:real", 2).await;
    wait_for_active_thread(&provider).await;
    cancellation.cancel();
    provider.shutdown().await;

    let error = worker.await.unwrap().unwrap_err();
    assert!(matches!(error, BabysitError::Cancelled));
    let events = std::fs::read_to_string(&events).unwrap();
    let interrupted = events
        .lines()
        .position(|line| line == "emit:turn/completed:interrupted")
        .unwrap();
    let deleted = events
        .lines()
        .enumerate()
        .filter(|(_, line)| *line == "thread/delete:thread-1")
        .map(|(index, _)| index)
        .last()
        .unwrap();
    assert!(interrupted < deleted);
    assert!(provider.process.lock().await.is_none());
    assert!(provider.active_threads.lock().unwrap().is_empty());
}

#[tokio::test]
async fn thread_list_capability_probe_returns_upgrade_recovery_category() {
    let (provider, _temp, _events) = provider("thread-list-no-data", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();

    let error = provider
        .generate(request(), &cancellation)
        .await
        .unwrap_err();

    assert!(matches!(
        &error,
        BabysitError::ProviderSetup { provider_id, failure: BabysitProviderSetupFailure::Incompatible }
            if provider_id == "codex"
    ));
    let payload = crate::babysit::BabysitErrorPayload::from(error);
    assert_eq!(
        payload.code,
        crate::babysit::BabysitErrorCode::ProviderIncompatible
    );
    assert_eq!(
        payload.recovery,
        Some(crate::babysit::BabysitRecoveryAction::UpgradeProvider)
    );
    assert!(!payload.detail.contains("/tmp"));
}

#[tokio::test]
async fn initialize_transport_failure_returns_retry_recovery_category() {
    let (provider, _temp, _events) = provider("init-fail", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();

    let error = provider
        .generate(request(), &cancellation)
        .await
        .unwrap_err();

    assert!(matches!(
        &error,
        BabysitError::ProviderSetup { provider_id, failure: BabysitProviderSetupFailure::InitializationFailed }
            if provider_id == "codex"
    ));
    let payload = crate::babysit::BabysitErrorPayload::from(error);
    assert_eq!(
        payload.recovery,
        Some(crate::babysit::BabysitRecoveryAction::Retry)
    );
}

#[tokio::test]
async fn app_server_auth_failure_returns_login_recovery_category() {
    let (provider, _temp, _events) = provider("auth-fail", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();

    let error = provider
        .generate(request(), &cancellation)
        .await
        .unwrap_err();

    assert!(matches!(
        &error,
        BabysitError::ProviderSetup { provider_id, failure: BabysitProviderSetupFailure::SignedOut }
            if provider_id == "codex"
    ));
    let payload = crate::babysit::BabysitErrorPayload::from(error);
    assert_eq!(
        payload.code,
        crate::babysit::BabysitErrorCode::ProviderSignedOut
    );
    assert_eq!(
        payload.recovery,
        Some(crate::babysit::BabysitRecoveryAction::LoginProvider)
    );
}

#[tokio::test]
async fn malformed_initialize_response_returns_retry_recovery_category() {
    let (provider, _temp, _events) = provider("init-no-agent", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();

    let error = provider
        .generate(request(), &cancellation)
        .await
        .unwrap_err();

    assert!(matches!(
        &error,
        BabysitError::ProviderSetup { provider_id, failure: BabysitProviderSetupFailure::InitializationFailed }
            if provider_id == "codex"
    ));
    let payload = crate::babysit::BabysitErrorPayload::from(error);
    assert_eq!(
        payload.recovery,
        Some(crate::babysit::BabysitRecoveryAction::Retry)
    );
}

#[tokio::test]
async fn capability_probe_accepts_restricted_structured_output_and_cleanup() {
    let (provider, _temp, events) = provider("complete", CODEX_ANALYSIS_TIMEOUT);

    provider
        .prepare_process(Path::new("/tmp/narrant-review-workspace"))
        .await
        .unwrap();

    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "thread/start:restricted"));
    assert!(events.lines().any(|line| line == "turn/start:sentinel"));
    assert!(events.lines().any(|line| line == "turn/interrupt:sentinel"));
    assert!(events.lines().any(|line| line == "thread/delete:thread-1"));
    assert!(!events.lines().any(|line| line == "source-in-input"));
    assert!(!events.lines().any(|line| line == "turn/start:real"));
    assert!(!events.lines().any(|line| line == "agent-message"));
}

#[tokio::test]
async fn capability_probe_ignores_unknown_notifications() {
    let (provider, _temp, events) = provider("unknown-notification", CODEX_ANALYSIS_TIMEOUT);

    provider
        .prepare_process(Path::new("/tmp/narrant-review-workspace"))
        .await
        .unwrap();

    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "thread/delete:thread-1"));
    assert!(!events.lines().any(|line| line == "turn/start:real"));
    assert!(!events.lines().any(|line| line == "agent-message"));
}

#[tokio::test]
async fn missing_interrupt_is_incompatible_but_probe_thread_is_deleted() {
    let (provider, _temp, events) = provider("missing-interrupt", CODEX_ANALYSIS_TIMEOUT);

    let error = provider
        .prepare_process(Path::new("/tmp/narrant-review-workspace"))
        .await
        .unwrap_err();

    assert_codex_setup_failure(&error, BabysitProviderSetupFailure::Incompatible);
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "thread/delete:thread-1"));
    assert!(events.lines().any(|line| line == "turn/interrupt:sentinel"));
    assert!(!events.lines().any(|line| line == "turn/start:real"));
    assert!(!events.lines().any(|line| line == "agent-message"));
}

#[tokio::test]
async fn missing_delete_is_incompatible_after_the_probe_turn() {
    let (provider, _temp, events) = provider("missing-delete", CODEX_ANALYSIS_TIMEOUT);

    let error = provider
        .prepare_process(Path::new("/tmp/narrant-review-workspace"))
        .await
        .unwrap_err();

    assert_codex_setup_failure(&error, BabysitProviderSetupFailure::Incompatible);
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "turn/start:sentinel"));
    assert!(events.lines().any(|line| line == "turn/interrupt:sentinel"));
    assert!(events.lines().any(|line| line == "thread/delete:thread-1"));
    assert!(!events.lines().any(|line| line == "turn/start:real"));
    assert!(!events.lines().any(|line| line == "agent-message"));
}

#[tokio::test]
async fn rejected_output_schema_is_incompatible_without_source_call() {
    let (provider, _temp, events) = provider("rejected-output-schema", CODEX_ANALYSIS_TIMEOUT);

    let error = provider
        .prepare_process(Path::new("/tmp/narrant-review-workspace"))
        .await
        .unwrap_err();

    assert_codex_setup_failure(&error, BabysitProviderSetupFailure::Incompatible);
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "thread/delete:thread-1"));
    assert!(events.lines().any(|line| line == "turn/start:sentinel"));
    assert!(!events.lines().any(|line| line == "turn/start:real"));
    assert!(!events.lines().any(|line| line == "agent-message"));
    assert!(!events.lines().any(|line| line == "source-in-input"));
}

#[tokio::test]
async fn rejected_permission_profile_is_incompatible_before_turn() {
    let (provider, _temp, events) = provider("rejected-permission", CODEX_ANALYSIS_TIMEOUT);

    let error = provider
        .prepare_process(Path::new("/tmp/narrant-review-workspace"))
        .await
        .unwrap_err();

    assert_codex_setup_failure(&error, BabysitProviderSetupFailure::Incompatible);
    let events = std::fs::read_to_string(events).unwrap();
    assert!(!events.lines().any(|line| line == "turn/start:sentinel"));
    assert!(!events.lines().any(|line| line == "turn/start:real"));
    assert!(!events.lines().any(|line| line == "agent-message"));
}

#[tokio::test]
async fn restricted_policy_probe_returns_upgrade_recovery_category() {
    let (provider, _temp, _events) = provider("insecure-policy", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();

    let error = provider
        .generate(request(), &cancellation)
        .await
        .unwrap_err();

    assert_codex_setup_failure(&error, BabysitProviderSetupFailure::Incompatible);
}

#[tokio::test]
async fn malformed_structured_output_can_be_repaired_on_the_same_thread() {
    let (provider, _temp, events) = provider("invalid-json-repair", CODEX_ANALYSIS_TIMEOUT);
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
            vec![
                ValidationFailure::MissingLine {
                    side: FileSide::Before,
                    line_number: 1,
                },
                ValidationFailure::MissingLine {
                    side: FileSide::After,
                    line_number: 1,
                },
            ],
            &cancellation,
        )
        .await
        .unwrap();
    assert_eq!(repaired.after["1"], "after explanation");
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "json-error-feedback"));
    assert!(events.lines().any(|line| line == "repair-numbered-content"));
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
            .repair(request.clone(), output, vec![failure], &cancellation)
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
    wait_for_active_thread(&provider).await;
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
    // 事件文件先于客户端收到响应；等 ownership 落入 active map 后再取消。
    wait_for_active_thread(&provider).await;
    worker.abort();
    let _ = worker.await;

    wait_for_event(&events, "thread/delete:thread-1").await;
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "turn/interrupt"));
}

#[tokio::test]
async fn dropping_during_delete_keeps_cleanup_guard_armed_until_confirmation() {
    let (provider, _temp, events) = provider("delayed-delete", CODEX_ANALYSIS_TIMEOUT);
    let provider = Arc::new(provider);
    let worker_provider = provider.clone();
    let worker = tokio::spawn(async move {
        let cancellation = OperationContext::background().cancellation().clone();
        worker_provider.generate(request(), &cancellation).await
    });
    wait_for_event(&events, "thread/delete:thread-1").await;
    worker.abort();
    let _ = worker.await;

    for _ in 0..1000 {
        let count = std::fs::read_to_string(&events)
            .map(|value| {
                value
                    .lines()
                    .filter(|line| *line == "thread/delete:thread-1")
                    .count()
            })
            .unwrap_or_default();
        if count >= 2 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("cleanup guard did not retry an unconfirmed delete");
}

#[test]
fn dropping_cleanup_guard_without_runtime_removes_ownership_and_poisons_generation() {
    let process = Arc::new(tokio::sync::Mutex::new(None));
    let active = Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    let failed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let run = ActiveThread {
        thread_id: "thread-1".to_owned(),
        turn_id: "turn-1".to_owned(),
        repair_attempts: 0,
        deadline: std::time::Instant::now() + Duration::from_secs(1),
        malformed_output: false,
    };
    active.lock().unwrap().insert("key".to_owned(), run.clone());

    let guard = RunCleanupGuard::new(
        process,
        active.clone(),
        failed.clone(),
        "key".to_owned(),
        run,
    );
    drop(guard);

    assert!(!active.lock().unwrap().contains_key("key"));
    assert!(failed.load(std::sync::atomic::Ordering::Acquire));
}

#[tokio::test]
async fn total_deadline_fails_and_cleans_up_without_partial_output() {
    let (provider, _temp, events) = provider("hang", Duration::from_secs(15));
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

    assert_codex_setup_failure(&error, BabysitProviderSetupFailure::Incompatible);
    let events = std::fs::read_to_string(events).unwrap();
    assert!(!events.lines().any(|line| line == "turn/start"));
    assert!(events.lines().any(|line| line == "thread/delete:thread-1"));
}

#[tokio::test]
async fn personal_model_instruction_file_is_allowed_by_environment_reuse() {
    let (provider, _temp, events) = provider("personal-model-instructions", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();
    let output = provider.generate(request(), &cancellation).await.unwrap();

    assert_eq!(output.after["1"], "after explanation");
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "thread/start"));
}

#[tokio::test]
async fn managed_instruction_requirement_is_allowed_by_environment_reuse() {
    let (provider, _temp, events) = provider("managed-instructions", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();
    let output = provider.generate(request(), &cancellation).await.unwrap();

    assert_eq!(output.after["1"], "after explanation");
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.lines().any(|line| line == "thread/start"));
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
async fn lost_thread_start_response_restarts_and_cleans_marker_orphan_before_reuse() {
    let (provider, _temp, events) = provider("lost-thread-response", CODEX_ANALYSIS_TIMEOUT);
    let cancellation = OperationContext::background().cancellation().clone();
    let first = provider.generate(request(), &cancellation).await;
    assert!(matches!(first, Err(BabysitError::Provider { .. })));

    let output = provider.generate(request(), &cancellation).await.unwrap();
    assert_eq!(output.after["1"], "after explanation");
    let events = std::fs::read_to_string(events).unwrap();
    assert_eq!(
        events.lines().filter(|line| *line == "initialize").count(),
        2
    );
    let orphan_deleted = events
        .lines()
        .position(|line| line == "thread/delete:orphan-thread")
        .unwrap();
    let lines = events.lines().collect::<Vec<_>>();
    let second_turn = lines
        .iter()
        .rposition(|line| *line == "turn/start")
        .unwrap();
    assert!(orphan_deleted < second_turn);
}

#[tokio::test]
async fn blocked_stdin_writer_obeys_the_analysis_deadline() {
    let (provider, _temp, events) = provider("block-writer", Duration::from_secs(2));
    let cancellation = OperationContext::background().cancellation().clone();
    let mut request = request();
    request.before = "x".repeat(2 * 1024 * 1024);
    request.after = "y".repeat(2 * 1024 * 1024);
    let error = provider.generate(request, &cancellation).await.unwrap_err();

    assert!(matches!(error, BabysitError::Provider { .. }));
    let _ = events;
}
