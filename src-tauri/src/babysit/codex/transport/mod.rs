//! Codex App Server 协议门面。

use super::model_catalog::{CodexModelCapability, CodexModelCatalog, ModelListPage};
use super::{
    restricted_config, AppServerCommand, AppServerProcess, RpcFailure, CODEX_CLEANUP_TIMEOUT,
    CODEX_CLIENT_NAME, CODEX_INITIALIZATION_TIMEOUT, CODEX_THREAD_SOURCE,
};
use crate::babysit::{BabysitProviderRequest, CancellationSignal};
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Instant;

mod jsonl_connection;
pub(super) use jsonl_connection::JsonlConnection;
mod rpc_session;
pub(super) use rpc_session::RpcSession;
use rpc_session::{RpcSessionError, ServerRequest};

/// 不存在的 thread/turn 只用于解析协议参数，绝不会触发模型推理。
const CAPABILITY_SENTINEL_THREAD_ID: &str = "00000000-0000-0000-0000-000000000001";
const CAPABILITY_SENTINEL_TURN_ID: &str = "00000000-0000-0000-0000-000000000002";

/// initialize 端点当前稳定且必须存在的响应字段。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct InitializeResponse {
    user_agent: String,
    codex_home: String,
}

impl InitializeResponse {
    /// 解码并校验握手所需的非空元数据，额外字段由 serde 自动忽略。
    fn decode(value: Value) -> Result<Self, RpcFailure> {
        let response: Self = serde_json::from_value(value).map_err(|_| RpcFailure::Protocol)?;
        if response.user_agent.trim().is_empty() || response.codex_home.trim().is_empty() {
            return Err(RpcFailure::Protocol);
        }
        Ok(response)
    }
}

/// account/read 只建模认证判断依赖的字段，账户详情保持不透明。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountReadResponse {
    requires_openai_auth: bool,
    account: Option<Value>,
}

impl AccountReadResponse {
    /// 解码账户状态，同时容忍服务端新增字段或扩展账户对象。
    fn decode(value: Value) -> Result<Self, RpcFailure> {
        serde_json::from_value(value).map_err(|_| RpcFailure::Protocol)
    }

    /// 仅在服务端要求 OpenAI 认证但没有账户时报告未登录。
    fn validate(self) -> Result<(), RpcFailure> {
        if self.requires_openai_auth && self.account.is_none_or(|value| value.is_null()) {
            return Err(RpcFailure::SignedOut);
        }
        Ok(())
    }
}

/// thread/start 返回中 Babysit 必须持有的稳定 thread 标识。
#[derive(Debug, Deserialize)]
struct ThreadStartResponse {
    thread: EndpointId,
    #[serde(skip)]
    opaque: Value,
}

impl ThreadStartResponse {
    /// 解码 thread 标识，并显式保留策略校验所需的演进载荷。
    fn decode(value: Value) -> Result<Self, RpcFailure> {
        let mut response: Self =
            serde_json::from_value(value.clone()).map_err(|_| RpcFailure::Protocol)?;
        response.opaque = value;
        Ok(response)
    }

    /// 返回已校验存在的 thread 标识。
    fn thread_id(&self) -> &str {
        &self.thread.id
    }
}

/// turn/start 返回中 Babysit 必须持有的稳定 turn 标识。
#[derive(Debug, Deserialize)]
struct TurnStartResponse {
    turn: EndpointId,
}

impl TurnStartResponse {
    /// 解码 turn 标识，未建模字段继续允许服务端演进。
    fn decode(value: Value) -> Result<Self, RpcFailure> {
        serde_json::from_value(value).map_err(|_| RpcFailure::Protocol)
    }

    /// 返回已校验存在的 turn 标识。
    fn turn_id(&self) -> &str {
        &self.turn.id
    }
}

/// Codex thread 与 turn 端点共用的稳定标识对象。
#[derive(Debug, Deserialize)]
struct EndpointId {
    id: String,
}

/// thread/list 当前清理策略依赖的最小 thread 摘要。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadSummary {
    id: Option<String>,
    thread_source: Option<String>,
}

/// thread/list 分页响应，只建模孤儿清理所需的字段。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ThreadListResponse {
    data: Vec<ThreadSummary>,
    next_cursor: Option<String>,
}

impl ThreadListResponse {
    /// 解码分页响应；缺少 data 属于协议错误，未知字段保持兼容。
    fn decode(value: Value) -> Result<Self, RpcFailure> {
        serde_json::from_value(value).map_err(|_| RpcFailure::Protocol)
    }

