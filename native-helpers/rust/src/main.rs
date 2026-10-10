//! stdout 只发送协议，查询失败整次退出；忙碌期间取消由公共 Host 的 ProcessTree 有界回收。
use narrant_rust_core::{
    projection,
    protocol::{PROTOCOL, Request},
};
use serde_json::json;
use std::io::{BufRead, Write};

/// 每个进程只分析一个固定侧，不复用数据库或建立服务器会话。
fn main() -> anyhow::Result<()> {
    send(
        &json!({"protocol":PROTOCOL,"type":"ready","helper_id":"narrant-rust","helper_version":"0.1.0","projection_revision":"rust-source-v1","languages":["rust"],"profiles":["rust-repository-v1","rust-standard-library-v1","rust-all-dependencies-v1"]}),
    )?;
    let mut line = String::new();
    std::io::stdin().lock().read_line(&mut line)?;
    let request = serde_json::from_str::<Request>(&line);
    let id = request
        .as_ref()
        .map(|request| request.id.as_str())
        .unwrap_or("analysis-1");
    let result = request
        .as_ref()
        .map_err(|error| anyhow::anyhow!(error.to_string()))
        .and_then(projection::analyze);
    let completed = match result {
        Ok(mut result) => {
            let object = result.as_object_mut().expect("投影必须返回对象");
            object.extend([
                ("protocol".into(), json!(PROTOCOL)),
                ("type".into(), json!("completed")),
                ("id".into(), json!(id)),
                ("outcome".into(), json!("ok")),
            ]);
            if serde_json::to_vec(&result)?.len()
                > request.as_ref().unwrap().limits.max_result_bytes
            {
                failed(id, "分析结果超过协议限额")
            } else {
                result
            }
        }
        Err(error) => {
            eprintln!("{error:#}");
            failed(id, &format!("{error:#}"))
        }
    };
    send(&completed)
}
/// 输出唯一错误终结，不混入部分图或成功字段。
fn failed(id: &str, message: &str) -> serde_json::Value {
    json!({"protocol":PROTOCOL,"type":"completed","id":id,"outcome":"error","error":{"code":"object_extraction_failed","message":message,"paths":[]}})
}
/// 一行一条 JSON 并立即 flush，便于 Host 验证严格状态机。
fn send(value: &serde_json::Value) -> anyhow::Result<()> {
    let mut output = std::io::stdout().lock();
    serde_json::to_writer(&mut output, value)?;
    writeln!(output)?;
    output.flush()?;
    Ok(())
}
