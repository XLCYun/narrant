#![cfg(unix)]

use super::RustLsp;
use crate::lsp::{test_support::FakeServer, Position};
use serde_json::{json, Value};

/// Rust Adapter 所需能力，通用客户端测试无需这些字段。
fn capabilities() -> Value {
    json!({"definitionProvider":true, "callHierarchyProvider":true, "experimental":{"parentModule":true}})
}

/// 标准握手结果在 Rust Adapter 中按分析需求校验。
fn initialized(capabilities: Value) -> Value {
    json!({"jsonrpc":"2.0", "id":1, "result":{"capabilities":capabilities}})
}

/// 健康度和就绪条件属于 Rust 扩展。
fn status(health: &str) -> Value {
    json!({"jsonrpc":"2.0", "method":"experimental/serverStatus", "params":{
        "quiescent":true, "health":health, "message":"test project loading"
    }})
}

/// 缺少必要能力或位置编码不匹配时应失败并回收服务器。
#[test]
fn requires_rust_capabilities_and_utf16() {
    let mut missing_parent = capabilities();
    missing_parent["experimental"] = json!({});
    let mut missing_definition = capabilities();
    missing_definition["definitionProvider"] = json!(false);
    let mut utf8 = capabilities();
    utf8["positionEncoding"] = json!("utf-8");
    for capabilities in [missing_parent, missing_definition, utf8] {
        let server = FakeServer::new(vec![vec![initialized(capabilities)], vec![]]);
        assert!(RustLsp::start(&server.workspace().join("server"), server.workspace()).is_err());
        // SAFETY: 信号 0 只查询本测试服务器是否仍存在。
        assert_eq!(unsafe { libc::kill(server.pid() as i32, 0) }, -1);
    }
}

/// warning 也可能是依赖或宏加载失败，不能进入构图阶段。
#[test]
fn rejects_degraded_project_loading() {
    for health in ["warning", "error"] {
        let server = FakeServer::new(vec![
            vec![initialized(capabilities())],
            vec![status(health)],
        ]);
        let mut client =
            RustLsp::start(&server.workspace().join("server"), server.workspace()).unwrap();
        assert!(client
            .wait_ready()
            .unwrap_err()
            .message
            .contains("项目未完整加载"));
        drop(client);
        // SAFETY: 信号 0 只查询本测试服务器是否仍存在。
        assert_eq!(unsafe { libc::kill(server.pid() as i32, 0) }, -1);
    }
}

/// 通过对端收到的消息验证 Rust 配置、语言名称与就绪顺序。
#[test]
fn configures_rust_and_only_queries_after_loading() {
    let server = FakeServer::new(vec![
        vec![initialized(capabilities())],
        vec![],
        vec![status("ok")],
        vec![json!({"jsonrpc":"2.0", "id":2, "result":[]})],
        vec![json!({"jsonrpc":"2.0", "id":3, "result":null})],
    ]);
    let mut client =
        RustLsp::start(&server.workspace().join("server"), server.workspace()).unwrap();
    assert!(client
        .definitions("file:///lib.rs", Position::default())
        .unwrap_err()
        .message
        .contains("尚未就绪"));
    client.open("file:///lib.rs", "fn f() {}").unwrap();
    client.wait_ready().unwrap();
    assert!(client.open("file:///late.rs", "").is_err());
    assert!(client
        .definitions("file:///lib.rs", Position::default())
        .unwrap()
        .is_empty());
    client.shutdown().unwrap();
    let requests = server.requests();
    assert_eq!(
        requests[0]["params"]["initializationOptions"]["checkOnSave"],
        false
    );
    assert_eq!(
        requests[0]["params"]["capabilities"]["experimental"]["serverStatusNotification"],
        true
    );
    assert_eq!(requests[2]["params"]["textDocument"]["languageId"], "rust");
}

/// 查询中的成功响应不能掩盖期间到达的降级通知。
#[test]
fn rust_extension_queries_check_status_changes() {
    let server = FakeServer::new(vec![
        vec![initialized(capabilities())],
        vec![status("ok")],
        vec![
            status("warning"),
            json!({"jsonrpc":"2.0", "id":2, "result":[]}),
        ],
    ]);
    let mut client =
        RustLsp::start(&server.workspace().join("server"), server.workspace()).unwrap();
    client.wait_ready().unwrap();
    assert!(client
        .parent_modules("file:///lib.rs")
        .unwrap_err()
        .message
        .contains("项目未完整加载"));
    assert_eq!(server.requests()[2]["method"], "experimental/parentModule");
}
