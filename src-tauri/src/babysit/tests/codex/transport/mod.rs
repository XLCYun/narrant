use super::*;
use serde_json::json;

/// 稳定端点字段应被强校验，同时忽略未来新增的响应字段。
#[test]
fn endpoint_dtos_require_stable_fields_and_tolerate_unknown_fields() {
    assert!(InitializeResponse::decode(json!({
        "userAgent": "codex/1.0",
        "codexHome": "/tmp/codex",
        "future": {"enabled": true}
    }))
    .is_ok());
    assert!(matches!(
        InitializeResponse::decode(json!({"userAgent": "codex/1.0"})),
        Err(RpcFailure::Protocol)
    ));

    assert_eq!(
        AccountReadResponse::decode(json!({
            "requiresOpenaiAuth": true,
            "account": null,
            "future": true
        }))
        .and_then(AccountReadResponse::validate),
        Err(RpcFailure::SignedOut)
    );
}

/// facade 只抽取业务必需的标识符，未建模的端点载荷保持可演进。
#[test]
fn lifecycle_dtos_extract_required_ids_from_evolving_payloads() {
    let thread = ThreadStartResponse::decode(json!({
        "thread": {"id": "thread-1", "future": "kept"},
        "cwd": "/workspace",
        "runtimeWorkspaceRoots": ["/workspace"],
        "activePermissionProfile": {"id": "narrant-babysit-restricted-v1"},
        "sandbox": {"type": "readOnly", "networkAccess": false},
        "future": [1, 2, 3]
    }))
    .unwrap();
    assert_eq!(thread.thread_id(), "thread-1");

    let turn = TurnStartResponse::decode(json!({
        "turn": {"id": "turn-1", "future": true},
        "future": {"opaque": true}
    }))
    .unwrap();
    assert_eq!(turn.turn_id(), "turn-1");

    let page = ThreadListResponse::decode(json!({
        "data": [
            {"id": "owned", "threadSource": "feature:narrant-babysit", "future": true},
            {"id": "other", "threadSource": "feature:other"}
        ],
        "nextCursor": "next",
        "future": false
    }))
    .unwrap();
    assert_eq!(page.marked_thread_ids(), vec!["owned"]);
    assert_eq!(page.next_cursor(), Some("next"));
}

/// 未知通知保持可忽略；已知通知只要求当前业务真正依赖的字段。
#[test]
fn notification_interpretation_is_typed_but_forward_compatible() {
    assert!(matches!(
        Notification::decode(json!({
            "method": "future/event",
            "future": "ignored"
        })),
        Ok(Notification::Unknown)
    ));

    assert!(matches!(
        Notification::decode(json!({
            "method": "item/agentMessage/delta",
            "params": {
                "threadId": "thread-1",
                "turnId": "turn-1",
                "delta": "hello",
                "future": true
            }
        })),
        Ok(Notification::AgentMessageDelta { delta, .. }) if delta == "hello"
    ));

    assert!(matches!(
        Notification::decode(json!({
            "method": "turn/completed",
            "params": {"threadId": "thread-1", "turn": {"id": "turn-1"}}
        })),
        Err(RpcFailure::Protocol)
    ));
}

/// 机制错误保持原类别，远程 RPC 错误只在 facade 中投影为业务失败。
#[test]
fn facade_preserves_failure_classification_boundaries() {
    assert_eq!(
        map_session_error(RpcSessionError::Failure(RpcFailure::Timeout)),
        RpcFailure::Timeout
    );
    assert_eq!(
        classify_remote_error(&json!({"code": -32601, "message": "future wording"})),
        RpcFailure::MethodNotFound
    );
    assert_eq!(
        classify_remote_error(&json!({"code": -32602, "message": "future wording"})),
        RpcFailure::InvalidParams
    );
    assert_eq!(
        classify_remote_error(&json!({"message": "authentication required"})),
        RpcFailure::SignedOut
    );
    assert_eq!(
        classify_remote_error(&json!({"message": "thread not found"})),
        RpcFailure::EntityNotFound
    );
    assert_eq!(
        classify_remote_error(&json!({"message": "turn already completed"})),
        RpcFailure::AlreadyComplete
    );
    assert_eq!(
        classify_remote_error(&json!({"code": -32000, "message": "opaque rejection"})),
        RpcFailure::Remote
    );

    assert_eq!(
        classify_initialize_error(RpcFailure::Timeout),
        RpcFailure::Initialization
    );
    assert_eq!(
        classify_start_error(RpcFailure::Protocol),
        RpcFailure::Incompatible
    );
    assert_eq!(
        classify_capability_probe_error(RpcFailure::EntityNotFound),
        RpcFailure::Incompatible
    );
}
