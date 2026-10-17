//! Python 原生数据库和图投影通过公共单次控制循环输出。
use narrant_python_core::{
    projection,
    protocol::{PROTOCOL, Request},
};
use serde_json::json;

/// 每个进程只加载一侧显式配置的原生数据库，不建立服务或默认环境。
fn main() -> anyhow::Result<()> {
    narrant_native_common::one_shot::run::<Request>(
        json!({"protocol":PROTOCOL,"type":"ready","helper_id":"narrant-python","helper_version":"0.1.0","projection_revision":"python-source-v1","languages":["python"],"profiles":["python-static-v1"]}),
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
