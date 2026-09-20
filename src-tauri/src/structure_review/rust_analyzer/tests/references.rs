#![cfg(unix)]

use std::{collections::BTreeSet, fs, path::PathBuf};

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

/// 赋值目标在语义查询前省略，目标内的求值调用、回调实参和右侧函数值仍保留。
#[test]
fn skips_assignment_target_queries_and_preserves_evaluated_references() {
    let source = "fn callback() {} fn run() { VALUE = callback; VALUE += rhs(); (PAREN) = rhs(); state.field = rhs(); nested.child.field += rhs(); (*pointer).field = rhs(); buffer[index(callback)] = rhs(); place(callback).field += rhs(); (FIRST, [SECOND]) = values(); Record { field: THIRD } = make(); }";
    let mut expected_sites: Vec<_> = ["callback", "rhs", "index", "place", "values", "make"]
        .into_iter()
        .flat_map(|name| {
            source
                .match_indices(name)
                .filter(|(offset, _)| *offset > source.find("fn run").unwrap())
                .map(move |(offset, _)| (offset, name))
        })
        .collect();
    expected_sites.sort();
    let server = FakeServer::with_workspace(|root| {
        let uri = file_uri(&root.join("lib.rs")).unwrap();
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
        for (index, (_, name)) in expected_sites.iter().enumerate() {
            let result = if *name == "callback" {
                let offset = source.find("callback").unwrap();
                json!([{"uri":uri,"range":{
                    "start":{"line":0,"character":offset},
                    "end":{"line":0,"character":offset + name.len()}
                }}])
            } else {
                json!([])
            };
            steps.push(vec![
                json!({"jsonrpc":"2.0", "id":index + 3, "result":result}),
            ]);
        }
        // 两个函数分别查询调用层级，随后按正常协议关闭会话。
        for index in 0..2 {
            steps.push(vec![json!({
                "jsonrpc":"2.0", "id":expected_sites.len() + index + 3, "result":[]
            })]);
        }
        steps.push(vec![json!({
            "jsonrpc":"2.0", "id":expected_sites.len() + 5, "result":null
        })]);
        steps
    });
    fs::write(server.workspace().join("lib.rs"), source).unwrap();
    let graph = RustLanguageAnalyzer::new(server.workspace().join("server"))
        .analyze(&AnalysisInput {
            options: crate::structure_review::code_graph::AnalysisOptions {
                include_tests: true,
            },
            repository_root: server.workspace().to_path_buf(),
            files: vec!["lib.rs".into()],
        })
        .unwrap();
    let requests = server.requests();
    let positions: Vec<_> = requests
        .iter()
        .filter(|request| request["method"] == "textDocument/definition")
        .map(|request| request["params"]["position"]["character"].as_u64().unwrap() as usize)
        .collect();
    assert_eq!(
        positions,
        expected_sites
            .iter()
            .map(|(offset, _)| *offset)
            .collect::<Vec<_>>(),
        "赋值目标不应产生写入查询，也不能改为普通引用查询：{requests:#?}"
    );
    let find = |name: &str| {
        graph
            .root()
            .contains
            .iter()
            .filter_map(|id| graph.node(id))
            .find(|node| node.data().name.as_deref() == Some(name))
            .unwrap()
            .data()
    };
    let run = find("run");
    assert!(run.code.as_ref().unwrap().contains("VALUE = callback"));
    let calls: BTreeSet<_> = run
        .references
        .iter()
        .filter_map(
            |reference| match (&*reference.semantic, &reference.target) {
                ("code.calls", ReferenceTarget::Unresolved { name }) => Some(name.as_str()),
                _ => None,
            },
        )
        .collect();
    assert_eq!(
        calls,
        BTreeSet::from(["rhs", "index", "place", "values", "make"])
    );
    assert!(run.references.contains(&CodeReference {
        semantic: "code.references".into(),
        target: ReferenceTarget::Node(find("callback").id.clone()),
    }));
    assert_eq!(run.references.len(), calls.len() + 1, "{run:#?}");
    assert!(run
        .references
        .iter()
        .all(|reference| { !matches!(reference.semantic.as_str(), "code.reads" | "code.writes") }));
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
