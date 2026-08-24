use super::super::{
    BabysitAgentProvider, BabysitProviderRequest, BabysitProviderSetupFailure, ClaudeCodeProvider,
    FileSide, LineTarget,
};
use crate::operation::OperationContext;
use crate::operation::TestCancellation;
use std::path::PathBuf;
use std::time::Duration;

/// 断言事件日志中某个 CLI flag 后紧跟指定值，并兼容平台换行符。
fn assert_flag_value(events: &str, flag: &str, value: &str) {
    let lines: Vec<_> = events.lines().collect();
    assert!(
        lines.windows(2).any(|pair| pair == [flag, value]),
        "expected {flag} followed by {value}; events={lines:?}"
    );
}

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
        model: "sonnet".to_owned(),
        reasoning_effort: "low".to_owned(),
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
    assert_flag_value(&events, "--model", "sonnet");
    assert_flag_value(&events, "--effort", "low");
    assert!(events.contains("--safe-mode"));
    assert_flag_value(&events, "--tools", "Read,Glob,Grep");
    assert_flag_value(&events, "--disallowedTools", "Bash,Write,Edit,Web,mcp__*");
    assert_flag_value(&events, "--permission-mode", "dontAsk");
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
async fn generic_nonzero_exit_returns_only_the_redacted_process_category() {
    let (provider, temp, _) = provider("process_exit", Duration::from_secs(5));
    let workspace = temp.path().join("private-workspace-sentinel");
    std::fs::create_dir(&workspace).unwrap();
    let error = provider
        .generate(
            request(workspace.to_string_lossy().into_owned()),
            OperationContext::background().cancellation(),
        )
        .await
        .unwrap_err();
    let message = error.to_string();

    assert_eq!(
        message,
        "宝宝模式 provider 失败：Claude Code 进程失败（process_exit）"
    );
    assert!(!message.contains("private-stdout-sentinel"));
    assert!(!message.contains("private-stderr-sentinel"));
    assert!(!message.contains(workspace.to_string_lossy().as_ref()));
}

#[tokio::test]
async fn timeout_discards_partial_output_and_terminates_the_process() {
    let (provider, temp, events) = provider("hang", Duration::from_millis(100));
    let workspace = temp.path().to_string_lossy().into_owned();
    let worker = tokio::spawn(async move {
        provider
            .generate(
                request(workspace),
                OperationContext::background().cancellation(),
            )
            .await
    });
    let descendant = wait_for_fixture_descendant(&events).await;
    let error = worker.await.unwrap().unwrap_err();
    assert!(matches!(error, super::super::BabysitError::Provider { .. }));
    assert!(error.to_string().contains("timeout"));
    assert_process_terminated(descendant, "timeout").await;
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
    let descendant = wait_for_fixture_descendant(&events).await;
    cancellation.cancel();
    let error = worker.await.unwrap().unwrap_err();
    assert_eq!(error, super::super::BabysitError::Cancelled);
    assert_process_terminated(descendant, "cancellation").await;
}

/// 等待 fixture 报告其后代 PID，避免测试与进程启动时序耦合。
async fn wait_for_fixture_descendant(events: &std::path::Path) -> u32 {
    for _ in 0..100 {
        if let Some(descendant) = read_fixture_descendant(events) {
            return descendant;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("fixture should report its descendant process");
}

/// 解析 fixture 写入的后代 PID。
fn read_fixture_descendant(events: &std::path::Path) -> Option<u32> {
    std::fs::read_to_string(events).ok().and_then(|value| {
        value
            .lines()
            .find_map(|line| line.strip_prefix("child:"))?
            .parse::<u32>()
            .ok()
    })
}

/// 轮询确认指定 fixture 后代已经被进程树清理逻辑终止。
async fn assert_process_terminated(descendant: u32, trigger: &str) {
    for _ in 0..100 {
        if !process_is_alive(descendant) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("Claude descendant process {descendant} survived {trigger}");
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
