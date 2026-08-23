use super::super::{
    BabysitAgentProvider, BabysitProviderRequest, BabysitProviderSetupFailure, ClaudeCodeProvider,
    FileSide, LineTarget,
};
use crate::operation::OperationContext;
use crate::operation::TestCancellation;
use std::path::PathBuf;
use std::time::Duration;

/// 构造同时包含 before/after 目标的 provider 请求。
fn request(workspace_path: String) -> BabysitProviderRequest {
    BabysitProviderRequest {
        path: "src/example.rs".to_owned(),
        locale: "zh-CN".to_owned(),
        workspace_path,
        before: "let old = 1;".to_owned(),
        after: "let next = 2;".to_owned(),
        targets: vec![
            LineTarget {
                side: FileSide::Before,
                line_number: 1,
                source: "let old = 1;".to_owned(),
            },
            LineTarget {
                side: FileSide::After,
                line_number: 1,
                source: "let next = 2;".to_owned(),
            },
        ],
    }
}

#[tokio::test]
async fn structured_output_uses_non_persistent_read_only_claude_process() {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/babysit/tests/fixtures/fake_claude.sh");
    let temp = tempfile::tempdir().unwrap();
    let events = temp.path().join("events.log");
    let provider =
        ClaudeCodeProvider::for_test(fixture, "success", events.clone(), Duration::from_secs(5));

    let output = provider
        .generate(
            request(temp.path().to_string_lossy().into_owned()),
            OperationContext::background().cancellation(),
        )
        .await
        .unwrap();

    assert_eq!(output.before["1"], "旧值初始化");
    assert_eq!(output.after["1"], "新值初始化");
    let events = std::fs::read_to_string(events).unwrap();
    assert!(events.contains("--no-session-persistence"));
    assert!(events.contains("--model\nsonnet"));
    assert!(events.contains("--effort\nlow"));
    assert!(events.contains("--safe-mode"));
    assert!(events.contains("--tools\nRead,Glob,Grep"));
    assert!(events.contains("--disallowedTools\nBash,Write,Edit,Web,mcp__*"));
    assert!(events.contains("--permission-mode\ndontAsk"));
    assert!(!events.contains("--resume"));
}

/// 构造指定 scenario 的确定性 provider 与事件文件。
fn provider(scenario: &str, timeout: Duration) -> (ClaudeCodeProvider, tempfile::TempDir, PathBuf) {
    let fixture =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/babysit/tests/fixtures/fake_claude.sh");
    let temp = tempfile::tempdir().unwrap();
    let events = temp.path().join("events.log");
    (
        ClaudeCodeProvider::for_test(fixture, scenario, events.clone(), timeout),
        temp,
        events,
    )
}

#[tokio::test]
async fn malformed_generation_is_repaired_by_a_fresh_self_contained_process() {
    let (provider, temp, events) = provider("repair", Duration::from_secs(5));
    let cancellation = OperationContext::background().cancellation().clone();
    let request = request(temp.path().to_string_lossy().into_owned());
    let previous = provider
        .generate(request.clone(), &cancellation)
        .await
        .unwrap();

    let output = provider
        .repair(
            request,
            previous,
            super::super::ValidationFailure::MissingLine {
                side: FileSide::Before,
                line_number: 1,
            },
            &cancellation,
        )
        .await
        .unwrap();

    assert_eq!(output.before["1"], "旧值初始化");
    let events = std::fs::read_to_string(events).unwrap();
    assert_eq!(events.matches("round:").count(), 2);
    assert_eq!(events.matches("--no-session-persistence").count(), 2);
    assert!(!events.contains("--resume"));
    assert!(events.contains("let old = 1;"));
    assert!(events.contains("上一轮输出 JSON"));
    assert!(events.contains("Before 缺少第 1 行解释"));
}

#[tokio::test]
async fn malformed_cli_json_returns_only_a_redacted_category() {
    let (provider, temp, _) = provider("malformed", Duration::from_secs(5));
    let error = provider
        .generate(
            request(temp.path().to_string_lossy().into_owned()),
            OperationContext::background().cancellation(),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, super::super::BabysitError::Provider { .. }));
    assert!(!error.to_string().contains("not json"));
    assert!(!error
        .to_string()
        .contains(temp.path().to_string_lossy().as_ref()));
}

#[tokio::test]
async fn auth_and_incompatible_results_use_stable_setup_categories() {
    for (scenario, expected) in [
        (
            "auth",
            super::super::BabysitError::provider_setup(
                "claude_code",
                BabysitProviderSetupFailure::SignedOut,
            ),
        ),
        (
            "incompatible",
            super::super::BabysitError::provider_setup(
                "claude_code",
                BabysitProviderSetupFailure::Incompatible,
            ),
        ),
    ] {
        let (provider, temp, _) = provider(scenario, Duration::from_secs(5));
        let error = provider
            .generate(
                request(temp.path().to_string_lossy().into_owned()),
                OperationContext::background().cancellation(),
            )
            .await
            .unwrap_err();
        assert_eq!(error, expected);
    }
}

#[tokio::test]
async fn timeout_discards_partial_output_and_terminates_the_process() {
    let (provider, temp, _) = provider("hang", Duration::from_millis(100));
    let error = provider
        .generate(
            request(temp.path().to_string_lossy().into_owned()),
            OperationContext::background().cancellation(),
        )
        .await
        .unwrap_err();
    assert!(matches!(error, super::super::BabysitError::Provider { .. }));
    assert!(error.to_string().contains("timeout"));
}

#[tokio::test]
async fn cancellation_discards_output_and_terminates_the_process() {
    let (provider, temp, events) = provider("hang", Duration::from_secs(5));
    let cancellation = TestCancellation::default();
    let worker_cancellation = cancellation.clone();
    let workspace = temp.path().to_string_lossy().into_owned();
    let worker = tokio::spawn(async move {
        provider
            .generate(request(workspace), &worker_cancellation)
            .await
    });
    let mut descendant = None;
    for _ in 0..100 {
        descendant = std::fs::read_to_string(&events).ok().and_then(|value| {
            value
                .lines()
                .find_map(|line| line.strip_prefix("child:"))?
                .parse::<u32>()
                .ok()
        });
        if descendant.is_some() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    cancellation.cancel();
    let error = worker.await.unwrap().unwrap_err();
    assert_eq!(error, super::super::BabysitError::Cancelled);
    let descendant = descendant.expect("fixture should report its descendant process");
    for _ in 0..100 {
        if !process_is_alive(descendant) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("Claude descendant process {descendant} survived cancellation");
}

/// 查询 fixture 后代是否仍存活，仅用于验证进程树清理这一系统边界。
fn process_is_alive(process_id: u32) -> bool {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {process_id}"), "/NH"])
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).contains(&process_id.to_string())
    }
    #[cfg(unix)]
    {
        // SAFETY: signal 0 不修改进程，只查询该 fixture PID 是否存在。
        unsafe { libc::kill(process_id as i32, 0) == 0 }
    }
}
