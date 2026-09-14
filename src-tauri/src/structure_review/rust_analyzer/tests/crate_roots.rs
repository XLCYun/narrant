#![cfg(unix)]

use std::fs;

use serde_json::{json, Value};

use crate::lsp::{file_uri, test_support::FakeServer};
use crate::structure_review::{
    code_graph::{AnalysisInput, CodeGraph, LanguageAnalyzer},
    rust_analyzer::RustLanguageAnalyzer,
};

/// 为两个根文件提供真实 stdio 握手、父模块和调用层级响应；不根据文件名推断根。
fn analyze_roots(files: [&str; 2], parents: [Value; 2]) -> CodeGraph {
    let reply = |id, result| json!({"jsonrpc":"2.0", "id":id, "result":result});
    let server = FakeServer::new(vec![
        vec![reply(
            1,
            json!({"capabilities":{
                "definitionProvider":true, "callHierarchyProvider":true,
                "experimental":{"parentModule":true}
            }}),
        )],
        vec![],
        vec![],
        vec![
            json!({"jsonrpc":"2.0", "method":"experimental/serverStatus", "params":{
                "quiescent":true, "health":"ok"
            }}),
        ],
        vec![reply(2, parents[0].clone())],
        vec![reply(3, json!([]))],
        vec![reply(4, parents[1].clone())],
        vec![reply(5, json!([]))],
        vec![reply(6, Value::Null)],
    ]);
    for (path, source) in files
        .into_iter()
        .zip(["pub fn run() {}", "#[test] fn checks() {}"])
    {
        let path = server.workspace().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    RustLanguageAnalyzer::new(server.workspace().join("server"))
        .analyze(&AnalysisInput {
            options: crate::structure_review::code_graph::AnalysisOptions {
                include_tests: true,
            },
            repository_root: server.workspace().to_owned(),
            files: files.into_iter().map(Into::into).collect(),
        })
        .unwrap()
}

/// crate 根由服务器指向清单的响应确认，工作目录改变不影响根及成员身份。
#[test]
fn separates_library_and_integration_test_crate_roots() {
    let parent = json!([{"uri":"file:///workspace/Cargo.toml", "range":{
        "start":{"line":0,"character":0}, "end":{"line":0,"character":0}
    }}]);
    let files = ["src/lib.rs", "tests/github_discovery.rs"];
    let graph = analyze_roots(files, [parent.clone(), parent.clone()]);
    assert_eq!(graph.root().contains.len(), 2);
    for (id, (path, member)) in graph
        .root()
        .contains
        .iter()
        .zip(files.into_iter().zip(["run", "checks"]))
    {
        let root = graph.node(id).unwrap();
        assert_eq!(root.data().native_kind, "crate");
        assert_eq!(root.data().name.as_deref(), Some(path));
        assert_eq!(root.data().contains.len(), 1);
        assert_eq!(
            graph
                .node(&root.data().contains[0])
                .unwrap()
                .data()
                .name
                .as_deref(),
            Some(member)
        );
        assert!(root.data().signature.is_none());
        assert!(root.data().code.is_none());
        let source = if member == "run" {
            "pub fn run() {}"
        } else {
            "#[test] fn checks() {}"
        };
        assert_eq!(root.data().source.as_deref(), Some(source));
        assert_eq!(
            graph
                .node(&root.data().contains[0])
                .unwrap()
                .data()
                .source
                .as_deref(),
            Some(source)
        );
    }
    let repeat = analyze_roots(files, [parent.clone(), parent]);
    assert!(graph.diff(&repeat).unwrap().nodes.is_empty());
}

/// 自定义入口同样生成根；成功空响应不构成 crate 身份证据。
#[test]
fn only_creates_crate_roots_confirmed_by_the_language_server() {
    let manifest = file_uri(std::path::Path::new("/workspace/Cargo.toml")).unwrap();
    let parent = json!([{"targetUri":manifest, "targetRange":{
        "start":{"line":0,"character":0}, "end":{"line":0,"character":0}
    }, "targetSelectionRange":{"start":{"line":0,"character":0},"end":{"line":0,"character":0}}}]);
    let graph = analyze_roots(
        ["custom/entry.rs", "tests/detached.rs"],
        [parent, json!([])],
    );
    let roots: Vec<_> = graph
        .root()
        .contains
        .iter()
        .map(|id| graph.node(id).unwrap())
        .collect();
    assert_eq!(
        roots
            .iter()
            .filter(|node| node.data().native_kind == "crate")
            .count(),
        1
    );
    assert!(roots
        .iter()
        .any(|node| node.data().name.as_deref() == Some("custom/entry.rs")));
    assert!(roots
        .iter()
        .any(|node| node.data().name.as_deref() == Some("checks")));
}
