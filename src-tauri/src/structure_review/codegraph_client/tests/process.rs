use super::super::tests::transport::fixture;
use super::*;

/// 冻结真实 stdout 读线程，先确认根退出，再同时放行读取和稍后取消。
fn exited_response_then_cancel(
    scenario: &str,
) -> crate::structure_review::code_graph::AnalysisError {
    let (_pack, _repository, mut analyzer, input) = fixture(scenario);
    let (release, gate) = mpsc::channel();
    let gate = std::sync::Mutex::new(gate);
    analyzer.test_stdout_gate = Some(Arc::new(move || {
        gate.lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
    }));
    let release = std::sync::Mutex::new(Some(release));
    analyzer.cancelled = Arc::new(move || {
        if let Some(release) = release.lock().unwrap().take() {
            release.send(()).unwrap();
        }
        true
    });
    let scratch = tempfile::tempdir().unwrap();
    let request = serde_json::json!({"protocol": protocol::PROTOCOL, "type":"analyze", "id":protocol::REQUEST_ID,
        "repository_root":input.repository_root, "files":["main.ts"], "options":{}});
    let mut session = Session::start(&analyzer, &request, scratch.path()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if session.process.status().unwrap().is_some() {
            break;
        }
        assert!(Instant::now() < deadline, "真实错误夹具必须先退出");
        std::thread::yield_now();
    }
    let error = exchange(&mut session, &analyzer).unwrap_err();
    session.finish().unwrap();
    error
}

/// 根真实退出但 stdout 尚未被调度读取时，稍后取消不能覆盖已经写入管道的公共失败。
#[test]
fn exited_backend_error_is_drained_before_later_cancellation() {
    let error = exited_response_then_cancel("error-null");
    assert!(!error.is_cancelled(), "{error}");
    assert!(
        error.message.contains("invalid_request：fixture failure"),
        "{error}"
    );
}

/// 排空退出后的管道仍严格验证身份、UTF-8 和完整响应，不能借取消掩盖坏消息。
#[test]
fn exited_invalid_or_missing_output_still_fails_after_cancellation() {
    for (scenario, expected) in [
        ("error-id", "固定身份"),
        ("utf8", "line 1 column 1"),
        ("crash", "completed"),
        ("empty-diagnostic", "fixture diagnostic"),
    ] {
        let error = exited_response_then_cancel(scenario);
        assert!(!error.is_cancelled(), "{scenario}: {error}");
        assert!(error.message.contains(expected), "{scenario}: {error}");
    }
}

/// 有效成功图既不能覆盖已发生的取消，也不能使非零退出被接受。
#[test]
fn exited_success_cannot_override_cancellation_or_nonzero_status() {
    assert!(exited_response_then_cancel("valid").is_cancelled());
    let error = exited_response_then_cancel("exit");
    assert!(!error.is_cancelled(), "{error}");
    assert!(error.message.contains("异常退出"), "{error}");
}
