//! 本次快照独占的 stdio LSP 会话；协议和进程生命周期不进入公共图模型。

use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, SyncSender},
    time::{Duration, Instant},
};

use serde_json::{json, Value};
use url::Url;

use super::{failure, Result};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(90);
const LOAD_TIMEOUT: Duration = Duration::from_secs(180);
const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

/// 写线程回传发送结果，避免服务器停止读取时卡住分析调用线程。
type Outgoing = (Vec<u8>, mpsc::Sender<Result<()>>);

/// 回收服务器及其启动的构建进程，覆盖初始化失败等提前返回路径。
struct ServerProcess(Child);

impl Drop for ServerProcess {
    /// 回收本次服务器的进程树，并等待直接子进程退出。
    fn drop(&mut self) {
        #[cfg(unix)]
        // SAFETY: 该子进程由下方 process_group(0) 创建，负 PID 只指向本次会话的进程组。
        unsafe {
            libc::kill(-(self.0.id() as i32), libc::SIGKILL);
        }
        #[cfg(windows)]
        {
            let mut command = Command::new("taskkill");
            command
                .args(["/PID", &self.0.id().to_string(), "/T", "/F"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            crate::child_process::configure_std_command(&mut command);
            let _ = command.status();
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// 顺序执行查询，并在等待响应时处理服务器状态和反向请求。
pub(super) struct Lsp {
    _process: ServerProcess,
    outgoing: SyncSender<Outgoing>,
    incoming: Receiver<Result<Value>>,
    next_id: u64,
    status: Value,
    ready: bool,
}

impl Lsp {
    /// 启动已有二进制；项目发现、Cargo 配置与宏加载由服务器负责。
    pub fn start(executable: &Path, repository: &Path) -> Result<Self> {
        let mut command = Command::new(executable);
        command
            .current_dir(repository)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        crate::child_process::configure_std_command(&mut command);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut process = ServerProcess(command.spawn().map_err(|error| {
            failure(format!(
                "无法启动 rust-analyzer {}：{error}",
                executable.display()
            ))
        })?);
        let mut stdin = process
            .0
            .stdin
            .take()
            .ok_or_else(|| failure("rust-analyzer 缺少输入流"))?;
        let stdout = process
            .0
            .stdout
            .take()
            .ok_or_else(|| failure("rust-analyzer 缺少输出流"))?;
        let (sender, incoming) = mpsc::sync_channel(64);
        std::thread::Builder::new()
            .name("rust-code-graph-lsp".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                loop {
                    let message = read_message(&mut reader);
                    let failed = message.is_err();
                    if sender.send(message).is_err() || failed {
                        break;
                    }
                }
            })
            .map_err(failure)?;
        let (outgoing, writes) = mpsc::sync_channel::<Outgoing>(1);
        std::thread::Builder::new()
            .name("rust-code-graph-lsp-write".into())
            .spawn(move || {
                while let Ok((bytes, complete)) = writes.recv() {
                    let result = (|| {
                        write!(stdin, "Content-Length: {}\r\n\r\n", bytes.len())?;
                        stdin.write_all(&bytes)?;
                        stdin.flush()
                    })()
                    .map_err(failure);
                    let failed = result.is_err();
                    if complete.send(result).is_err() || failed {
                        break;
                    }
                }
            })
            .map_err(failure)?;
        let mut client = Self {
            _process: process,
            outgoing,
            incoming,
            next_id: 0,
            status: Value::Null,
            ready: false,
        };
        let uri = file_uri(repository)?;
        let initialized = client.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "clientInfo": {"name": "narrant-code-graph"},
                "rootUri": uri,
                "workspaceFolders": [{"uri": uri, "name": "code-graph"}],
                "capabilities": {
                    "general": {"positionEncodings": ["utf-16"]},
                    "workspace": {"workspaceFolders": true},
                    "textDocument": {"definition": {"linkSupport": true}, "callHierarchy": {}},
                    "window": {"workDoneProgress": true},
                    "experimental": {"serverStatusNotification": true}
                },
                "initializationOptions": {
                    "checkOnSave": false,
                    "cachePriming": {"enable": false},
                    "diagnostics": {"enable": false}
                }
            }),
        )?;
        let capabilities = &initialized["capabilities"];
        for capability in ["definitionProvider", "callHierarchyProvider"] {
            if capabilities[capability] != true && !capabilities[capability].is_object() {
                return Err(failure(format!(
                    "rust-analyzer 未提供必需能力：{capability}"
                )));
            }
        }
        if capabilities["experimental"]["parentModule"] != true {
            return Err(failure("rust-analyzer 未提供必需的 parentModule 能力"));
        }
        if capabilities
            .get("positionEncoding")
            .is_some_and(|encoding| encoding != "utf-16")
        {
            return Err(failure("rust-analyzer 未采用协商的 UTF-16 位置编码"));
        }
        client.notify("initialized", json!({}))?;
        Ok(client)
    }

    /// 同步选中文件的固定内容，不复用用户编辑器中的缓冲区。
    pub fn open(&mut self, uri: &str, source: &str) -> Result<()> {
        self.notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": uri, "languageId": "rust", "version": 1, "text": source
            }}),
        )
    }

    /// 等待项目及构建数据加载收敛，不能把降级加载当作完整分析。
    pub fn wait_ready(&mut self) -> Result<()> {
        let deadline = Instant::now() + LOAD_TIMEOUT;
        while self.status["quiescent"] != true {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| failure("等待 rust-analyzer 加载项目超时"))?;
            self.receive(remaining)?;
        }
        self.ensure_healthy()?;
        self.ready = true;
        Ok(())
    }

    /// 已知加载问题必须显式失败；单个查询未找到目标由上层记录 Unresolved。
    fn ensure_healthy(&self) -> Result<()> {
        if self.status["health"] != "ok" || self.status["quiescent"] != true {
            return Err(failure(format!(
                "rust-analyzer 项目未完整加载：{}",
                self.status
            )));
        }
        Ok(())
    }

    /// Content-Length 按 UTF-8 字节计数，避免中文和 emoji 导致协议错帧。
    fn send(&mut self, message: Value) -> Result<()> {
        let bytes = serde_json::to_vec(&message).map_err(failure)?;
        let (complete, completed) = mpsc::channel();
        self.outgoing.try_send((bytes, complete)).map_err(failure)?;
        completed
            .recv_timeout(REQUEST_TIMEOUT)
            .map_err(|error| failure(format!("向 rust-analyzer 发送消息失败：{error}")))?
    }

    /// 发送不需要响应的文档/生命周期通知。
    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    /// 只接受与当前请求匹配的成功响应；传输和协议错误不会变成空结果。
    pub fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        self.request_with_timeout(method, params, REQUEST_TIMEOUT)
    }

    /// 每个请求使用固定截止时间，持续到达的通知也不能无限延长等待。
    fn request_with_timeout(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| failure(format!("LSP 请求超时：{method}")))?;
            let message = self
                .receive(remaining)
                .map_err(|error| failure(format!("LSP {method} 等待响应失败：{error}")))?;
            if message.get("method").is_none() && message["id"] == id {
                if let Some(error) = message.get("error") {
                    return Err(failure(format!("LSP {method} 失败：{error}")));
                }
                if self.ready {
                    self.ensure_healthy()?;
                }
                return message
                    .get("result")
                    .cloned()
                    .ok_or_else(|| failure(format!("LSP {method} 缺少 result")));
            }
        }
    }

    /// 接收消息并回应进度注册等服务器请求，未知请求显式拒绝。
    fn receive(&mut self, timeout: Duration) -> Result<Value> {
        let message = self.incoming.recv_timeout(timeout).map_err(failure)??;
        if let Some(method) = message["method"].as_str() {
            if method == "experimental/serverStatus" {
                self.status = message["params"].clone();
            }
            if let Some(id) = message.get("id") {
                let response = match method {
                    "window/workDoneProgress/create"
                    | "client/registerCapability"
                    | "client/unregisterCapability" => {
                        json!({"jsonrpc":"2.0", "id":id, "result":null})
                    }
                    _ => json!({"jsonrpc":"2.0", "id":id, "error":{
                        "code":-32601, "message":"Narrant code graph does not support this request"
                    }}),
                };
                self.send(response)?;
            }
        }
        Ok(message)
    }

    /// 发送正常退出协议；进程守卫同时兜底处理异常退出。
    pub fn shutdown(&mut self) -> Result<()> {
        self.request("shutdown", Value::Null)?;
        self.notify("exit", Value::Null)
    }
}

