#![cfg(unix)]

use std::{fs, path::PathBuf};

use serde_json::json;

use crate::lsp::{test_support::FakeServer, Position};
use crate::structure_review::{
    code_graph::{AnalysisInput, LanguageAnalyzer},
    rust_analyzer::RustLanguageAnalyzer,
};

/// 从真实 stdio 请求验证省略发生在语义查询前；声明与源码内容仍完整保留。
#[test]
fn skips_definition_queries_for_type_mentions_and_read_only_fields() {
    let server = FakeServer::new(vec![
        vec![json!({"jsonrpc":"2.0", "id":1, "result":{"capabilities":{
            "definitionProvider":true, "callHierarchyProvider":true,
            "experimental":{"parentModule":true}
        }}})],
        vec![],
        vec![
            json!({"jsonrpc":"2.0", "method":"experimental/serverStatus", "params":{
                "quiescent":true, "health":"ok"
            }}),
        ],
        vec![json!({"jsonrpc":"2.0", "id":2, "result":[]})],
        vec![json!({"jsonrpc":"2.0", "id":3, "result":[]})],
        vec![json!({"jsonrpc":"2.0", "id":4, "result":[]})],
        vec![json!({"jsonrpc":"2.0", "id":5, "result":null})],
    ]);
    let source = "struct Input; struct Output { field: u32 } fn run(_: Input) -> Output { make().field; Output { field: 1 } }";
    fs::write(server.workspace().join("lib.rs"), source).unwrap();
    let graph = RustLanguageAnalyzer::new(server.workspace().join("server"))
        .analyze(&AnalysisInput {
            options: crate::structure_review::code_graph::AnalysisOptions {
                include_tests: true,
            },
            repository_root: server.workspace().to_path_buf(),
            files: vec![PathBuf::from("lib.rs")],
        })
        .unwrap();
    let requests = server.requests();
    let definitions: Vec<_> = requests
        .iter()
        .filter(|request| request["method"] == "textDocument/definition")
        .collect();
    assert_eq!(definitions.len(), 1, "{requests:#?}");
    assert_eq!(
        definitions[0]["params"]["position"],
        json!(Position {
            line: 0,
            character: source.find("make").unwrap() as u32
        })
    );
    let run = graph
        .root()
        .contains
        .iter()
        .filter_map(|id| graph.node(id))
        .find(|node| node.data().name.as_deref() == Some("run"))
        .unwrap()
        .data();
    assert!(run.signature.as_ref().unwrap().contains("Input"));
    assert!(run.code.as_ref().unwrap().contains("field"));
    assert_eq!(run.references.len(), 1);
    assert_eq!(run.references[0].semantic, "code.calls");
}