    /// 提取明确属于 Narrant 的 thread，避免删除其它来源的会话。
    fn marked_thread_ids(&self) -> Vec<&str> {
        self.data
            .iter()
            .filter(|thread| thread.thread_source.as_deref() == Some(CODEX_THREAD_SOURCE))
            .filter_map(|thread| thread.id.as_deref())
            .collect()
    }

    /// 返回下一页 cursor；缺失表示分页结束。
    fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }
}

/// facade 识别的通知语义；未知 method 明确保留为可忽略分支。
#[derive(Debug)]
enum Notification {
    AgentMessageDelta {
        thread_id: String,
        turn_id: String,
        delta: String,
    },
    ItemCompleted {
        thread_id: String,
        turn_id: String,
        item: Value,
    },
    TurnCompleted {
        thread_id: String,
        turn: CompletedTurn,
    },
    Unknown,
}

impl Notification {
    /// 按 method 选择局部 DTO；未知通知不解析 params，因此可向前兼容。
    fn decode(value: Value) -> Result<Self, RpcFailure> {
        let envelope: NotificationEnvelope =
            serde_json::from_value(value).map_err(|_| RpcFailure::Protocol)?;
        match envelope.method.as_str() {
            "item/agentMessage/delta" => {
                let params: AgentMessageDeltaParams =
                    serde_json::from_value(envelope.params).map_err(|_| RpcFailure::Protocol)?;
                Ok(Self::AgentMessageDelta {
                    thread_id: params.thread_id,
                    turn_id: params.turn_id,
                    delta: params.delta,
                })
            }
            "item/completed" => {
                let params: ItemCompletedParams =
                    serde_json::from_value(envelope.params).map_err(|_| RpcFailure::Protocol)?;
                Ok(Self::ItemCompleted {
                    thread_id: params.thread_id,
                    turn_id: params.turn_id,
                    item: params.item,
                })
            }
            "turn/completed" => {
                let params: TurnCompletedParams =
                    serde_json::from_value(envelope.params).map_err(|_| RpcFailure::Protocol)?;
                Ok(Self::TurnCompleted {
                    thread_id: params.thread_id,
                    turn: params.turn,
                })
            }
            _ => Ok(Self::Unknown),
        }
    }
}

/// 从 RpcSession 取得的通知 envelope，params 作为显式演进载荷交给端点 DTO。
#[derive(Debug, Deserialize)]
struct NotificationEnvelope {
    method: String,
    #[serde(default)]
    params: Value,
}

/// agent message 流式增量的稳定字段。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentMessageDeltaParams {
    thread_id: String,
    turn_id: String,
    delta: String,
}

/// item/completed 中用于筛选目标 turn 和读取最终 item 的字段。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemCompletedParams {
    thread_id: String,
    turn_id: String,
    item: Value,
}

/// turn/completed 中用于筛选目标 thread 的字段。
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TurnCompletedParams {
    thread_id: String,
    turn: CompletedTurn,
}

/// 已完成 turn 的稳定状态字段；items 与 error 保持不透明以兼容服务端演进。
#[derive(Debug, Deserialize)]
struct CompletedTurn {
    id: String,
    status: String,
    error: Option<Value>,
    items: Option<Value>,
}

impl AppServerProcess {
    /// 分页读取完整 model/list，并在返回前过滤隐藏模型及校验默认能力。
    pub(super) async fn model_catalog(
        &mut self,
        deadline: Instant,
    ) -> Result<CodexModelCatalog, RpcFailure> {
        let mut cursor: Option<String> = None;
        let mut models: Vec<CodexModelCapability> = Vec::new();
        loop {
            let value = self
                .request("model/list", json!({"cursor": cursor}), deadline, None)
                .await?;
            cursor = ModelListPage::decode(value)
                .map_err(|_| RpcFailure::Protocol)?
                .append_to(&mut models);
            if cursor.is_none() {
                break;
            }
        }
        CodexModelCatalog::from_pages(models).map_err(|_| RpcFailure::Protocol)
    }

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
            if matches!(
                message,
                Notification::TurnCompleted {
                    thread_id: ref message_thread_id,
                    ref turn,
                } if message_thread_id == thread_id && turn.id == turn_id
            ) {
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
        InitializeResponse::decode(initialize).map_err(classify_initialize_error)?;
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
        AccountReadResponse::decode(account)
            .and_then(AccountReadResponse::validate)
            .map_err(classify_initialize_error)?;
        process.cleanup_marked_threads().await?;
        Ok(process)
    }

