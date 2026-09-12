//! Rust 专用的 LSP Adapter：配置、能力要求和项目就绪规则集中在这里。

use std::{path::Path, process::Command, time::Duration};

use serde_json::{json, Value};

use super::{failure, Result};
use crate::lsp::{Location, LspClient, LspOptions, OutgoingCall, Position};

const SERVER_STATUS: &str = "experimental/serverStatus";
const LOAD_TIMEOUT: Duration = Duration::from_secs(180);

/// 在通用会话上提供 Rust 语义查询，保证构图阶段始终使用完整加载的项目。
pub(super) struct RustLsp {
    client: LspClient,
    ready: bool,
}

impl RustLsp {
    /// 使用 Rust 初始化约定建立会话，并检查本分析器依赖的能力。
    pub fn start(executable: &Path, repository: &Path) -> Result<Self> {
        let mut options = LspOptions {
            initialization_options: json!({
                "checkOnSave": false,
                "cachePriming": {"enable": false},
                "diagnostics": {"enable": false}
            }),
            ..LspOptions::default()
        };
        options.capabilities["experimental"] = json!({"serverStatusNotification": true});
        let client =
            LspClient::start(Command::new(executable), repository, options).map_err(failure)?;
        let capabilities = client.capabilities();
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
        Ok(Self {
            client,
            ready: false,
        })
    }

    /// 在开始构图前同步 Rust 文档，随后保持选中源码快照不变。
    pub fn open(&mut self, uri: &str, source: &str) -> Result<()> {
        if self.ready {
            return Err(failure("Rust LSP 已开始分析，不能再打开文档"));
        }
        self.client
            .open_document(uri, "rust", 1, source)
            .map_err(failure)
    }

    /// 解释 rust-analyzer 的状态扩展，warning 也不能充当完整加载。
    pub fn wait_ready(&mut self) -> Result<()> {
        self.client
            .wait_for_notification(SERVER_STATUS, LOAD_TIMEOUT, |status| {
                status["quiescent"] == true
            })
            .map_err(failure)?;
        self.ensure_healthy()?;
        self.ready = true;
        Ok(())
    }

    /// 查询源码引用的定义，协议位置响应已由通用层规范化。
    pub fn definitions(&mut self, uri: &str, position: Position) -> Result<Vec<Location>> {
        self.query(|client| client.definitions(uri, position))
    }

    /// 获取文件的全部父模块，包括 #[path] 和同一文件的多个模块归属。
    pub fn parent_modules(&mut self, uri: &str) -> Result<Vec<Location>> {
        self.query(|client| {
            client.request_locations(
                "experimental/parentModule",
                json!({
                    "textDocument": {"uri": uri}, "position": Position::default()
                }),
            )
        })
    }

    /// 取得函数的出向调用，包括服务器能定位到原源码的宏展开调用。
    pub fn outgoing_calls(&mut self, uri: &str, position: Position) -> Result<Vec<OutgoingCall>> {
        self.query(|client| client.outgoing_calls(uri, position))
    }

    /// 结束本次仓库快照会话，底层负责正常协议及异常回收。
    pub fn shutdown(self) -> Result<()> {
        self.client.shutdown().map_err(failure)
    }

    /// 所有语义查询共享 Rust 就绪检查，协议错误与状态降级都向上传播。
    fn query<T>(
        &mut self,
        query: impl FnOnce(&mut LspClient) -> crate::lsp::Result<T>,
    ) -> Result<T> {
        if !self.ready {
            return Err(failure("Rust LSP 项目尚未就绪"));
        }
        self.ensure_healthy()?;
        let result = query(&mut self.client).map_err(failure)?;
        self.ensure_healthy()?;
        Ok(result)
    }

    /// 仅 Rust Adapter 知道 health 和 quiescent 字段的含义。
    fn ensure_healthy(&self) -> Result<()> {
        let status = self
            .client
            .latest_notification(SERVER_STATUS)
            .unwrap_or(&Value::Null);
        if status["health"] != "ok" || status["quiescent"] != true {
            return Err(failure(format!("rust-analyzer 项目未完整加载：{status}")));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/lsp.rs"]
mod tests;
