//! 单次原生分析与控制输入分离；cancel/EOF 终止自身专属域，Host 保留最终退出屏障。
use narrant_rust_core::{
    projection,
    protocol::{PROTOCOL, Request},
};
use serde_json::json;
use std::{
    io::{BufRead, Write},
    sync::{Arc, Mutex},
};

// 直接复用宿主的跨平台所有权实现；本 helper 不调用其中的 Tokio/LSP 接口。
#[allow(dead_code)]
#[path = "../../../src-tauri/src/child_process.rs"]
mod child_process;

/// 每个进程只分析一个固定侧，不复用数据库或建立服务器会话。
fn main() -> anyhow::Result<()> {
    let domain = Arc::new(child_process::CurrentProcessDomain::establish()?);
    send(
        &json!({"protocol":PROTOCOL,"type":"ready","helper_id":"narrant-rust","helper_version":"0.1.0","projection_revision":"rust-source-v1","languages":["rust"],"profiles":["rust-repository-v1","rust-standard-library-v1","rust-all-dependencies-v1"]}),
    )?;
    let mut line = String::new();
    if std::io::stdin().lock().read_line(&mut line)? == 0 {
        domain.completed()?;
        return Ok(());
    }
    let request = serde_json::from_str::<Request>(&line);
    let id = request
        .as_ref()
        .map(|request| request.id.as_str())
        .unwrap_or("analysis-1");
    let terminal = Arc::new(Mutex::new(false));
    if let Ok(request) = &request {
        control(request.id.clone(), domain.clone(), terminal.clone())?;
    }
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
    // 终结提交与控制输入互斥；控制线程等锁有界，flush 完成之后不再接受取消/EOF。
    let mut ended = terminal
        .lock()
        .map_err(|_| anyhow::anyhow!("生命周期状态锁中毒"))?;
    domain.completed()?;
    *ended = true;
    send(&completed)
}

/// 后端内部阻塞 spawn 不支持协作取消，控制线程仍能独立终止完整隔离域。
fn control(
    id: String,
    domain: Arc<child_process::CurrentProcessDomain>,
    terminal: Arc<Mutex<bool>>,
) -> anyhow::Result<()> {
    std::thread::Builder::new()
        .name("rust-helper-control".into())
        .spawn(move || {
            let mut line = String::new();
            let _reason = match std::io::stdin().lock().read_line(&mut line) {
                Ok(0) => "Host stdin 在终结前关闭".to_owned(),
                Ok(_) => match narrant_rust_core::protocol::validate_cancel(&line, &id) {
                    Ok(()) => "收到取消请求".to_owned(),
                    Err(error) => format!("无效控制输入：{error:#}"),
                },
                Err(error) => format!("控制输入读取失败：{error}"),
            };
            // completed 的写入可能受 stdout 背压阻塞；控制线程不能无限等终结锁。
            // 正常 flush 后的 EOF 给主线程短暂释放锁的机会，避免误杀已完成的正常退出。
            let deadline = std::time::Instant::now() + std::time::Duration::from_millis(100);
            let _ending = loop {
                match terminal.try_lock() {
                    Ok(ended) => {
                        if *ended {
                            return;
                        }
                        break Some(ended);
                    }
                    Err(std::sync::TryLockError::Poisoned(_)) => break None,
                    Err(std::sync::TryLockError::WouldBlock) => {
                        if std::time::Instant::now() >= deadline {
                            break None;
                        }
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
            };
            // Host 可能已关闭 stderr，终止前不能因诊断写入阻塞或 panic 而留下原生工作。
            let _ = domain.terminate();
            // Windows 终止是异步的；此处不返回到分析线程或发送成功结果。
            std::process::exit(1);
        })?;
    Ok(())
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
