//! Codex App Server JSONL transport 与进程协议。

use super::{
    restricted_config, validate_thread_policy, AppServerCommand, AppServerProcess, RpcFailure,
    CODEX_CLEANUP_TIMEOUT, CODEX_CLIENT_NAME, CODEX_INITIALIZATION_TIMEOUT, CODEX_THREAD_SOURCE,
};
use crate::babysit::{BabysitProviderRequest, CancellationSignal};
use serde_json::{json, Value};
use std::path::Path;
use std::time::Instant;

#[path = "transport/jsonl_connection.rs"]
mod jsonl_connection;
pub(super) use jsonl_connection::JsonlConnection;
#[path = "transport/rpc_session.rs"]
mod rpc_session;
pub(super) use rpc_session::RpcSession;
use rpc_session::{RpcSessionError, ServerRequest};

/// 不存在的 thread/turn 只用于解析协议参数，绝不会触发模型推理。
const CAPABILITY_SENTINEL_THREAD_ID: &str = "00000000-0000-0000-0000-000000000001";
const CAPABILITY_SENTINEL_TURN_ID: &str = "00000000-0000-0000-0000-000000000002";

impl AppServerProcess {
    /// poison restart 前同步完成 child reap。
    pub(super) async fn shutdown(&mut self) {
        self.connection.shutdown().await;
    }

    /// interrupt 后等待精确匹配的 terminal notification，再允许删除 thread。
    pub(super) async fn wait_for_terminal(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        deadline: Instant,
    ) -> Result<(), RpcFailure> {
        loop {
            let message = self.next_notification(deadline, None).await?;
            if message.get("method").and_then(Value::as_str) == Some("turn/completed")
                && same_turn(&message, thread_id, turn_id)
            {
                return Ok(());
            }
        }
    }

    /// 启动 stdio App Server，并完成 initialize/initialized handshake。
    pub(super) async fn spawn(command: AppServerCommand) -> Result<Self, RpcFailure> {
        let connection = JsonlConnection::spawn(&command).await?;
        let mut process = Self {
            connection,
            rpc_session: RpcSession::new(),
        };
        let deadline = Instant::now() + CODEX_INITIALIZATION_TIMEOUT;
        let initialize = process
            .request(
                "initialize",
                json!({
                    "clientInfo": {
                        "name": CODEX_CLIENT_NAME,
                        "title": "Narrant Babysit",
                        "version": env!("CARGO_PKG_VERSION")
                    },
                    "capabilities": {"experimentalApi": true}
                }),
                deadline,
                None,
            )
            .await
            .map_err(classify_initialize_error)?;
        validate_initialize_response(&initialize).map_err(classify_initialize_error)?;
        process
            .notification("initialized", json!({}), deadline, None)
            .await
            .map_err(|_| RpcFailure::Initialization)?;
        let account = process
            .request(
                "account/read",
                json!({"refreshToken": false}),
                deadline,
                None,
            )
            .await
            .map_err(classify_initialize_error)?;
        validate_account_state(&account).map_err(classify_initialize_error)?;
        process.cleanup_marked_threads().await?;
        Ok(process)
    }

    /// 在同一 App Server 连接上验证受限 thread、结构化输出、interrupt 和 delete 能力。
    pub(super) async fn probe_capabilities(&mut self, workspace: &Path) -> Result<(), RpcFailure> {
        let cancellation = CancellationSignal::default();
        let request = BabysitProviderRequest {
            path: "__narrant_babysit_capability_probe__".to_owned(),
            locale: "en".to_owned(),
            workspace_path: workspace.to_string_lossy().into_owned(),
            before: String::new(),
            after: String::new(),
            targets: Vec::new(),
        };
        let deadline = Instant::now() + CODEX_INITIALIZATION_TIMEOUT;
        let thread_id = self
            .start_thread(&request, deadline, &cancellation)
            .await
            .map_err(classify_capability_probe_error)?;
        let probe_result = self.probe_turn_capabilities(&request, deadline).await;
        // 真实 restricted thread 只验证 policy 与 delete；永远不在它上面启动 turn。
        let delete_result = self.delete_probe_thread(&thread_id).await;
        match (probe_result, delete_result) {
            (Err(error), _) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
            (Ok(()), Err(error)) => Err(error),
        }
    }

