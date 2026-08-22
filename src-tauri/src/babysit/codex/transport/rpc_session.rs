//! Codex App Server 的顺序 JSON-RPC 会话。
//!
//! JsonlConnection 负责字节流；这里负责顺序 id、response correlation、
//! notification 缓冲和 server-request 分类，不创建后台 reader。

use super::super::RpcFailure;
use super::jsonl_connection::JsonlConnection;
use crate::babysit::CancellationSignal;
use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;
use std::time::Instant;

/// Codex request 的 typed envelope；省略 jsonrpc 以兼容现有 App Server。
#[derive(Clone, Debug, Serialize)]
pub(super) struct RequestEnvelope {
    /// Narrant session 分配的顺序数字 id。
    id: Value,
    /// App Server 方法名。
    method: String,
    /// 方法参数，允许 evolving payload。
    params: Value,
}

impl RequestEnvelope {
    /// 构造不携带 jsonrpc 字段的 request。
    pub(super) fn new(id: u64, method: impl Into<String>, params: Value) -> Self {
        Self {
            id: Value::from(id),
            method: method.into(),
            params,
        }
    }
}

/// Codex notification 的 typed envelope；参数保持显式对象形状。
#[derive(Clone, Debug, Serialize)]
pub(super) struct NotificationEnvelope {
    /// App Server 方法名。
    method: String,
    /// notification 参数。
    params: Value,
}

impl NotificationEnvelope {
    /// 构造不携带 jsonrpc 字段的 notification。
    fn new(method: impl Into<String>, params: Value) -> Self {
        Self {
            method: method.into(),
            params,
        }
    }
}

/// 已校验的远程 RPC error；保留原始 object 交给 Codex facade 分类。
#[derive(Clone, Debug, PartialEq)]
pub(super) struct RemoteRpcError(Value);

impl RemoteRpcError {
    /// 校验 JSON-RPC 必需的 code/message 字段但不解释业务含义。
    fn from_value(value: Value) -> Result<Self, RpcFailure> {
        let object = value.as_object().ok_or(RpcFailure::Protocol)?;
        if object.get("code").and_then(Value::as_i64).is_none()
            || object.get("message").and_then(Value::as_str).is_none()
        {
            return Err(RpcFailure::Protocol);
        }
        Ok(Self(value))
    }

    /// 返回完整 error object，供 facade 做脱敏分类。
    pub(super) fn as_value(&self) -> &Value {
        &self.0
    }
}

/// 已校验的 JSON-RPC response envelope。
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ResponseEnvelope {
    /// response 对应的 wire id。
    id: Value,
    /// 成功结果；显式 null 仍是合法结果。
    result: Option<Value>,
    /// 失败时提取的远程 RPC error。
    error: Option<RemoteRpcError>,
}

impl ResponseEnvelope {
    /// 从不要求 jsonrpc 的 response object 构造 envelope。
    fn from_value(value: Value) -> Result<Self, RpcFailure> {
        let object = value.as_object().ok_or(RpcFailure::Protocol)?;
        let id = object.get("id").cloned().ok_or(RpcFailure::Protocol)?;
        if !valid_id(&id) {
            return Err(RpcFailure::Protocol);
        }
        let result = object.get("result").cloned();
        let error = object
            .get("error")
            .cloned()
            .map(RemoteRpcError::from_value)
            .transpose()?;
        if result.is_some() == error.is_some() {
            return Err(RpcFailure::Protocol);
        }
        Ok(Self { id, result, error })
    }

    /// 返回 response id 供 correlation。
    fn id(&self) -> &Value {
        &self.id
    }

    /// 将成功结果或远程 error 转成 session 层结果。
    fn into_result(self) -> Result<Value, RpcSessionError> {
        if let Some(error) = self.error {
            return Err(RpcSessionError::Remote(error));
        }
        self.result
            .ok_or(RpcSessionError::Failure(RpcFailure::Protocol))
    }
}

/// 保留完整 payload 的 notification；未知 method 由 facade 选择忽略。
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Notification(Value);

