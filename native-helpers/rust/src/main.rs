//! Rust 固定 ready 描述和语言投影，控制/退出循环与 Python 共用。
use narrant_rust_core::{
    projection,
    protocol::{PROTOCOL, Request},
};
use serde_json::json;

/// 每个进程只分析一个固定侧，不复用数据库或建立服务器会话。
fn main() -> anyhow::Result<()> {
    narrant_native_common::one_shot::run::<Request>(
        json!({"protocol":PROTOCOL,"type":"ready","helper_id":"narrant-rust","helper_version":"0.1.0","projection_revision":"rust-source-v1","languages":["rust"],"profiles":["rust-repository-v1","rust-standard-library-v1","rust-all-dependencies-v1"]}),
        |request| {
            anyhow::ensure!(
                request.protocol == PROTOCOL
                    && request.message_type == "analyze"
                    && !request.id.is_empty(),
                "无效初始分析请求"
            );
            Ok(&request.id)
        },
        |request| request.limits.max_result_bytes,
        projection::analyze,
    )
}
