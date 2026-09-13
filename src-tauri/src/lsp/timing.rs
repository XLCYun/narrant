//! 按方法汇总真实请求耗时；不保留参数、响应内容或文档 URI。

use serde_json::Value;
use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

/// 一种请求的累计值，空间只随方法种类增长。
#[derive(Default)]
struct RequestTiming {
    count: u64,
    failed: u64,
    total: Duration,
    longest: Duration,
}

/// 会话退出或失败时输出汇总，并用标准 work-done 通知区分服务器加载阶段。
#[derive(Default)]
pub(super) struct SessionTiming {
    requests: BTreeMap<String, RequestTiming>,
    progress: BTreeMap<String, (String, Instant)>,
}

impl SessionTiming {
    /// 一次调用只计入一个方法；慢请求另外保留请求序号便于定位首次语义计算。
    pub fn request(&mut self, method: &str, id: u64, elapsed: Duration, failed: bool) {
        let timing = self.requests.entry(method.to_owned()).or_default();
        timing.count += 1;
        timing.failed += u64::from(failed);
        timing.total += elapsed;
        timing.longest = timing.longest.max(elapsed);
        if elapsed >= Duration::from_secs(1) {
            tracing::info!(target: "narrant::lsp", event = "slow_request", method, request_id = id,
                elapsed_ms = elapsed.as_secs_f64() * 1000.0, success = !failed, "LSP 慢请求");
        }
    }

    /// 只记录进度标题与起止，忽略可能包含路径、源码或构建输出的 message 字段。
    pub fn progress(&mut self, params: &Value) {
        let Some(token) = params.get("token") else {
            return;
        };
        let key = token.to_string();
        let value = &params["value"];
        match value["kind"].as_str() {
            Some("begin") => {
                let title = value["title"].as_str().unwrap_or("server work").to_owned();
                tracing::info!(target: "narrant::lsp", event = "server_progress_started", title, "LSP 服务端阶段开始");
                self.progress.insert(key, (title, Instant::now()));
            }
            Some("end") => {
                if let Some((title, started)) = self.progress.remove(&key) {
                    tracing::info!(target: "narrant::lsp", event = "server_progress_finished", title,
                        elapsed_ms = started.elapsed().as_secs_f64() * 1000.0, "LSP 服务端阶段结束");
                }
            }
            _ => {}
        }
    }
}

impl Drop for SessionTiming {
    /// 汇总包含失败请求；请求内部的服务器进度不能再与请求耗时相加。
    fn drop(&mut self) {
        for (method, timing) in &self.requests {
            tracing::info!(target: "narrant::lsp", event = "request_summary", method,
                requests = timing.count, failed = timing.failed,
                total_ms = timing.total.as_secs_f64() * 1000.0,
                max_ms = timing.longest.as_secs_f64() * 1000.0,
                mean_ms = timing.total.as_secs_f64() * 1000.0 / timing.count as f64,
                "LSP 请求耗时汇总");
        }
        for (title, started) in self.progress.values() {
            tracing::info!(target: "narrant::lsp", event = "server_progress_incomplete", title,
                elapsed_ms = started.elapsed().as_secs_f64() * 1000.0, "LSP 服务端阶段未完成");
        }
    }
}