impl Notification {
    /// 从不要求 jsonrpc 的 notification object 构造 envelope。
    fn from_value(value: Value) -> Result<Self, RpcFailure> {
        let object = value.as_object().ok_or(RpcFailure::Protocol)?;
        if object.get("method").and_then(Value::as_str).is_none() {
            return Err(RpcFailure::Protocol);
        }
        Ok(Self(value))
    }

    /// 返回 notification 方法名。
    pub(super) fn method(&self) -> &str {
        self.0["method"].as_str().unwrap_or_default()
    }

    /// 返回缺省为 null 的 params。
    pub(super) fn params(&self) -> &Value {
        self.0.get("params").unwrap_or(&Value::Null)
    }

    /// 返回完整 payload，保留未知字段。
    pub(super) fn as_value(&self) -> &Value {
        &self.0
    }

    /// 转移完整 payload 给需要动态字段的 facade。
    pub(super) fn into_value(self) -> Value {
        self.0
    }
}

/// 保留完整 payload 的 server request；回复策略由 Codex facade 决定。
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ServerRequest(Value);

impl ServerRequest {
    /// 从带 id/method 的 object 构造 server request。
    fn from_value(value: Value) -> Result<Self, RpcFailure> {
        let object = value.as_object().ok_or(RpcFailure::Protocol)?;
        let id = object.get("id").ok_or(RpcFailure::Protocol)?;
        if !valid_id(id) || object.get("method").and_then(Value::as_str).is_none() {
            return Err(RpcFailure::Protocol);
        }
        Ok(Self(value))
    }

    /// 返回 server request id，供 facade 构造同 id 回复。
    pub(super) fn id_value(&self) -> Value {
        self.0["id"].clone()
    }

    /// 返回 server request 方法名。
    pub(super) fn method(&self) -> &str {
        self.0["method"].as_str().unwrap_or_default()
    }

    /// 返回缺省为 null 的 params。
    pub(super) fn params(&self) -> &Value {
        self.0.get("params").unwrap_or(&Value::Null)
    }
}

/// JSON-RPC wire message 的三种双向形状。
#[derive(Clone, Debug, PartialEq)]
pub(super) enum IncomingMessage {
    /// 与 request correlation 的 response。
    Response(ResponseEnvelope),
    /// server 推送的 notification。
    Notification(Notification),
    /// server 发起、由 facade 回复的 request。
    ServerRequest(ServerRequest),
}

impl IncomingMessage {
    /// 按 id/method 字段分类并校验当前协议所需字段。
    pub(super) fn decode(value: Value) -> Result<Self, RpcFailure> {
        let object = value.as_object().ok_or(RpcFailure::Protocol)?;
        match (object.contains_key("id"), object.contains_key("method")) {
            (true, true) => ServerRequest::from_value(value).map(Self::ServerRequest),
            (true, false) => ResponseEnvelope::from_value(value).map(Self::Response),
            (false, true) => Notification::from_value(value).map(Self::Notification),
            (false, false) => Err(RpcFailure::Protocol),
        }
    }
}

/// session 机制错误；远程 error 不在此处归为 signed-out 等业务类别。
#[derive(Clone, Debug, PartialEq)]
pub(super) enum RpcSessionError {
    /// I/O、取消、deadline 或 envelope 机制失败。
    Failure(RpcFailure),
    /// 已提取的远程 RPC error，交给 facade 分类。
    Remote(RemoteRpcError),
}

/// 单连接的顺序 RPC 状态；不保存并发 pending map。
pub(crate) struct RpcSession {
    /// 下一个顺序 request id。
    next_request_id: u64,
    /// response 等待期间暂存的 notification。
    pending_notifications: VecDeque<Notification>,
}

impl RpcSession {
    /// 创建从数字 id 1 开始的空 session。
    pub(super) fn new() -> Self {
        Self {
            next_request_id: 1,
            pending_notifications: VecDeque::new(),
        }
    }

    /// 取出最早到达的缓冲 notification。
    pub(super) fn pop_notification(&mut self) -> Option<Notification> {
        self.pending_notifications.pop_front()
    }

