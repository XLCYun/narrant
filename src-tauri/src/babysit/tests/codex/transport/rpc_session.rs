use super::*;
use crate::babysit::codex::AppServerCommand;
use serde_json::json;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// Codex 省略 `jsonrpc` 时，typed envelope 仍应保持可发送和可解析。
#[test]
fn typed_envelopes_accept_omitted_jsonrpc() {
    let request = RequestEnvelope::new(7, "demo/method", json!({"answer": 42}));
    assert_eq!(
        serde_json::to_value(request).unwrap(),
        json!({"id": 7, "method": "demo/method", "params": {"answer": 42}})
    );
    assert_eq!(
        serde_json::to_value(NotificationEnvelope::new("initialized", json!({}))).unwrap(),
        json!({"method": "initialized", "params": {}})
    );

    let message = IncomingMessage::decode(json!({"id": 7, "result": {"ok": true}})).unwrap();
    match message {
        IncomingMessage::Response(response) => {
            assert_eq!(response.id(), &json!(7));
            assert_eq!(response.into_result().unwrap(), json!({"ok": true}));
        }
        other => panic!("expected response, got {other:?}"),
    }
}

/// response 缺少 result/error 时必须报告协议失败，而不是猜测成功结果。
#[test]
fn response_requires_exactly_one_result_or_error() {
    assert!(matches!(
        IncomingMessage::decode(json!({"id": 1})),
        Err(RpcFailure::Protocol)
    ));
    assert!(matches!(
        IncomingMessage::decode(json!({
            "id": 1,
            "result": {},
            "error": {"code": -1, "message": "both"}
        })),
        Err(RpcFailure::Protocol)
    ));
}

/// 远程 error 的 code/message/data 和未知字段必须在机制层保留给 facade。
#[test]
fn remote_error_is_extracted_without_business_classification() {
    let message = IncomingMessage::decode(json!({
        "jsonrpc": "2.0",
        "id": 2,
        "error": {
            "code": -32602,
            "message": "invalid field",
            "data": {"field": "permissions"},
            "future": true
        }
    }))
    .unwrap();
    let IncomingMessage::Response(response) = message else {
        panic!("expected response");
    };
    let error = response.into_result().unwrap_err();
    let RpcSessionError::Remote(error) = error else {
        panic!("expected remote error");
    };
    assert_eq!(error.as_value()["code"], -32602);
    assert_eq!(error.as_value()["message"], "invalid field");
    assert_eq!(error.as_value()["data"], json!({"field": "permissions"}));
    assert_eq!(error.as_value()["future"], true);
}

/// fake server 为每个 request 先发未知 notification；session 应按顺序分配 id 并保留它。
#[tokio::test]
async fn requests_are_sequential_and_notifications_are_buffered() {
    let script = r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  printf '{"method":"future/event","params":{"opaque":true,"id":%s}}\n' "$id"
  printf '{"id":999,"result":{"stale":true}}\n'
  printf '{"id":%s,"result":{"requestId":%s}}\n' "$id" "$id"
done
"#;
    let command = AppServerCommand {
        program: PathBuf::from("sh"),
        args: vec![OsString::from("-c"), OsString::from(script)],
    };
    let mut connection = JsonlConnection::spawn(&command).await.unwrap();
    let mut session = RpcSession::new();
    let deadline = Instant::now() + Duration::from_secs(2);

    let first = session
        .request(&mut connection, "first", json!({}), deadline, None, |_| {
            Err(RpcSessionError::Failure(RpcFailure::Protocol))
        })
        .await
        .unwrap();
    assert_eq!(first, json!({"requestId": 1}));

    let notification = session.pop_notification().unwrap();
    assert_eq!(notification.method(), "future/event");
    assert_eq!(notification.params()["opaque"], true);
    assert_eq!(notification.as_value()["params"]["id"], 1);

    let second = session
        .request(&mut connection, "second", json!({}), deadline, None, |_| {
            Err(RpcSessionError::Failure(RpcFailure::Protocol))
        })
        .await
        .unwrap();
    assert_eq!(second, json!({"requestId": 2}));
    connection.shutdown().await;
}

/// server request 的 method/id/params 由 session 分类并交给 facade handler，回复仍由 facade 决定。
#[tokio::test]
async fn server_requests_are_surfaced_to_the_facade_handler() {
    let script = r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9][0-9]*\).*/\1/p')
  if printf '%s' "$line" | grep -q '"method":"demo"'; then
    printf '{"id":99,"method":"server/approval","params":{"question":"read"}}\n'
  fi
  printf '{"id":%s,"result":{"ok":true}}\n' "$id"
done
"#;
    let command = AppServerCommand {
        program: PathBuf::from("sh"),
        args: vec![OsString::from("-c"), OsString::from(script)],
    };
    let mut connection = JsonlConnection::spawn(&command).await.unwrap();
    let mut session = RpcSession::new();
    let observed = std::sync::Arc::new(std::sync::Mutex::new(None));
    let observed_by_handler = observed.clone();
    let result = session
        .request(
            &mut connection,
            "demo",
            json!({}),
            Instant::now() + Duration::from_secs(2),
            None,
            move |request| {
                *observed_by_handler.lock().unwrap() = Some((
                    request.id_value(),
                    request.method().to_owned(),
                    request.params().clone(),
                ));
                Ok(json!({
                    "id": request.id_value(),
                    "error": {"code": -32601, "message": "unsupported"}
                }))
            },
        )
        .await
        .unwrap();
    assert_eq!(result, json!({"ok": true}));
    assert_eq!(
        observed.lock().unwrap().clone(),
        Some((
            json!(99),
            "server/approval".to_owned(),
            json!({"question": "read"})
        ))
    );
    connection.shutdown().await;
}
