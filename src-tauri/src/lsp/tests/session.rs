use crate::lsp::{test_support::FakeServer, LspClient, LspOptions, Position};
use serde_json::{json, Value};
use std::time::Duration;

/// 通用握手无需任何 Rust 扩展或语义查询能力。
fn initialized() -> Value {
    json!({"jsonrpc":"2.0", "id":1, "result":{"capabilities":{}}})
}

/// 为临时可执行文件的首次启动留出余量，同时缩短生产默认超时。
fn options() -> LspOptions {
    LspOptions {
        request_timeout: Duration::from_secs(5),
        ..LspOptions::default()
    }
}

/// Python 文档与扩展通知复用通用会话，反向请求不会吞掉正常响应。
#[test]
fn generic_session_handles_another_language_and_interleaved_messages() {
    let server = FakeServer::new(vec![
        vec![initialized()],
        vec![],
        vec![],
        vec![
            json!({"jsonrpc":"2.0", "id":"progress", "method":"window/workDoneProgress/create", "params":{"token":1}}),
            json!({"jsonrpc":"2.0", "id":"unknown", "method":"example/unsupported", "params":{}}),
            json!({"jsonrpc":"2.0", "method":"python/projectState", "params":{"loaded":true}}),
            json!({"jsonrpc":"2.0", "id":2, "result":{"echo":"中文😀"}}),
        ],
        vec![],
        vec![],
        vec![json!({"jsonrpc":"2.0", "id":3, "result":null})],
    ]);
    let mut configured = options();
    configured.initialization_options = json!({"exampleOption":true});
    let mut client = LspClient::start(server.command(), server.workspace(), configured).unwrap();
    assert_eq!(client.capabilities(), &json!({}));
    client
        .open_document("file:///module.py", "python", 7, "name = '中文😀'")
        .unwrap();
    assert_eq!(
        client.request("example/inspect", json!({})).unwrap(),
        json!({"echo":"中文😀"})
    );
    assert_eq!(
        client.latest_notification("python/projectState"),
        Some(&json!({"loaded":true}))
    );
    client.shutdown().unwrap();
    let requests = server.requests();
    assert_eq!(requests[0]["method"], "initialize");
    assert_eq!(
        requests[0]["params"]["initializationOptions"],
        json!({"exampleOption":true})
    );
    assert_eq!(requests[1]["method"], "initialized");
    assert_eq!(
        requests[2]["params"]["textDocument"]["languageId"],
        "python"
    );
    assert_eq!(requests[2]["params"]["textDocument"]["version"], 7);
    assert_eq!(
        requests[2]["params"]["textDocument"]["text"],
        "name = '中文😀'"
    );
    assert_eq!(
        requests[4],
        json!({"jsonrpc":"2.0", "id":"progress", "result":null})
    );
    assert_eq!(requests[5]["error"]["code"], -32601);
    assert_eq!(requests[6]["method"], "shutdown");
    // SAFETY: 信号 0 只查询本测试服务器是否已回收。
    assert_eq!(unsafe { libc::kill(server.pid() as i32, 0) }, -1);
}

/// 两种标准定义位置统一输出，null 保持成功的空查询语义。
#[test]
fn normalizes_standard_definition_responses() {
    let server = FakeServer::new(vec![
        vec![initialized()],
        vec![],
        vec![json!({
            "jsonrpc":"2.0", "id":2, "result":[
                {"targetUri":"file:///a.py", "targetSelectionRange":{"start":{"line":3,"character":4}}},
                {"uri":"file:///b.py", "range":{"start":{"line":6,"character":7}}}
            ]
        })],
        vec![json!({"jsonrpc":"2.0", "id":3, "result":null})],
    ]);
    let mut client = LspClient::start(server.command(), server.workspace(), options()).unwrap();
    let locations = client
        .definitions(
            "file:///source.py",
            Position {
                line: 1,
                character: 9,
            },
        )
        .unwrap();
    assert_eq!(locations.len(), 2);
    assert_eq!(locations[0].uri, "file:///a.py");
    assert_eq!(
        locations[0].position,
        Position {
            line: 3,
            character: 4
        }
    );
    assert_eq!(
        locations[1].position,
        Position {
            line: 6,
            character: 7
        }
    );
    assert!(client
        .definitions("file:///source.py", Position::default())
        .unwrap()
        .is_empty());
    assert_eq!(
        server.requests()[2]["params"]["position"],
        json!({"line":1,"character":9})
    );
}

/// 第二步调用查询完整带回服务器提供的 data 和扩展字段。
#[test]
fn outgoing_calls_preserve_opaque_prepared_items() {
    let range = json!({"start":{"line":2,"character":3}, "end":{"line":2,"character":7}});
    let item = json!({"uri":"file:///source.ts", "name":"source", "kind":12,
        "range":range, "selectionRange":range, "data":{"opaque":[1,"中"]}, "futureField":true});
    let server = FakeServer::new(vec![
        vec![initialized()],
        vec![],
        vec![json!({"jsonrpc":"2.0", "id":2, "result":[item]})],
        vec![json!({"jsonrpc":"2.0", "id":3, "result":[{"to":{
            "uri":"file:///target.ts", "name":"target", "selectionRange":range
        }, "fromRanges":[range]}]})],
    ]);
    let mut client = LspClient::start(server.command(), server.workspace(), options()).unwrap();
    let calls = client
        .outgoing_calls("file:///source.ts", Position::default())
        .unwrap();
    assert_eq!(calls[0].to.name, "target");
    assert_eq!(
        calls[0].from_ranges[0].start,
        Position {
            line: 2,
            character: 3
        }
    );
    let requests = server.requests();
    assert_eq!(requests[2]["method"], "textDocument/prepareCallHierarchy");
    assert_eq!(requests[3]["method"], "callHierarchy/outgoingCalls");
    assert_eq!(requests[3]["params"]["item"], item);
}

