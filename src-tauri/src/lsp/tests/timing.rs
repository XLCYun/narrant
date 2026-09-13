use crate::lsp::{test_support::FakeServer, LspClient, LspOptions};
use serde_json::json;
use std::{
    io::Write,
    process::Command,
    sync::{Arc, Mutex},
};

/// 测试独享的终端写入端，用于验证公开会话输出，不安装全局 subscriber。
struct CapturedLog(Arc<Mutex<Vec<u8>>>);

impl Write for CapturedLog {
    /// 保留实际 formatter 输出，断言秘密没有经过任何日志路径泄漏。
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Ok(bytes.len())
    }

    /// 内存写入无需额外刷新。
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// 请求失败也进入计数；交错进度通知不改变响应，参数、正文与进度详情均不输出。
#[test]
fn summarizes_real_requests_without_logging_payloads() {
    // tracing 的 callsite interest 在进程内共享；隔离其他并发测试的无 subscriber 会话。
    const CHILD: &str = "NARRANT_LSP_TIMING_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "lsp::tests::timing::summarizes_real_requests_without_logging_payloads",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success()
                && String::from_utf8_lossy(&output.stdout).contains("1 passed;"),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let server = FakeServer::new(vec![
        vec![json!({"jsonrpc":"2.0","id":1,"result":{"capabilities":{}}})],
        vec![],
        vec![],
        vec![
            json!({"jsonrpc":"2.0","method":"$/progress","params":{"token":"private-token","value":{"kind":"begin","title":"Indexing","message":"DO_NOT_LOG"}}}),
            json!({"jsonrpc":"2.0","method":"$/progress","params":{"token":"private-token","value":{"kind":"report","message":"DO_NOT_LOG"}}}),
            json!({"jsonrpc":"2.0","method":"$/progress","params":{"token":"private-token","value":{"kind":"end","message":"DO_NOT_LOG"}}}),
            json!({"jsonrpc":"2.0","id":2,"result":{"body":"DO_NOT_LOG"}}),
        ],
        vec![json!({"jsonrpc":"2.0","id":3,"error":{"code":-1,"message":"DO_NOT_LOG"}})],
        vec![json!({"jsonrpc":"2.0","id":4,"result":null})],
    ]);
    let output = Arc::new(Mutex::new(Vec::new()));
    let writer = output.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || CapturedLog(writer.clone()))
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        let mut client =
            LspClient::start(server.command(), server.workspace(), LspOptions::default()).unwrap();
        client
            .open_document("file:///PRIVATE_PATH", "rust", 1, "DO_NOT_LOG")
            .unwrap();
        assert_eq!(
            client
                .request("example/inspect", json!({"secret":"DO_NOT_LOG"}))
                .unwrap(),
            json!({"body":"DO_NOT_LOG"})
        );
        assert!(client.request("example/inspect", json!({})).is_err());
        client.shutdown().unwrap();
    });
    let output = String::from_utf8(output.lock().unwrap().clone()).unwrap();
    let summary = output
        .lines()
        .find(|line| line.contains("request_summary") && line.contains("example/inspect"))
        .unwrap_or_else(|| panic!("没有请求汇总：{output}"));
    assert!(summary.contains("requests=2"), "{output}");
    assert!(summary.contains("failed=1"), "{output}");
    for metric in ["total_ms=", "max_ms=", "mean_ms="] {
        assert!(summary.contains(metric));
    }
    assert!(output.contains("server_progress_started"));
    assert!(output.contains("server_progress_finished"));
    for private in ["DO_NOT_LOG", "PRIVATE_PATH", "private-token"] {
        assert!(!output.contains(private), "{output}");
    }
}
