//! 真实 executable 生命周期；成功判据先观察自然退出，失败后的兜底清理不计为通过。
use serde_json::{Value, json};
#[cfg(unix)]
use std::{fs, os::unix::fs::PermissionsExt};
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

/// 只控制 Host 的关闭方式，分析与后代回收始终由真实 helper 实现。
#[cfg(unix)]
#[derive(Clone, Copy, Debug)]
enum Stop {
    Cancel,
    Eof,
}

/// 运行中工具链探针阻塞在真实 sleep 进程，PID 标记只用于协调而非退出证据。
#[cfg(unix)]
fn exercise(stop: Stop) {
    let root = tempfile::tempdir().unwrap();
    let tools = tempfile::tempdir().unwrap();
    fs::write(root.path().join("Cargo.toml"), "[package]\nname=\"lifecycle\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\npath=\"lib.rs\"\n").unwrap();
    fs::write(root.path().join("lib.rs"), "pub fn run() {}").unwrap();
    let marker = tools.path().join("active");
    let cargo = tools.path().join("cargo");
    fs::write(
        &cargo,
        format!(
            "#!/bin/sh\nprintf '%s %s' \"$$\" \"$PPID\" > '{}'\nexec /bin/sleep 30\n",
            marker.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let mut paths = vec![tools.path().into()];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    let mut helper = Command::new(env!("CARGO_BIN_EXE_narrant-rust"))
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env("RUSTUP_TOOLCHAIN", "stable")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = helper.stdin.take().unwrap();
    let mut output = BufReader::new(helper.stdout.take().unwrap());
    let mut ready = String::new();
    output.read_line(&mut ready).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&ready).unwrap()["type"],
        "ready"
    );
    let request = json!({"protocol":"narrant.native-analyzer/1","type":"analyze","id":"analysis-1","input":{"repository_root":root.path(),"files":["lib.rs"],"include_tests":false,"profile":"rust-repository-v1"},"environment":{"scratch_root":tools.path(),"platforms":[]},"limits":{"analysis_timeout_ms":30000,"max_result_bytes":1000000,"max_diagnostic_bytes":65536}});
    writeln!(input, "{request}").unwrap();
    input.flush().unwrap();
    let start = Instant::now();
    let pids = loop {
        if let Ok(text) = fs::read_to_string(&marker) {
            let ids = text
                .split_whitespace()
                .filter_map(|id| id.parse::<i32>().ok())
                .collect::<Vec<_>>();
            if ids.len() == 2 {
                break ids;
            }
        }
        assert!(
            start.elapsed() < Duration::from_secs(10),
            "工具链探针没有启动"
        );
        thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(
        pids[1],
        helper.id() as i32,
        "原生分析不启动脱离外层所有权的 worker"
    );
    // SAFETY: 仅查询真实 PID，独立启动也不能继承测试或 shell 的进程组。
    assert_eq!(
        unsafe { libc::getpgid(helper.id() as i32) },
        helper.id() as i32
    );
    assert_eq!(unsafe { libc::getpgid(pids[0]) }, helper.id() as i32);
    match stop {
        Stop::Cancel => {
            writeln!(
                input,
                "{}",
                json!({"protocol":"narrant.native-analyzer/1","type":"cancel","id":"analysis-1"})
            )
            .unwrap();
            input.flush().unwrap();
        }
        Stop::Eof => drop(input),
    }
    let start = Instant::now();
    let natural = loop {
        let exited = helper.try_wait().unwrap().is_some();
        // SAFETY: 信号0只查询记录的真实进程；父/后代均消失才判定生命周期结束。
        let descendants_gone = pids.iter().all(|pid| unsafe { libc::kill(*pid, 0) } != 0);
        if exited && descendants_gone {
            break true;
        }
        if start.elapsed() > Duration::from_secs(5) {
            break false;
        }
        thread::sleep(Duration::from_millis(10));
    };
    if !natural {
        // 仅在断言已判定失败后防止污染机器；这次清理永远不能满足成功判据。
        let _ = helper.kill();
        let _ = helper.wait();
        for pid in &pids {
            unsafe {
                libc::kill(*pid, libc::SIGKILL);
            }
        }
    }
    assert!(
        natural,
        "{stop:?} 未使 helper/实际后代在期限内自行结束：{pids:?}"
    );
    let status = helper.wait().unwrap();
    let mut remaining = String::new();
    output.read_to_string(&mut remaining).unwrap();
    let messages = remaining
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert!(
        messages.iter().all(|message| message["outcome"] != "ok"),
        "停止后不能发送成功图"
    );
    assert!(!status.success(), "不可协作后端应主动终止自身域");
    assert!(messages.is_empty(), "停止后不发送候选图");
}

/// cancel 由仍存活的 helper 主动处理并回收后代，不由测试/Host 强杀兜底。
#[cfg(unix)]
#[test]
fn running_cancel_reaps_helper_and_descendant() {
    exercise(Stop::Cancel);
}
/// 运行中 stdin EOF 必须停止原生加载、退出 helper 并确认后代消失。
#[cfg(unix)]
#[test]
fn running_stdin_eof_reaps_helper_and_descendant() {
    exercise(Stop::Eof);
}

/// 正常终结不等待 Host 关闭 stdin；Windows 自身 Job 的关闭必须保留 exit 0。
#[test]
fn completed_exits_without_waiting_for_stdin_eof() {
    let root = tempfile::tempdir().unwrap();
    let mut helper = Command::new(env!("CARGO_BIN_EXE_narrant-rust"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = helper.stdin.take().unwrap();
    let mut output = BufReader::new(helper.stdout.take().unwrap());
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap()["type"],
        "ready"
    );
    let request = json!({"protocol":"narrant.native-analyzer/1","type":"analyze","id":"analysis-1","input":{"repository_root":root.path(),"files":[],"include_tests":false,"profile":"rust-repository-v1"},"environment":{"scratch_root":root.path(),"platforms":[]},"limits":{"analysis_timeout_ms":30000,"max_result_bytes":1000000,"max_diagnostic_bytes":65536}});
    writeln!(input, "{request}").unwrap();
    input.flush().unwrap();
    line.clear();
    output.read_line(&mut line).unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&line).unwrap()["outcome"],
        "ok"
    );
    let start = Instant::now();
    let status = loop {
        if let Some(status) = helper.try_wait().unwrap() {
            break status;
        }
        if start.elapsed() > Duration::from_secs(5) {
            let _ = helper.kill();
            let _ = helper.wait();
            panic!("completed 后仍等待 stdin");
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert!(status.success());
    let mut extra = String::new();
    output.read_to_string(&mut extra).unwrap();
    assert!(extra.is_empty());
    drop(input);
}

/// stdout 背压阻塞 completed 时，提前 stdin EOF 仍须有界停止，不能卡在终结锁上。
#[test]
fn stdin_eof_interrupts_blocked_completed_output() {
    let root = tempfile::tempdir().unwrap();
    let mut helper = Command::new(env!("CARGO_BIN_EXE_narrant-rust"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut input = helper.stdin.take().unwrap();
    let mut output = BufReader::new(helper.stdout.take().unwrap());
    let mut ready = String::new();
    output.read_line(&mut ready).unwrap();
    let request = json!({"protocol":"narrant.native-analyzer/1","type":"analyze","id":"x".repeat(1024 * 1024),"input":{"repository_root":root.path(),"files":[],"include_tests":false,"profile":"rust-repository-v1"},"environment":{"scratch_root":root.path(),"platforms":[]},"limits":{"analysis_timeout_ms":30000,"max_result_bytes":2000000,"max_diagnostic_bytes":65536}});
    writeln!(input, "{request}").unwrap();
    input.flush().unwrap();
    // 只读一个字节确认进入输出，随后保持 stdout 打开且不排空。
    let mut first = [0u8; 1];
    output.read_exact(&mut first).unwrap();
    drop(input);
    let start = Instant::now();
    loop {
        if let Some(status) = helper.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        if start.elapsed() > Duration::from_secs(5) {
            let _ = helper.kill();
            let _ = helper.wait();
            panic!("stdout 背压阻塞 EOF 清理");
        }
        thread::sleep(Duration::from_millis(10));
    }
}
