//! 真实 Python helper 的单次完成、活动分析 cancel/EOF 与输出背压；兜底清理不计为通过。
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

/// 明确提供 executable 安装根与同源标准声明，不导入用户 Python 环境。
fn request(repository: &Path, scratch: &Path, files: Vec<&str>) -> Value {
    json!({"protocol":"narrant.native-analyzer/1","type":"analyze","id":"analysis-1","input":{"repository_root":repository,"files":files,"include_tests":false,"profile":"python-static-v1"},"environment":{"scratch_root":scratch,"platforms":[{"id":"stdlib","kind":"python-embedded-stdlib","version":"0.0.84","root":Path::new(env!("CARGO_BIN_EXE_narrant-python")).parent().unwrap(),"metadata":{"source_revision":"11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a"}}]},"limits":{"analysis_timeout_ms":30000,"max_result_bytes":32000000,"max_diagnostic_bytes":65536}})
}
/// ready 先证明当前 helper 已建立专属域，之后才提交固定侧。
fn start() -> (
    Child,
    std::process::ChildStdin,
    BufReader<std::process::ChildStdout>,
) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_narrant-python"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut ready = String::new();
    output.read_line(&mut ready).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&ready).unwrap()["type"],
        "ready"
    );
    #[cfg(unix)]
    assert_eq!(
        unsafe { libc::getpgid(child.id() as i32) },
        child.id() as i32
    );
    (child, input, output)
}
/// 先观察真实自然退出；只在判定失败后清理进程，清理不能满足断言。
fn natural_exit(child: &mut Child) -> std::process::ExitStatus {
    let started = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        if started.elapsed() > Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("helper 未在期限内自行退出");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
/// 活动原生分析在 Host 仍存活时收到 cancel、EOF 或无效控制，必须停止且不发送成功图。
#[test]
fn running_analysis_stops_on_cancel_eof_and_invalid_control() {
    for stop in ["cancel", "eof", "invalid-id"] {
        let repository = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let source: String = (0..15000)
            .map(|index| format!("def value_{index}(): return len([])\n"))
            .collect();
        std::fs::write(repository.path().join("main.py"), source).unwrap();
        let (mut child, mut input, mut output) = start();
        writeln!(
            input,
            "{}",
            request(repository.path(), scratch.path(), vec!["main.py"])
        )
        .unwrap();
        input.flush().unwrap();
        let (sent, received) = mpsc::channel();
        let reader = thread::spawn(move || {
            let mut remaining = String::new();
            output.read_to_string(&mut remaining).unwrap();
            sent.send(remaining).unwrap();
        });
        assert!(
            matches!(
                received.recv_timeout(Duration::from_millis(150)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "busy fixture 已意外终结"
        );
        assert!(child.try_wait().unwrap().is_none(), "原生分析没有保持运行");
        if stop != "eof" {
            writeln!(input, "{}", json!({"protocol":"narrant.native-analyzer/1","type":"cancel","id":if stop == "cancel" { "analysis-1" } else { "wrong" }})).unwrap();
            input.flush().unwrap();
        }
        drop(input);
        assert!(!natural_exit(&mut child).success(), "{stop}");
        let remaining = received.recv_timeout(Duration::from_secs(2)).unwrap();
        reader.join().unwrap();
        assert!(remaining.is_empty(), "停止后发送候选图：{remaining}");
    }
}
/// 正常 completed 不等 stdin EOF，并且错误政策只能产生一次真实请求的 error。
#[test]
fn terminal_completion_exits_while_host_input_remains_open() {
    for valid in [true, false] {
        let repository = tempfile::tempdir().unwrap();
        let scratch = tempfile::tempdir().unwrap();
        let (mut child, mut input, mut output) = start();
        let mut value = request(repository.path(), scratch.path(), vec![]);
        if !valid {
            value["environment"]["platforms"][0]["metadata"]["source_revision"] =
                json!("0".repeat(40));
        }
        writeln!(input, "{value}").unwrap();
        input.flush().unwrap();
        let mut terminal = String::new();
        output.read_line(&mut terminal).unwrap();
        let message: Value = serde_json::from_str(&terminal).unwrap();
        assert_eq!(message["id"], "analysis-1");
        assert_eq!(message["outcome"], if valid { "ok" } else { "error" });
        assert!(natural_exit(&mut child).success());
        let mut extra = String::new();
        output.read_to_string(&mut extra).unwrap();
        assert!(extra.is_empty());
        drop(input);
    }
}
/// completed 写入受 stdout 背压阻塞时，stdin EOF 仍能中断终结锁。
#[test]
fn stdin_eof_interrupts_completed_backpressure() {
    let repository = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let (mut child, mut input, mut output) = start();
    let mut value = request(repository.path(), scratch.path(), vec![]);
    value["id"] = json!("x".repeat(1024 * 1024));
    writeln!(input, "{value}").unwrap();
    input.flush().unwrap();
    let mut first = [0];
    output.read_exact(&mut first).unwrap();
    drop(input);
    assert!(!natural_exit(&mut child).success());
}
