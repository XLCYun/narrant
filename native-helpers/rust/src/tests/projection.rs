use crate::{projection, protocol::Request, test_support as support};
use serde_json::{Value, json};
use std::{fs, path::Path};

/// 构造严格公共请求，所有源码来自临时目录。
fn request(root: &Path, files: &[&str], include_tests: bool) -> Request {
    serde_json::from_value(json!({"protocol":"narrant.native-analyzer/1","type":"analyze","id":"analysis-1","input":{"repository_root":root.canonicalize().unwrap(),"files":files,"include_tests":include_tests,"profile":"rust-repository-v1"},"environment":{"scratch_root":std::env::temp_dir(),"platforms":[]},"limits":{"analysis_timeout_ms":30000,"max_result_bytes":10000000,"max_diagnostic_bytes":65536}})).unwrap()
}

/// 真实完整图的别名来源、未激活 cfg 源码和测试排除均生成明确 evidence。
#[test]
fn evidence_preserves_aliases_inactive_declarations_and_test_scope() {
    let root = support::fixture();
    let source = "#[path=\"store.rs\"] mod store; #[cfg(test)] #[path=\"specs.rs\"] mod tests; #[cfg(any())] fn inactive() { missing(); } pub fn run(){store::helper();}";
    fs::write(root.path().join("src/lib.rs"), source).unwrap();
    let before = support::snapshot(root.path());
    let result = projection::analyze(&request(
        root.path(),
        &["src/lib.rs", "src/store.rs", "src/specs.rs"],
        false,
    ))
    .unwrap();
    let nodes = result["graph"]["nodes"].as_array().unwrap();
    assert!(nodes.iter().any(|node| node["name"] == "inactive"));
    assert_eq!(
        result["evidence"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|file| file["path"] == "src/specs.rs")
            .unwrap()["disposition"],
        "excluded_test"
    );
    assert_eq!(
        nodes.len(),
        result["evidence"]["origins"].as_array().unwrap().len()
    );
    support::assert_unchanged(root.path(), &before);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.path().join("src"), root.path().join("alias")).unwrap();
        let result = projection::analyze(&request(
            root.path(),
            &["src/lib.rs", "alias/lib.rs"],
            false,
        ))
        .unwrap();
        assert!(
            result["evidence"]["origins"]
                .as_array()
                .unwrap()
                .iter()
                .all(|origin| origin["selected_paths"] == json!(["src/lib.rs", "alias/lib.rs"]))
        );
    }
}

/// 语法异常和已选二进制资源均终止整图，不能返回部分节点或成功空图。
#[test]
fn extraction_errors_abort_the_entire_graph() {
    let root = support::fixture();
    for source in [b"fn broken( {".as_slice(), &[0xff, 0xfe]] {
        fs::write(root.path().join("src/lib.rs"), source).unwrap();
        assert!(projection::analyze(&request(root.path(), &["src/lib.rs"], true)).is_err());
    }
}

/// CRLF、多字节名称、宏标点和不同 edition 保留旧语法规则，全部来自原生数据库。
#[test]
fn native_syntax_preserves_source_and_language_scope() {
    let root = support::fixture();
    let source = "mod demo {\r\n    /// 注释\r\n    fn 读取() { const N: u8 = 1; consume(N); }\r\n}\r\nfn gen() {}\r\n#[cfg(any())] fn inactive() {}";
    fs::write(root.path().join("src/lib.rs"), source).unwrap();
    let result = projection::analyze(&request(root.path(), &["src/lib.rs"], true)).unwrap();
    let node = |name: &str| -> &Value {
        result["graph"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["name"] == name)
            .unwrap()
    };
    assert_eq!(
        node("读取")["source"],
        "    /// 注释\r\n    fn 读取() { const N: u8 = 1; consume(N); }"
    );
    assert!(!node("读取")["code"].as_str().unwrap().contains("const"));
    assert_eq!(node("gen")["native_kind"], "fn");
    assert_eq!(node("inactive")["native_kind"], "fn");
}

/// 显式项目的声明根丢失时不能用已选旁路文件拼出不完整成功图。
#[test]
fn missing_json_crate_root_aborts_the_entire_graph() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("selected.rs"), "pub fn selected() {}").unwrap();
    fs::write(
        root.path().join("rust-project.json"),
        r#"{"crates":[{"root_module":"missing.rs","edition":"2021","deps":[],"cfg":[],"env":{}}]}"#,
    )
    .unwrap();
    let error = projection::analyze(&request(root.path(), &["selected.rs"], true)).unwrap_err();
    assert!(format!("{error:#}").contains("JSON crate 根源码未完整加载"));
}
