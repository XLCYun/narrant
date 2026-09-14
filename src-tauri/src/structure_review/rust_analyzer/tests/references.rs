#![cfg(unix)]

use std::{fs, path::PathBuf};

use serde_json::json;

use crate::lsp::{file_uri, test_support::FakeServer, Position};
use crate::structure_review::{
    code_graph::{AnalysisInput, CodeCategory, CodeReference, LanguageAnalyzer, ReferenceTarget},
    rust_analyzer::RustLanguageAnalyzer,
};

/// 从真实 stdio 请求验证省略发生在语义查询前；声明与源码内容仍完整保留。
#[test]
fn skips_definition_queries_for_non_field_types_and_read_only_fields() {
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
        vec![json!({"jsonrpc":"2.0", "id":5, "result":[]})],
        vec![json!({"jsonrpc":"2.0", "id":6, "result":null})],
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
    assert_eq!(definitions.len(), 2, "{requests:#?}");
    for (request, token) in definitions.iter().zip(["u32", "make"]) {
        assert_eq!(
            request["params"]["position"],
            json!(Position {
                line: 0,
                character: source.find(token).unwrap() as u32
            })
        );
    }
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

/// 图外类型在加载源码前被丢弃；同名参数、值和未解析结果都不能变成类型连线。
#[test]
fn field_types_only_resolve_to_selected_type_declarations() {
    let source = "struct Target; struct Shadow<T> { generic: T, local: Option<(Target, Target)>, outside: Foreign, unknown: Missing, recursive: Box<Self>, not_type: VALUE } const VALUE: u8 = 1;";
    let server = FakeServer::with_workspace(|root| {
        let local = file_uri(&root.join("lib.rs")).unwrap();
        let outside = file_uri(&root.join("outside.rs")).unwrap();
        let location = |uri: &str, offset: usize| {
            json!([{"uri":uri,"range":{
                "start":{"line":0,"character":offset},"end":{"line":0,"character":offset + 1}
            }}])
        };
        let mut steps = vec![
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
        ];
        for (index, result) in [
            location(&local, source.find("<T>").unwrap() + 1),
            location(&outside, 0),
            location(&local, source.find("Target").unwrap()),
            location(&local, source.find("Target").unwrap()),
            location(&outside, 0),
            json!([]),
            location(&outside, 0),
            location(&local, source.rfind("VALUE").unwrap()),
        ]
        .into_iter()
        .enumerate()
        {
            steps.push(vec![
                json!({"jsonrpc":"2.0", "id":index + 3, "result":result}),
            ]);
        }
        steps.push(vec![json!({"jsonrpc":"2.0", "id":11, "result":null})]);
        steps
    });
    fs::write(server.workspace().join("lib.rs"), source).unwrap();
    // 若类型定义仍经过图外源码加载，这个文件会令分析失败。
    fs::write(server.workspace().join("outside.rs"), "struct {").unwrap();
    let graph = RustLanguageAnalyzer::new(server.workspace().join("server"))
        .analyze(&AnalysisInput {
            options: crate::structure_review::code_graph::AnalysisOptions {
                include_tests: true,
            },
            repository_root: server.workspace().to_path_buf(),
            files: vec!["lib.rs".into()],
        })
        .unwrap();
    let find = |name: &str| {
        graph
            .root()
            .contains
            .iter()
            .filter_map(|id| graph.node(id))
            .find(|node| node.data().name.as_deref() == Some(name))
            .unwrap()
    };
    let target = find("Target");
    let shadow = find("Shadow");
    for id in &shadow.data().contains {
        let field = graph.node(id).unwrap().data();
        let expected = match field.name.as_deref().unwrap() {
            "local" => vec![CodeReference {
                semantic: "code.type_use".into(),
                target: ReferenceTarget::Node(target.data().id.clone()),
            }],
            "recursive" => vec![CodeReference {
                semantic: "code.type_use".into(),
                target: ReferenceTarget::Node(shadow.data().id.clone()),
            }],
            _ => vec![],
        };
        assert_eq!(field.references, expected, "{field:?}");
    }
    assert!(graph.external_symbols().is_empty());
    assert_eq!(target.category(), CodeCategory::Type);
    let requests = server.requests();
    let positions: Vec<_> = requests
        .iter()
        .filter(|request| request["method"] == "textDocument/definition")
        .map(|request| request["params"]["position"]["character"].as_u64().unwrap() as usize)
        .collect();
    assert_eq!(
        positions,
        [
            "generic: T",
            "Option",
            "Target,",
            "Target)",
            "Foreign",
            "Missing",
            "Box",
            "not_type: VALUE"
        ]
        .map(|text| source.find(text).unwrap() + text.find(": ").map_or(0, |offset| offset + 2))
    );
}
