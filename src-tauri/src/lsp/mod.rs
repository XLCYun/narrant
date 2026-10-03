//! 可复用的同步 stdio LSP 会话；语言配置与服务器扩展由各语言 Adapter 决定。

mod protocol;
mod transport;

use std::{
    collections::BTreeMap,
    fmt::Display,
    path::Path,
    process::Command,
    sync::Arc,
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use url::Url;

pub use protocol::{CallTarget, Location, OutgoingCall, Position, Range};
#[cfg(unix)]
pub(crate) use transport::group_members as native_group_members;
#[cfg(all(test, windows))]
pub(crate) use transport::windows_validation;
use transport::Transport;

/// 会话或协议失败；不依赖代码图的错误类型。
#[derive(Debug)]
pub struct LspError {
    pub message: String,
    cancelled: bool,
}

impl LspError {
    /// 将明确的本地取消与 I/O、协议和服务器错误区分。
    fn cancelled() -> Self {
        Self {
            message: "LSP 操作已取消".into(),
            cancelled: true,
        }
    }

    /// 供语言 Adapter 保留取消类别，不再依赖错误文案或后到的取消请求。
    pub fn is_cancelled(&self) -> bool {
        self.cancelled
    }

    /// 添加请求上下文时保留原始错误的取消语义。
    fn context(mut self, context: impl Display) -> Self {
        self.message = format!("{context}：{}", self.message);
        self
    }
}

impl Display for LspError {
    /// 保留底层错误文字供语言 Adapter 添加上下文。
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for LspError {}

/// 通用 LSP 操作的结果，语言 Adapter 可在自己的 seam 转换错误。
pub type Result<T> = std::result::Result<T, LspError>;

/// 只观察调用方的取消状态；通用会话不拥有应用操作的取消触发权。
pub type CancellationCheck = Arc<dyn Fn() -> bool + Send + Sync>;

/// 原样传给服务器的能力及初始化选项；超时限制一次请求的总等待时间。
#[derive(Clone)]
pub struct LspOptions {
    pub capabilities: Value,
    pub initialization_options: Value,
    pub request_timeout: Duration,
    pub cancelled: CancellationCheck,
}

impl Default for LspOptions {
    /// 默认提供 UTF-16 位置、定义链接和调用层级支持，不要求服务器实现任何语言扩展。
    fn default() -> Self {
        Self {
            capabilities: json!({
                "general": {"positionEncodings": ["utf-16"]},
                "workspace": {"workspaceFolders": true},
                "textDocument": {"definition": {"linkSupport": true}, "callHierarchy": {}},
                "window": {"workDoneProgress": true}
            }),
            initialization_options: Value::Null,
            request_timeout: Duration::from_secs(90),
            cancelled: Arc::new(|| false),
        }
    }
}

/// 独占服务器进程，隐藏初始化顺序、消息分帧、请求匹配及反向请求处理。
///
/// 标准握手成功不代表语言项目已经加载；语言 Adapter 自行解释扩展状态。
/// 扩展通知按方法名保留最近一次参数，供状态查询使用，不作为完整事件历史。
pub struct LspClient {
    transport: Transport,
    capabilities: Value,
    notifications: BTreeMap<String, Value>,
    next_id: u64,
    request_timeout: Duration,
    cancelled: CancellationCheck,
}

impl LspClient {
    /// 仅测试借用当前会话查询准确 HANDLE 的 Job 归属，不向调用方暴露 Job 句柄。
    #[cfg(all(test, windows))]
    pub(crate) fn windows_owner_contains(
        &self,
        process: &std::os::windows::io::OwnedHandle,
    ) -> std::io::Result<bool> {
        self.transport.windows_owner_contains(process)
    }

    /// 仅测试读取当前会话的共享 owner 统计，正常请求与清理均不自动采样。
    #[cfg(all(test, windows))]
    pub(crate) fn windows_owner_snapshot(
        &self,
    ) -> std::io::Result<windows_validation::OwnerSnapshot> {
        self.transport.windows_owner_snapshot()
    }

    /// 启动调用方提供的命令并完成 initialize/initialized；命令可携带语言所需参数和环境。
    pub fn start(mut command: Command, workspace: &Path, options: LspOptions) -> Result<Self> {
        let uri = file_uri(workspace)?;
        command.current_dir(workspace);
        let mut client = Self {
            transport: Transport::start(command)?,
            capabilities: Value::Null,
            notifications: BTreeMap::new(),
            next_id: 0,
            request_timeout: options.request_timeout,
            cancelled: options.cancelled,
        };
        let initialized = client.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "clientInfo": {"name": "narrant-lsp"},
                "rootUri": uri,
                "workspaceFolders": [{"uri": uri, "name": "workspace"}],
                "capabilities": options.capabilities,
                "initializationOptions": options.initialization_options
            }),
        )?;
        client.capabilities = initialized
            .get("capabilities")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or_else(|| failure("LSP initialize 缺少 capabilities 对象"))?;
        client.notify("initialized", json!({}))?;
        Ok(client)
    }

    /// 返回服务器实际报告的能力；某个语言分析器需要哪些能力由该 Adapter 校验。
    pub fn capabilities(&self) -> &Value {
        &self.capabilities
    }

    /// 同步一份文档；语言名称和版本由调用方明确提供。
    pub fn open_document(
        &mut self,
        uri: &str,
        language: &str,
        version: i32,
        source: &str,
    ) -> Result<()> {
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri, "languageId": language, "version": version, "text": source
            }}),
        )
    }

    /// 发出标准或扩展请求；处理等待期间到达的通知，错误响应不会变成空结果。
    pub fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        let deadline = Instant::now() + self.request_timeout;
        self.transport.send(
            json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}),
            self.request_timeout,
            &self.cancelled,
        )?;
        #[cfg(all(test, windows))]
        windows_validation::post_send(method);
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| failure(format!("LSP 请求超时：{method}")))?;
            let message = self
                .receive(remaining)
                .map_err(|error| error.context(format!("LSP {method} 等待响应失败")))?;
            if message.get("method").is_none() && message["id"] == id {
                if let Some(error) = message.get("error") {
                    return Err(failure(format!("LSP {method} 失败：{error}")));
                }
                return message
                    .get("result")
                    .cloned()
                    .ok_or_else(|| failure(format!("LSP {method} 缺少 result")));
            }
        }
    }

    /// 发出不要求响应的标准或扩展通知。
    pub fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.transport.send(
            json!({"jsonrpc": "2.0", "method": method, "params": params}),
            self.request_timeout,
            &self.cancelled,
        )
    }

    /// 查询某方法最后收到的通知，便于 Adapter 检查请求期间发生的服务器状态变化。
    pub fn latest_notification(&self, method: &str) -> Option<&Value> {
        self.notifications.get(method)
    }

    /// 等待扩展状态满足调用方条件；通用层不解释通知字段，也不重置固定截止时间。
    pub fn wait_for_notification(
        &mut self,
        method: &str,
        timeout: Duration,
        ready: impl Fn(&Value) -> bool,
    ) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(params) = self
                .notifications
                .get(method)
                .filter(|params| ready(params))
            {
                return Ok(params.clone());
            }
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| failure(format!("等待 LSP 通知超时：{method}")))?;
            self.receive(remaining)
                .map_err(|error| error.context(format!("等待 LSP {method} 失败")))?;
        }
    }

    /// 查询定义，并统一 Location、LocationLink、单项及空响应。
    pub fn definitions(&mut self, uri: &str, position: Position) -> Result<Vec<Location>> {
        self.request_locations(
            "textDocument/definition",
            json!({"textDocument": {"uri": uri}, "position": position}),
        )
    }

    /// 标准及语言扩展中的位置查询共用响应校验，保持语义层不接触协议分支。
    pub fn request_locations(&mut self, method: &str, params: Value) -> Result<Vec<Location>> {
        protocol::locations(self.request(method, params)?)
    }

    /// 完成两步调用层级查询，原样保留服务器用于第二步的 opaque data。
    pub fn outgoing_calls(&mut self, uri: &str, position: Position) -> Result<Vec<OutgoingCall>> {
        let prepared = self.request(
            "textDocument/prepareCallHierarchy",
            json!({"textDocument": {"uri": uri}, "position": position}),
        )?;
        let mut calls = Vec::new();
        for item in protocol::optional_array(&prepared, "prepareCallHierarchy")? {
            let response = self.request("callHierarchy/outgoingCalls", json!({"item": item}))?;
            for call in protocol::optional_array(&response, "outgoingCalls")? {
                calls.push(serde_json::from_value(call.clone()).map_err(failure)?);
            }
        }
        Ok(calls)
    }

    /// 消耗会话并退出；失败或提前返回时仍由进程守卫回收整个进程组。
    pub fn shutdown(mut self) -> Result<()> {
        self.request("shutdown", Value::Null)?;
        self.notify("exit", Value::Null)?;
        self.transport.finish()
    }

    /// 处理标准反向请求并保存扩展通知；未实现的反向请求明确返回 MethodNotFound。
    fn receive(&mut self, timeout: Duration) -> Result<Value> {
        let deadline = Instant::now() + timeout;
        let message = self.transport.receive(timeout, &self.cancelled)?;
        if !message.is_object() || message["jsonrpc"] != "2.0" {
            return Err(failure("LSP 收到无效的 JSON-RPC 消息"));
        }
        if let Some(method) = message["method"].as_str() {
            if let Some(id) = message.get("id") {
                let response = match method {
                    "window/workDoneProgress/create"
                    | "client/registerCapability"
                    | "client/unregisterCapability" => {
                        json!({"jsonrpc": "2.0", "id": id, "result": null})
                    }
                    _ => {
                        json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "Unsupported client request"}})
                    }
                };
                let remaining = deadline
                    .checked_duration_since(Instant::now())
                    .ok_or_else(|| failure("LSP 反向请求处理超时"))?;
                self.transport.send(response, remaining, &self.cancelled)?;
            } else {
                self.notifications
                    .insert(method.to_owned(), message["params"].clone());
            }
        }
        Ok(message)
    }
}

/// 将本地路径转为 URI，正确处理空格及非 ASCII 字符。
pub fn file_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(String::from)
        .map_err(|_| failure("路径无法转换为 file URI"))
}

/// 保持协议和 I/O 错误的文字上下文，不引用具体语言的错误类型。
fn failure(message: impl Display) -> LspError {
    LspError {
        message: message.to_string(),
        cancelled: false,
    }
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;

#[cfg(all(test, unix))]
#[path = "tests/support.rs"]
pub(crate) mod test_support;
