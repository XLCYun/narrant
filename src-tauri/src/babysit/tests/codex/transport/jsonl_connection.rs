use super::*;
use crate::babysit::codex::{AppServerCommand, RpcFailure};
use crate::operation::TestCancellation;
use std::ffi::OsString;
use std::io::Cursor;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// 构造只输出固定 JSONL 的 shell child，避免测试依赖真实 Codex 安装。
fn command(script: &str) -> AppServerCommand {
    AppServerCommand {
        program: PathBuf::from("sh"),
        args: vec![OsString::from("-c"), OsString::from(script)],
    }
}

#[tokio::test]
async fn reads_one_jsonl_message_and_trims_the_line_ending() {
    let mut connection = JsonlConnection::spawn(&command("printf '%s\\n' '{\"ok\":true}'"))
        .await
        .expect("shell child should start");

    let message = connection
        .read_message(Instant::now() + Duration::from_secs(1), None)
        .await
        .expect("JSONL should parse");

    assert_eq!(message["ok"], true);
    connection.shutdown().await;
}

#[tokio::test]
async fn observes_cancellation_while_stdout_is_blocked() {
    let mut connection = JsonlConnection::spawn(&command("sleep 2"))
        .await
        .expect("shell child should start");
    let cancellation = TestCancellation::default();
    let worker_cancellation = cancellation.clone();
    let reader = tokio::spawn(async move {
        let result = connection
            .read_message(
                Instant::now() + Duration::from_secs(1),
                Some(&worker_cancellation),
            )
            .await;
        (connection, result)
    });

    tokio::time::sleep(Duration::from_millis(20)).await;
    cancellation.cancel();
    let (mut connection, result) = reader.await.expect("reader task should finish");

    assert_eq!(result, Err(RpcFailure::Cancelled));
    connection.shutdown().await;
}

#[tokio::test]
async fn rejects_a_line_that_exceeds_the_framing_bound() {
    let mut bytes = vec![b' '; MAX_JSONL_LINE_BYTES + 1];
    bytes.push(b'\n');
    let mut reader = tokio::io::BufReader::new(Cursor::new(bytes));

    let result = read_json_line(&mut reader, Instant::now() + Duration::from_secs(1), None).await;

    assert_eq!(result, Err(RpcFailure::InvalidJson));
}
