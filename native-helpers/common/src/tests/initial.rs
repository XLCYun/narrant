//! 两个真实 helper 共用初始协议反例，拒绝 schema/envelope 时不能伪造 completed ID。
use serde_json::json;
use std::{
    io::{BufRead, BufReader, Read, Write},
    process::{Command, Stdio},
};

/// 编译到实际语言 helper 的 integration target，测试不启动替身解析器。
#[test]
fn malformed_initial_input_has_no_fabricated_terminal() {
    let executable = option_env!("CARGO_BIN_EXE_narrant-python")
        .or(option_env!("CARGO_BIN_EXE_narrant-rust"))
        .unwrap();
    let language = if option_env!("CARGO_BIN_EXE_narrant-python").is_some() {
        "python-static-v1"
    } else {
        "rust-repository-v1"
    };
    let root = tempfile::tempdir().unwrap();
    let request = json!({"protocol":"narrant.native-analyzer/1","type":"analyze","id":"analysis-1","input":{"repository_root":root.path(),"files":[],"include_tests":false,"profile":language},"environment":{"scratch_root":root.path(),"platforms":[]},"limits":{"analysis_timeout_ms":1000,"max_result_bytes":1000000,"max_diagnostic_bytes":1000}});
    let valid = serde_json::to_string(&request).unwrap();
    for invalid in [
        "not JSON".to_owned(),
        valid.replace("\"type\":\"analyze\"", "\"type\":\"cancel\""),
        valid.replace("/1", "/2"),
        valid.replace("analysis-1", ""),
        valid.replace("\"id\":", "\"unknown\":true,\"id\":"),
        valid.replace("\"id\":", "\"id\":\"duplicate\",\"id\":"),
    ] {
        // 非法输入应独立于随后 EOF 失败退出，不能等待 Host 关闭管道。
        for close_input in [true, false] {
            let mut helper = Command::new(executable)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let mut output = BufReader::new(helper.stdout.take().unwrap());
            let mut ready = String::new();
            output.read_line(&mut ready).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&ready).unwrap()["type"],
                "ready"
            );
            let mut input = helper.stdin.take().unwrap();
            writeln!(input, "{invalid}").unwrap();
            if close_input {
                drop(input);
            }
            let mut terminal = String::new();
            output.read_to_string(&mut terminal).unwrap();
            assert!(
                !helper.wait().unwrap().success(),
                "close_input={close_input}: {invalid}"
            );
            assert!(terminal.is_empty(), "不合法初始请求产生终结：{terminal}");
        }
    }
}

/// 未提交请求即关闭输入仍正常退出，且只输出真实 ready。
#[test]
fn eof_before_initial_request_has_only_ready() {
    let executable = option_env!("CARGO_BIN_EXE_narrant-python")
        .or(option_env!("CARGO_BIN_EXE_narrant-rust"))
        .unwrap();
    let mut helper = Command::new(executable)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    drop(helper.stdin.take());
    let mut output = BufReader::new(helper.stdout.take().unwrap());
    let mut ready = String::new();
    output.read_line(&mut ready).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&ready).unwrap()["type"],
        "ready"
    );
    let mut terminal = String::new();
    output.read_to_string(&mut terminal).unwrap();
    assert!(helper.wait().unwrap().success());
    assert!(terminal.is_empty(), "未提交请求产生终结：{terminal}");
}