    /// 发送 notification；I/O 取消和 deadline 仍由 JsonlConnection 负责。
    pub(super) async fn notification(
        &mut self,
        connection: &mut JsonlConnection,
        method: &str,
        params: Value,
        deadline: Instant,
        cancellation: Option<&CancellationSignal>,
    ) -> Result<(), RpcSessionError> {
        let value = serde_json::to_value(NotificationEnvelope::new(method, params))
            .map_err(|_| RpcSessionError::Failure(RpcFailure::Protocol))?;
        connection
            .write_message(value, deadline, cancellation)
            .await
            .map_err(RpcSessionError::Failure)
    }

    /// 发送 request，顺序等待匹配 response，并把 server request 交给 facade handler。
    pub(super) async fn request<F>(
        &mut self,
        connection: &mut JsonlConnection,
        method: &str,
        params: Value,
        deadline: Instant,
        cancellation: Option<&CancellationSignal>,
        mut on_server_request: F,
    ) -> Result<Value, RpcSessionError>
    where
        F: FnMut(&ServerRequest) -> Result<Value, RpcSessionError>,
    {
        if cancellation.is_some_and(CancellationSignal::is_cancelled) {
            return Err(RpcSessionError::Failure(RpcFailure::Cancelled));
        }
        let request_number = self.next_request_id;
        let request_id = Value::from(request_number);
        self.next_request_id = self.next_request_id.saturating_add(1);
        let request = RequestEnvelope::new(request_number, method, params);
        let value = serde_json::to_value(request)
            .map_err(|_| RpcSessionError::Failure(RpcFailure::Protocol))?;
        connection
            .write_message(value, deadline, cancellation)
            .await
            .map_err(RpcSessionError::Failure)?;

        loop {
            let message = read_message(connection, deadline, cancellation).await?;
            match message {
                IncomingMessage::Response(response) if response.id() == &request_id => {
                    return response.into_result();
                }
                IncomingMessage::Response(_) => {
                    // 旧 response 不属于唯一 active request，安全忽略。
                }
                IncomingMessage::Notification(notification) => {
                    self.pending_notifications.push_back(notification);
                }
                IncomingMessage::ServerRequest(request) => {
                    let response = on_server_request(&request)?;
                    write_message(connection, response, deadline, cancellation).await?;
                }
            }
        }
    }

    /// 读取下一条 notification，并在途中处理 facade 提供的 server-request 回复策略。
    pub(super) async fn next_notification<F>(
        &mut self,
        connection: &mut JsonlConnection,
        deadline: Instant,
        cancellation: Option<&CancellationSignal>,
        mut on_server_request: F,
    ) -> Result<Notification, RpcSessionError>
    where
        F: FnMut(&ServerRequest) -> Result<Value, RpcSessionError>,
    {
        if let Some(notification) = self.pop_notification() {
            return Ok(notification);
        }
        loop {
            match read_message(connection, deadline, cancellation).await? {
                IncomingMessage::Notification(notification) => return Ok(notification),
                IncomingMessage::Response(_) => {
                    // 没有 active request 时，response 不属于当前等待。
                }
                IncomingMessage::ServerRequest(request) => {
                    let response = on_server_request(&request)?;
                    write_message(connection, response, deadline, cancellation).await?;
                }
            }
        }
    }
}

/// 判断 wire id 是否是 JSON-RPC 允许的字符串或数字。
fn valid_id(value: &Value) -> bool {
    value.is_string() || value.is_number()
}

/// 从 JsonlConnection 读取并完成 typed message 分类。
async fn read_message(
    connection: &mut JsonlConnection,
    deadline: Instant,
    cancellation: Option<&CancellationSignal>,
) -> Result<IncomingMessage, RpcSessionError> {
    let value = connection
        .read_message(deadline, cancellation)
        .await
        .map_err(RpcSessionError::Failure)?;
    IncomingMessage::decode(value).map_err(RpcSessionError::Failure)
}

/// 写入 facade 为 server request 选择的回复。
async fn write_message(
    connection: &mut JsonlConnection,
    value: Value,
    deadline: Instant,
    cancellation: Option<&CancellationSignal>,
) -> Result<(), RpcSessionError> {
    connection
        .write_message(value, deadline, cancellation)
        .await
        .map_err(RpcSessionError::Failure)
}

#[cfg(test)]
#[path = "../../tests/codex/transport/rpc_session.rs"]
mod tests;
