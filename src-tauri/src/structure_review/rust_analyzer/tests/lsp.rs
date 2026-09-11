use std::io::Cursor;

use serde_json::json;

use super::read_message;

/// 多帧和多字节内容必须按字节边界读取，后一个响应不能被吞掉。
#[test]
fn reads_utf8_and_consecutive_frames() {
    let messages = [json!({"result":"中文😀"}), json!({"result":null})];
    let bytes = messages.iter().map(|message| {
        let body = message.to_string();
        format!("Content-Length: {}\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{body}", body.len())
    }).collect::<String>();
    let mut input = Cursor::new(bytes);
    assert_eq!(read_message(&mut input).unwrap(), messages[0]);
    assert_eq!(read_message(&mut input).unwrap(), messages[1]);
    assert!(read_message(&mut input).is_err());
}

/// 不完整、重复长度和畸形正文都必须成为协议错误。
#[test]
fn rejects_invalid_frames() {
    for frame in [
        "\r\n{}",
        "Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}",
        "Content-Length: 99\r\n\r\n{}",
        "Content-Length: 2\r\n\r\nx!",
        "Content-Length: 999999999\r\n\r\n",
    ] {
        assert!(read_message(&mut Cursor::new(frame)).is_err(), "{frame}");
    }
}

/// 可控的 stdio 服务器验证加载警告和请求错误，避免仅覆盖正常解析结果。
#[cfg(unix)]
fn fake_server(
    health: &str,
    response: Option<serde_json::Value>,
) -> (tempfile::TempDir, super::Lsp) {
    use std::{fs, os::unix::fs::PermissionsExt};
    let directory = tempfile::tempdir().unwrap();
    let mut messages = vec![
        json!({"jsonrpc":"2.0", "id":1, "result":{"capabilities":{
            "definitionProvider":true, "callHierarchyProvider":true, "experimental":{"parentModule":true}
        }}}),
        json!({"jsonrpc":"2.0", "method":"experimental/serverStatus", "params":{
            "quiescent":true, "health":health, "message":"test project loading"
        }}),
    ];
    messages.extend(response);
    let frames = messages
        .into_iter()
        .map(|message| {
            let body = message.to_string();
            format!("Content-Length: {}\r\n\r\n{body}", body.len())
        })
        .collect::<String>();
    let responses = directory.path().join("responses");
    fs::write(&responses, frames).unwrap();
    let script = directory.path().join("server");
    let quoted = responses.to_string_lossy().replace('\'', "'\\''");
    fs::write(
        &script,
        format!("#!/bin/sh\ncat '{quoted}'\ncat >/dev/null\n"),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let client = super::Lsp::start(&script, directory.path()).unwrap();
    (directory, client)
}

/// warning 也可能表示依赖或宏加载失败，必须失败并回收服务器。
#[cfg(unix)]
#[test]
fn failed_project_loading_does_not_return_a_graph() {
    let (_directory, mut client) = fake_server("warning", None);
    let pid = client._process.0.id();
    assert!(client
        .wait_ready()
        .unwrap_err()
        .message
        .contains("项目未完整加载"));
    drop(client);
    // SAFETY: 信号 0 只查询刚回收的测试进程是否存在，不发送实际信号。
    assert_eq!(unsafe { libc::kill(pid as i32, 0) }, -1);
}

/// 错误响应和超时均不能伪装成成功的 null/空列表。
#[cfg(unix)]
#[test]
fn query_errors_and_timeouts_are_explicit() {
    let (_directory, mut client) = fake_server(
        "ok",
        Some(json!({"jsonrpc":"2.0", "id":2,
        "error":{"code":-32603,"message":"query failed"}})),
    );
    client.wait_ready().unwrap();
    assert!(client
        .request("textDocument/definition", json!({}))
        .unwrap_err()
        .message
        .contains("query failed"));
    let error = client
        .request_with_timeout(
            "textDocument/definition",
            json!({}),
            std::time::Duration::from_millis(50),
        )
        .unwrap_err();
    assert!(error.message.contains("textDocument/definition"));
}