    /// 在同一 App Server 连接上验证受限 thread、结构化输出、interrupt 和 delete 能力。
    pub(super) async fn probe_capabilities(&mut self, workspace: &Path) -> Result<(), RpcFailure> {
        let cancellation = CancellationSignal::default();
        let request = BabysitProviderRequest {
            analysis_run_id: 1,
            path: "__narrant_babysit_capability_probe__".to_owned(),
            locale: "en".to_owned(),
            workspace_path: workspace.to_string_lossy().into_owned(),
            before: String::new(),
            after: String::new(),
            targets: Vec::new(),
            model: "capability-probe".to_owned(),
            reasoning_effort: "low".to_owned(),
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
                    "model": request.model,
                    "reasoningEffort": request.reasoning_effort,
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
        let response = ThreadStartResponse::decode(result).map_err(classify_start_error)?;
        if validate_thread_policy(&response.opaque, &request.workspace_path).is_err() {
            if !response.thread_id().is_empty() {
                let _ = self
                    .request(
                        "thread/delete",
                        json!({"threadId": response.thread_id()}),
                        Instant::now() + CODEX_CLEANUP_TIMEOUT,
                        None,
                    )
                    .await;
            }
            return Err(RpcFailure::Incompatible);
        }
        if cancellation.is_cancelled() {
            if !response.thread_id().is_empty() {
                let _ = self
                    .request(
                        "thread/delete",
                        json!({"threadId": response.thread_id()}),
                        Instant::now() + CODEX_CLEANUP_TIMEOUT,
                        None,
                    )
                    .await;
            }
            return Err(RpcFailure::Cancelled);
        }
        Ok(response.thread_id().to_owned())
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
        TurnStartResponse::decode(result)
            .map(|response| response.turn_id().to_owned())
            .map_err(classify_start_error)
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
            match message {
                Notification::AgentMessageDelta {
                    thread_id: message_thread_id,
                    turn_id: message_turn_id,
                    delta,
                } if message_thread_id == thread_id && message_turn_id == turn_id => {
                    streamed_text.push_str(&delta);
                }
                Notification::ItemCompleted {
                    thread_id: message_thread_id,
                    turn_id: message_turn_id,
                    item,
                } if message_thread_id == thread_id && message_turn_id == turn_id => {
                    if let Some(text) = agent_message_text(&item) {
                        completed_text = Some(text);
                    }
                }
                Notification::TurnCompleted {
                    thread_id: message_thread_id,
                    turn,
                } if message_thread_id == thread_id && turn.id == turn_id => {
                    match turn.status.as_str() {
                        "completed" => {
                            let final_text = completed_text
                                .or_else(|| turn.items.as_ref().and_then(agent_message_text))
                                .or_else(|| (!streamed_text.is_empty()).then_some(streamed_text));
                            return Ok(final_text);
                        }
                        "interrupted" => return Err(RpcFailure::Cancelled),
                        "failed" => {
                            let failure = turn
                                .error
                                .as_ref()
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
                _ => {}
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
                let page =
                    ThreadListResponse::decode(result).map_err(classify_capability_probe_error)?;
                let ids = page
                    .marked_thread_ids()
                    .into_iter()
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
                let next_cursor = page.next_cursor().map(ToOwned::to_owned);
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
    ) -> Result<Notification, RpcFailure> {
        self.rpc_session
            .next_notification(&mut self.connection, deadline, cancellation, |request| {
                Ok(reject_server_request(request))
            })
            .await
            .map(|notification| notification.into_value())
            .map_err(map_session_error)
            .and_then(Notification::decode)
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

/// 校验 App Server 实际应用的 workspace、权限 profile 与只读 sandbox。
fn validate_thread_policy(result: &Value, workspace: &str) -> Result<(), RpcFailure> {
    let roots = result
        .get("runtimeWorkspaceRoots")
        .and_then(Value::as_array)
        .ok_or(RpcFailure::Protocol)?;
    let exact_root = roots.len() == 1 && roots[0].as_str() == Some(workspace);
    let exact_cwd = result.get("cwd").and_then(Value::as_str) == Some(workspace);
    let profile = result
        .get("activePermissionProfile")
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        == Some("narrant-babysit-restricted-v1");
    let sandbox = result.get("sandbox").unwrap_or(&Value::Null);
    let read_only = sandbox.get("type").and_then(Value::as_str) == Some("readOnly")
        && sandbox.get("networkAccess").and_then(Value::as_bool) == Some(false);
    (exact_root && exact_cwd && profile && read_only)
        .then_some(())
        .ok_or(RpcFailure::Protocol)
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

#[cfg(test)]
#[path = "../../tests/codex/transport/mod.rs"]
mod tests;