    /// 对不存在的 sentinel thread/turn 解析 turn 与 schema、interrupt 能力，不调用模型。
    async fn probe_turn_capabilities(
        &mut self,
        request: &BabysitProviderRequest,
        deadline: Instant,
    ) -> Result<(), RpcFailure> {
        match self
            .request(
                "turn/start",
                json!({
                    "threadId": CAPABILITY_SENTINEL_THREAD_ID,
                    "input": [],
                    "outputSchema": super::structured_output_schema(request)
                }),
                deadline,
                None,
            )
            .await
        {
            Ok(_) | Err(RpcFailure::EntityNotFound | RpcFailure::AlreadyComplete) => {}
            Err(RpcFailure::SignedOut) => return Err(RpcFailure::SignedOut),
            Err(error) => return Err(classify_capability_probe_error(error)),
        }
        match self
            .request(
                "turn/interrupt",
                json!({
                    "threadId": CAPABILITY_SENTINEL_THREAD_ID,
                    "turnId": CAPABILITY_SENTINEL_TURN_ID
                }),
                deadline,
                None,
            )
            .await
        {
            Ok(_) | Err(RpcFailure::EntityNotFound | RpcFailure::AlreadyComplete) => Ok(()),
            Err(RpcFailure::SignedOut) => Err(RpcFailure::SignedOut),
            Err(error) => Err(classify_capability_probe_error(error)),
        }
    }

    /// 删除 probe thread；缺少 delete 或字段不被接受必须归类为 incompatible。
    async fn delete_probe_thread(&mut self, thread_id: &str) -> Result<(), RpcFailure> {
        self.request(
            "thread/delete",
            json!({"threadId": thread_id}),
            Instant::now() + CODEX_CLEANUP_TIMEOUT,
            None,
        )
        .await
        .map(|_| ())
        .map_err(classify_capability_probe_error)
    }

