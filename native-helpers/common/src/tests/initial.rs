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
        drop(input);
        let mut terminal = String::new();
        output.read_to_string(&mut terminal).unwrap();
        assert!(!helper.wait().unwrap().success(), "{invalid}");
        assert!(terminal.is_empty(), "不合法初始请求产生终结：{terminal}");
    }
}