/// Adapter 解释自己的状态通知，通用层只等待并保留最新值。
#[test]
fn extension_notifications_are_observed_during_waits_and_queries() {
    let notice =
        |ready| json!({"jsonrpc":"2.0", "method":"example/state", "params":{"ready":ready}});
    let server = FakeServer::new(vec![
        vec![initialized()],
        vec![notice(false), notice(true)],
        vec![notice(false), json!({"jsonrpc":"2.0", "id":2, "result":42})],
    ]);
    let mut client = LspClient::start(server.command(), server.workspace(), options()).unwrap();
    client
        .wait_for_notification("example/state", Duration::from_secs(5), |params| {
            params["ready"] == true
        })
        .unwrap();
    assert_eq!(client.request("example/query", json!({})).unwrap(), 42);
    assert_eq!(
        client.latest_notification("example/state"),
        Some(&json!({"ready":false}))
    );
}

/// 服务器错误、畸形位置和超时均不能伪装成空列表。
#[test]
fn query_failures_are_explicit() {
    let server = FakeServer::new(vec![
        vec![initialized()],
        vec![],
        vec![json!({"jsonrpc":"2.0", "id":2, "error":{"code":-32603,"message":"query failed"}})],
        vec![
            json!({"jsonrpc":"2.0", "id":3, "result":{"uri":"file:///bad", "range":{"start":{"line":-1,"character":0}}}}),
        ],
        vec![],
    ]);
    let mut client = LspClient::start(server.command(), server.workspace(), options()).unwrap();
    assert!(client
        .request("example/fail", json!({}))
        .unwrap_err()
        .message
        .contains("query failed"));
    assert!(client
        .definitions("file:///bad", Position::default())
        .is_err());
    assert!(client
        .request("example/timeout", json!({}))
        .unwrap_err()
        .message
        .contains("example/timeout"));
}

/// 从公开启动入口覆盖无效帧和 EOF，失败路径也回收服务器。
#[test]
fn malformed_frames_and_eof_fail_and_reap_the_process() {
    let mut frames = vec![
        b"\r\n{}".to_vec(),
        b"Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}".to_vec(),
        b"Content-Length: 99\r\n\r\n{}".to_vec(),
        b"Content-Length: 2\r\n\r\nx!".to_vec(),
        b"Content-Length: 999999999\r\n\r\n".to_vec(),
        b"Content-Length: 2\r\n\r\n[]".to_vec(),
        Vec::new(),
    ];
    frames.push(format!("X-Long: {}\r\n\r\n", "x".repeat(20_000)).into_bytes());
    for frame in frames {
        let server = FakeServer::raw(vec![frame], false);
        assert!(LspClient::start(server.command(), server.workspace(), options()).is_err());
        // SAFETY: 信号 0 只查询本测试服务器是否仍存在。
        assert_eq!(unsafe { libc::kill(server.pid() as i32, 0) }, -1);
    }
}

/// 长时间不回应的服务器仍可及时取消，析构时回收真实服务器进程。
#[test]
fn cancellation_interrupts_a_pending_response_and_reaps_server() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    let server = FakeServer::new(vec![vec![initialized()], vec![], vec![]]);
    let revoked = Arc::new(AtomicBool::new(false));
    let observed = revoked.clone();
    let mut options = options();
    options.request_timeout = Duration::from_secs(30);
    options.cancelled = Arc::new(move || observed.load(Ordering::Acquire));
    let mut client = LspClient::start(server.command(), server.workspace(), options).unwrap();
    let requests_path = server.workspace().join("requests");
    let cancelling = std::thread::spawn(move || {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !std::fs::read_to_string(&requests_path)
            .unwrap_or_default()
            .contains("example/pending")
        {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        revoked.store(true, Ordering::Release);
    });
    let started = std::time::Instant::now();
    let error = client.request("example/pending", json!({})).unwrap_err();
    assert!(error.is_cancelled());
    assert!(error.message.contains("取消"));
    assert!(started.elapsed() < Duration::from_secs(5));
    cancelling.join().unwrap();
    drop(client);
    // SAFETY: 信号 0 只查询本测试启动的服务器。
    assert_eq!(unsafe { libc::kill(server.pid() as i32, 0) }, -1);
}

/// 真实后代进程必须在关闭返回或取消析构前消失，不能只检查直接服务器 PID。
#[test]
fn process_group_is_reclaimed_before_session_returns() {
    for normal_shutdown in [true, false] {
        let server = FakeServer::new(vec![
            vec![initialized()],
            vec![],
            vec![json!({"jsonrpc":"2.0", "id":2, "result":null})],
        ]);
        let executable = server.workspace().join("server");
        let script = std::fs::read_to_string(&executable).unwrap();
        std::fs::write(
            executable,
            script.replace(
                "set -eu\n",
                "set -eu\nsleep 600 &\nprintf '%s' \"$!\" > descendant-pid\n",
            ),
        )
        .unwrap();
        let client = LspClient::start(server.command(), server.workspace(), options()).unwrap();
        let descendant = std::fs::read_to_string(server.workspace().join("descendant-pid"))
            .unwrap()
            .parse::<i32>()
            .unwrap();
        // SAFETY: 信号 0 仅检查本测试创建的睡眠进程。
        assert_eq!(unsafe { libc::kill(descendant, 0) }, 0);
        if normal_shutdown {
            client.shutdown().unwrap();
        } else {
            drop(client);
        }
        // SAFETY: 守卫必须已等待该后代退出，不能让测试自己轮询掩盖回收缺口。
        assert_eq!(unsafe { libc::kill(descendant, 0) }, -1);
    }
}