    /// 创建一个只读、带 Narrant 标记的 fresh thread。
    pub(super) async fn start_thread(
        &mut self,
        request: &BabysitProviderRequest,
        deadline: Instant,
        cancellation: &CancellationSignal,
    ) -> Result<String, RpcFailure> {
        let config = restricted_config();
        let result = self
            .request(
                "thread/start",
                json!({
                    "cwd": request.workspace_path,
                    "approvalPolicy": "never",
                    "permissions": "narrant-babysit-restricted-v1",
                    "runtimeWorkspaceRoots": [request.workspace_path],
                    "model": super::BABYSIT_CODEX_MODEL,
                    "reasoningEffort": super::BABYSIT_REASONING_EFFORT,
                    "allowProviderModelFallback": false,
                    "config": config,
                    "serviceName": CODEX_CLIENT_NAME,
                    "personality": "none",
                    "threadSource": CODEX_THREAD_SOURCE
                }),
                deadline,
                None,
            )
            .await
            .map_err(classify_start_error)?;
        if validate_thread_policy(&result, &request.workspace_path).is_err() {
            if let Some(thread_id) = result
                .get("thread")
                .and_then(|thread| thread.get("id"))
                .and_then(Value::as_str)
            {
                let _ = self
                    .request(
                        "thread/delete",
                        json!({"threadId": thread_id}),
                        Instant::now() + CODEX_CLEANUP_TIMEOUT,
                        None,
                    )
                    .await;
            }
            return Err(RpcFailure::Incompatible);
        }
        if cancellation.is_cancelled() {
            if let Some(thread_id) = result
                .get("thread")
                .and_then(|thread| thread.get("id"))
                .and_then(Value::as_str)
            {
                let _ = self
                    .request(
                        "thread/delete",
                        json!({"threadId": thread_id}),
                        Instant::now() + CODEX_CLEANUP_TIMEOUT,
                        None,
                    )
                    .await;
            }
            return Err(RpcFailure::Cancelled);
        }
        result
            .get("thread")
            .and_then(|thread| thread.get("id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or(RpcFailure::Incompatible)
    }

    /// 提交 turn/start；outputSchema 要求 map 形状但不把动态行号伪装成固定 schema。
    pub(super) async fn start_turn(
        &mut self,
        thread_id: &str,
        prompt: String,
        request: &BabysitProviderRequest,
        deadline: Instant,
        cancellation: &CancellationSignal,
    ) -> Result<String, RpcFailure> {
        let result = self
            .request(
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "input": [{"type": "text", "text": prompt}],
                    "outputSchema": super::structured_output_schema(request)
                }),
                deadline,
                Some(cancellation),
            )
            .await
            .map_err(classify_start_error)?;
        result
            .get("turn")
            .and_then(|turn| turn.get("id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .ok_or(RpcFailure::Incompatible)
    }

    /// 读取 turn/completed，并只保留最终 agent message 文本，不传播其它事件。
    pub(super) async fn wait_for_turn(
        &mut self,
        thread_id: &str,
        turn_id: &str,
        deadline: Instant,
        cancellation: &CancellationSignal,
    ) -> Result<Option<String>, RpcFailure> {
        let mut streamed_text = String::new();
        let mut completed_text = None;
        loop {
            let message = self.next_notification(deadline, Some(cancellation)).await?;
            if let Some(method) = message.get("method").and_then(Value::as_str) {
                if method == "item/agentMessage/delta" && same_turn(&message, thread_id, turn_id) {
                    if let Some(delta) = message
                        .get("params")
                        .and_then(|params| params.get("delta"))
                        .and_then(Value::as_str)
                    {
                        streamed_text.push_str(delta);
                    }
                } else if method == "item/completed" && same_turn(&message, thread_id, turn_id) {
                    if let Some(text) = message
                        .get("params")
                        .and_then(|params| params.get("item"))
                        .and_then(agent_message_text)
                    {
                        completed_text = Some(text);
                    }
                } else if method == "turn/completed" && same_turn(&message, thread_id, turn_id) {
                    let turn = message
                        .get("params")
                        .and_then(|params| params.get("turn"))
                        .ok_or(RpcFailure::Protocol)?;
                    match turn.get("status").and_then(Value::as_str) {
                        Some("completed") => {
                            let final_text = completed_text
                                .or_else(|| turn.get("items").and_then(agent_message_text))
                                .or_else(|| (!streamed_text.is_empty()).then_some(streamed_text));
                            return Ok(final_text);
                        }
                        Some("interrupted") => return Err(RpcFailure::Cancelled),
                        Some("failed") => {
                            let failure = turn
                                .get("error")
                                .map(classify_remote_error)
                                .unwrap_or(RpcFailure::Remote);
                            return Err(if failure == RpcFailure::SignedOut {
                                RpcFailure::SignedOut
                            } else {
                                RpcFailure::Remote
                            });
                        }
                        _ => return Err(RpcFailure::Protocol),
                    }
                }
            }
        }
    }

    /// 启动后删除上次异常退出遗留、且明确带 Narrant feature marker 的 thread。
    async fn cleanup_marked_threads(&mut self) -> Result<(), RpcFailure> {
        for archived in [false, true] {
            let mut cursor: Option<String> = None;
            loop {
                let mut params = json!({
                    "archived": archived,
                    "limit": 100
                });
                if let Some(cursor_value) = cursor.as_deref() {
                    params["cursor"] = Value::String(cursor_value.to_owned());
                }
                let result = self
                    .request(
                        "thread/list",
                        params,
                        Instant::now() + CODEX_CLEANUP_TIMEOUT,
                        None,
                    )
                    .await
                    .map_err(classify_capability_probe_error)?;
                let Some(threads) = result.get("data").and_then(Value::as_array) else {
                    return Err(RpcFailure::Incompatible);
                };
                let ids = threads
                    .iter()
                    .filter(|thread| {
                        thread.get("threadSource").and_then(Value::as_str)
                            == Some(CODEX_THREAD_SOURCE)
                    })
                    .filter_map(|thread| thread.get("id").and_then(Value::as_str))
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>();
                for thread_id in ids {
                    self.request(
                        "thread/delete",
                        json!({"threadId": thread_id}),
                        Instant::now() + CODEX_CLEANUP_TIMEOUT,
                        None,
                    )
                    .await
                    .map_err(classify_capability_probe_error)?;
                }
                let next_cursor = result
                    .get("nextCursor")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned);
                if next_cursor.is_none() || next_cursor == cursor {
                    break;
                }
                cursor = next_cursor;
            }
        }
        Ok(())
    }

    /// 发送一条 JSON-RPC notification；协议线不写入 `jsonrpc` 字段。
    async fn notification(
        &mut self,
        method: &str,
        params: Value,
        deadline: Instant,
        cancellation: Option<&CancellationSignal>,
    ) -> Result<(), RpcFailure> {
        self.rpc_session
            .notification(&mut self.connection, method, params, deadline, cancellation)
            .await
            .map_err(map_session_error)
    }

    /// 读取一条 notification，并保持 Codex facade 的拒绝 server-request 策略。
    async fn next_notification(
        &mut self,
        deadline: Instant,
        cancellation: Option<&CancellationSignal>,
    ) -> Result<Value, RpcFailure> {
        self.rpc_session
            .next_notification(&mut self.connection, deadline, cancellation, |request| {
                Ok(reject_server_request(request))
            })
            .await
            .map(|notification| notification.into_value())
            .map_err(map_session_error)
    }

    /// 发送一个 JSON-RPC request，并等待 session correlation 的匹配 id。
    pub(super) async fn request(
        &mut self,
        method: &str,
        params: Value,
        deadline: Instant,
        cancellation: Option<&CancellationSignal>,
    ) -> Result<Value, RpcFailure> {
        self.rpc_session
            .request(
                &mut self.connection,
                method,
                params,
                deadline,
                cancellation,
                |request| Ok(reject_server_request(request)),
            )
            .await
            .map_err(map_session_error)
    }
}

/// 将 session 机制错误转换回当前 Codex facade 使用的内部类别。
fn map_session_error(error: RpcSessionError) -> RpcFailure {
    match error {
        RpcSessionError::Failure(error) => error,
        RpcSessionError::Remote(error) => classify_remote_error(error.as_value()),
    }
}

/// Codex facade 当前拒绝所有 server request，保持只读边界和既有 -32601 policy。
fn reject_server_request(request: &ServerRequest) -> Value {
    json!({
        "id": request.as_value()["id"].clone(),
        "error": {
            "code": -32601,
            "message": "Narrant Babysit does not accept server requests"
        }
    })
}

/// 仅在 App Server 明确要求 OpenAI 认证且没有任何 account 时判定未登录。
fn validate_account_state(value: &Value) -> Result<(), RpcFailure> {
    let requires_openai_auth = value
        .get("requiresOpenaiAuth")
        .and_then(Value::as_bool)
        .ok_or(RpcFailure::Protocol)?;
    if requires_openai_auth && value.get("account").is_none_or(Value::is_null) {
        return Err(RpcFailure::SignedOut);
    }
    Ok(())
}

/// 只识别有限的认证关键词，并丢弃 server 原始错误消息。
fn classify_remote_error(error: &Value) -> RpcFailure {
    let serialized = serde_json::to_string(error)
        .unwrap_or_default()
        .to_ascii_lowercase();
    if [
        "unauthorized",
        "unauthenticated",
        "not logged",
        "logged out",
        "login required",
        "requires login",
        "authentication",
        "auth_required",
    ]
    .iter()
    .any(|marker| serialized.contains(marker))
    {
        RpcFailure::SignedOut
    } else if error.get("code").and_then(Value::as_i64) == Some(-32601)
        || serialized.contains("method not found")
        || serialized.contains("unsupported method")
    {
        RpcFailure::MethodNotFound
    } else if error.get("code").and_then(Value::as_i64) == Some(-32602)
        || serialized.contains("invalid params")
        || serialized.contains("invalid field")
        || serialized.contains("unknown field")
    {
        RpcFailure::InvalidParams
    } else if serialized.contains("already complete")
        || serialized.contains("already completed")
        || serialized.contains("turn is complete")
    {
        RpcFailure::AlreadyComplete
    } else if serialized.contains("entity not found")
        || serialized.contains("thread not found")
        || serialized.contains("turn not found")
    {
        RpcFailure::EntityNotFound
    } else {
        RpcFailure::Remote
    }
}

/// Initialize 响应异常属于握手失败；只有明确的登录拒绝保留 signed-out 类别。
fn classify_initialize_error(error: RpcFailure) -> RpcFailure {
    match error {
        RpcFailure::SignedOut => RpcFailure::SignedOut,
        _ => RpcFailure::Initialization,
    }
}

/// 校验 initialize 的稳定元数据；字段缺失属于握手损坏，不猜测运行时兼容性。
fn validate_initialize_response(value: &Value) -> Result<(), RpcFailure> {
    let has_user_agent = value
        .get("userAgent")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    let has_codex_home = value
        .get("codexHome")
        .and_then(Value::as_str)
        .is_some_and(|value| !value.trim().is_empty());
    (has_user_agent && has_codex_home)
        .then_some(())
        .ok_or(RpcFailure::Protocol)
}

/// 生产请求保留普通传输/截止时间失败；协议字段缺失仍归类为兼容性问题。
fn classify_start_error(error: RpcFailure) -> RpcFailure {
    match error {
        RpcFailure::SignedOut => RpcFailure::SignedOut,
        RpcFailure::Remote
        | RpcFailure::Protocol
        | RpcFailure::MethodNotFound
        | RpcFailure::InvalidParams => RpcFailure::Incompatible,
        other => other,
    }
}

/// 初始化后的能力探测只把认证拒绝与协议缺失区分出来，其余归为 init_failed。
fn classify_capability_probe_error(error: RpcFailure) -> RpcFailure {
    match error {
        RpcFailure::SignedOut => RpcFailure::SignedOut,
        RpcFailure::Remote
        | RpcFailure::Protocol
        | RpcFailure::MethodNotFound
        | RpcFailure::InvalidParams
        | RpcFailure::EntityNotFound
        | RpcFailure::AlreadyComplete
        | RpcFailure::Incompatible => RpcFailure::Incompatible,
        _ => RpcFailure::Initialization,
    }
}

/// 判断 notification 是否属于目标 thread/turn。
fn same_turn(message: &Value, thread_id: &str, turn_id: &str) -> bool {
    let params = message.get("params").unwrap_or(&Value::Null);
    let same_thread = params.get("threadId").and_then(Value::as_str) == Some(thread_id);
    let notification_turn =
        if message.get("method").and_then(Value::as_str) == Some("turn/completed") {
            params
                .get("turn")
                .and_then(|turn| turn.get("id"))
                .and_then(Value::as_str)
        } else {
            params.get("turnId").and_then(Value::as_str)
        };
    let same_turn = notification_turn == Some(turn_id);
    same_thread && same_turn
}

/// 从 Codex ThreadItem 中提取最终 agentMessage 文本，不传播其它 item 内容。
fn agent_message_text(value: &Value) -> Option<String> {
    if value.get("type").and_then(Value::as_str) == Some("agentMessage") {
        return value
            .get("text")
            .and_then(Value::as_str)
            .map(ToOwned::to_owned);
    }
    value.as_array()?.iter().rev().find_map(agent_message_text)
}
