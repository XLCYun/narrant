//! 单次固定侧分析与独立控制输入；复用已审查的退出、终结和 stdout 背压行为。
use crate::{PROTOCOL, child_process};
use anyhow::{Result, ensure};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use std::{
    io::{BufRead, Write},
    sync::{Arc, Mutex},
};

/// 两个真实 helper 只传递请求身份、结果限额与语言投影函数，不增加过程所有权。
pub fn run<R: DeserializeOwned>(
    ready: Value,
    request_id: impl Fn(&R) -> Result<&str>,
    result_limit: impl Fn(&R) -> usize,
    analyze: impl Fn(&R) -> Result<Value>,
) -> Result<()> {
    let domain = Arc::new(child_process::CurrentProcessDomain::establish()?);
    let initial = (|| -> Result<Option<(R, String)>> {
        send(&ready)?;
        let mut line = String::new();
        if std::io::stdin().lock().read_line(&mut line)? == 0 {
            return Ok(None);
        }
        // 未收到合法 analyze 时不伪造请求身份或错误终结，直接失败退出。
        let request = serde_json::from_str::<R>(&line)?;
        let id = request_id(&request)?.to_owned();
        Ok(Some((request, id)))
    })();
    let (request, id) = match initial {
        Ok(Some(request)) => request,
        ended => {
            // 此时还没有原生工作；先解除自身 Job 的关闭即终止，保留初始错误退出码。
            domain.completed()?;
            return ended.map(|_| ());
        }
    };
    let terminal = Arc::new(Mutex::new(false));
    control(id.clone(), domain.clone(), terminal.clone())?;
    let result = analyze(&request);
    let completed = match result {
        Ok(mut result) => {
            let object = result.as_object_mut().expect("投影必须返回对象");
            object.extend([
                ("protocol".into(), json!(PROTOCOL)),
                ("type".into(), json!("completed")),
                ("id".into(), json!(id)),
                ("outcome".into(), json!("ok")),
            ]);
            if serde_json::to_vec(&result)?.len() > result_limit(&request) {
                failed(&id, "分析结果超过协议限额")
            } else {
                result
            }
        }
        Err(error) => {
            eprintln!("{error:#}");
            failed(&id, &format!("{error:#}"))
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
        .name("native-helper-control".into())
        .spawn(move || {
            let mut line = String::new();
            let _reason = match std::io::stdin().lock().read_line(&mut line) {
                Ok(0) => "Host stdin 在终结前关闭".to_owned(),
                Ok(_) => match validate_cancel(&line, &id) {
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
/// 运行中仅接受严格的同请求 cancel；未知字段、重复字段和错误 ID 均为协议错误。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cancel {
    protocol: String,
    #[serde(rename = "type")]
    message_type: String,
    id: String,
}

/// 控制读取独立于原生加载，不把错误 ID 或任意 JSON 当成有效取消。
pub fn validate_cancel(line: &str, id: &str) -> Result<()> {
    let cancel: Cancel = serde_json::from_str(line)?;
    ensure!(
        cancel.protocol == PROTOCOL && cancel.message_type == "cancel" && cancel.id == id,
        "无效取消请求"
    );
    Ok(())
}