/// 对包含空格、非 ASCII 字符的本地路径进行标准 file URI 编码。
pub(super) fn file_uri(path: &Path) -> Result<String> {
    Url::from_file_path(path)
        .map(String::from)
        .map_err(|_| failure("路径无法转换为 file URI"))
}

/// 读取有明确大小上限的 LSP 消息，拒绝缺失、重复或畸形的长度头。
fn read_message(reader: &mut impl BufRead) -> Result<Value> {
    let mut length = None;
    let mut header_bytes = 0;
    loop {
        let mut line = String::new();
        let count = reader.read_line(&mut line).map_err(failure)?;
        if count == 0 {
            return Err(failure("rust-analyzer 关闭了输出流"));
        }
        header_bytes += count;
        if header_bytes > 16 * 1024 {
            return Err(failure("LSP 消息头过长"));
        }
        if line == "\r\n" || line == "\n" {
            break;
        }
        let (key, value) = line
            .split_once(':')
            .ok_or_else(|| failure("LSP 消息头格式错误"))?;
        if key.eq_ignore_ascii_case("Content-Length") {
            if length.is_some() {
                return Err(failure("LSP Content-Length 重复"));
            }
            length = Some(value.trim().parse::<usize>().map_err(failure)?);
        }
    }
    let length = length
        .filter(|length| *length <= MAX_MESSAGE_BYTES)
        .ok_or_else(|| failure("LSP 消息缺少有效 Content-Length 或消息过大"))?;
    let mut body = vec![0; length];
    reader.read_exact(&mut body).map_err(failure)?;
    serde_json::from_slice(&body).map_err(failure)
}

#[cfg(test)]
#[path = "tests/lsp.rs"]
mod tests;
