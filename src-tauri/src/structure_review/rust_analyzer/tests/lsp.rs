#![cfg(unix)]

use super::RustLsp;
use crate::lsp::{test_support::FakeServer, Position};
use serde_json::{json, Value};

/// 两个扩展档位在真实 stdio 握手中打开标准库，且仅完整档加载项目依赖。
#[test]
fn selects_standard_library_and_dependency_loading_independently() {
    use super::RustDependencyScope;
    for scope in [
        RustDependencyScope::StandardLibrary,
        RustDependencyScope::AllDependencies,
    ] {
        let server = FakeServer::new(vec![
            vec![initialized(capabilities())],
            vec![status("ok")],
            vec![json!({"jsonrpc":"2.0", "id":2, "result":null})],
        ]);
        let mut client = RustLsp::start(
            &server.workspace().join("server"),
            server.workspace(),
            std::sync::Arc::new(|| false),
            scope,
        )
        .unwrap();
        client.wait_ready().unwrap();
        client.shutdown().unwrap();
        let requests = server.requests();
        let options = &requests[0]["params"]["initializationOptions"];
        assert_eq!(options["cargo"]["sysroot"], "discover");
        assert_eq!(
            options["cargo"]["noDeps"],
            scope != RustDependencyScope::AllDependencies
        );
        assert_eq!(options["cargo"]["buildScripts"]["enable"], false);
        assert_eq!(options["procMacro"]["enable"], false);
    }
}

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
        assert!(RustLsp::start(
            &server.workspace().join("server"),
            server.workspace(),
            std::sync::Arc::new(|| false),
            super::RustDependencyScope::default()
        )
        .is_err());
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
        let mut client = RustLsp::start(
            &server.workspace().join("server"),
            server.workspace(),
            std::sync::Arc::new(|| false),
            super::RustDependencyScope::default(),
        )
        .unwrap();
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
    // 只记录分析策略相关的环境变量，验证限制实际传给子进程。
    let executable = server.workspace().join("server");
    let script = std::fs::read_to_string(&executable).unwrap();
    std::fs::write(
        &executable,
        script.replacen(
            "#!/bin/sh\n",
            "#!/bin/sh\nprintf '%s\\n' \"${CARGO_NET_OFFLINE-unset}\" \"${RUSTUP_AUTO_INSTALL-unset}\" > analysis-environment\n",
            1,
        ),
    )
    .unwrap();
    let mut client = RustLsp::start(
        &server.workspace().join("server"),
        server.workspace(),
        std::sync::Arc::new(|| false),
        super::RustDependencyScope::default(),
    )
    .unwrap();
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
    let configuration = &requests[0]["params"]["initializationOptions"];
    assert_eq!(configuration["cargo"]["noDeps"], true);
    assert_eq!(configuration["cargo"].get("sysroot"), Some(&Value::Null));
    assert_eq!(configuration["cargo"]["buildScripts"]["enable"], false);
    assert_eq!(configuration["procMacro"]["enable"], false);
    assert_eq!(
        std::fs::read_to_string(server.workspace().join("analysis-environment")).unwrap(),
        "true\n0\n"
    );
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
    let mut client = RustLsp::start(
        &server.workspace().join("server"),
        server.workspace(),
        std::sync::Arc::new(|| false),
        super::RustDependencyScope::default(),
    )
    .unwrap();
    client.wait_ready().unwrap();
    assert!(client
        .parent_modules("file:///lib.rs")
        .unwrap_err()
        .message
        .contains("项目未完整加载"));
    assert_eq!(server.requests()[2]["method"], "experimental/parentModule");
}
