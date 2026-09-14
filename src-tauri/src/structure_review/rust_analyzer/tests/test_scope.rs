#![cfg(unix)]

use std::{fs, path::Path};

use serde_json::{json, Value};

use crate::lsp::{file_uri, test_support::FakeServer};
use crate::structure_review::{
    code_graph::{AnalysisInput, AnalysisOptions, CodeGraph, LanguageAnalyzer},
    rust_analyzer::RustLanguageAnalyzer,
};

/// 真实 stdio 握手后按请求返回语义，不以内部函数调用代替协议验证。
fn steps(results: Vec<Value>) -> Vec<Vec<Value>> {
    let mut steps = vec![
        vec![json!({"jsonrpc":"2.0", "id":1, "result":{"capabilities":{
            "definitionProvider":true, "callHierarchyProvider":true,
            "experimental":{"parentModule":true}
        }}})],
        vec![],
        vec![
            json!({"jsonrpc":"2.0", "method":"experimental/serverStatus", "params":{"quiescent":true,"health":"ok"}}),
        ],
    ];
    steps.extend(
        results
            .into_iter()
            .enumerate()
            .map(|(index, result)| vec![json!({"jsonrpc":"2.0","id":index+2,"result":result})]),
    );
    steps
}

/// 位置精确指向父文件中的模块声明，路径和名称本身不是测试判据。
fn location(root: &Path, file: &str, character: usize) -> Value {
    json!({"uri":file_uri(&root.canonicalize().unwrap().join(file)).unwrap(),"range":{
        "start":{"line":0,"character":character},"end":{"line":0,"character":character+1}
    }})
}

/// 选中文件走公开语言入口，其他源码只充当 LSP 返回的上下文。
fn analyze(server: &FakeServer, file: &str, include_tests: bool) -> CodeGraph {
    RustLanguageAnalyzer::new(server.workspace().join("server"))
        .analyze(&AnalysisInput {
            repository_root: server.workspace().to_owned(),
            files: vec![file.into()],
            options: AnalysisOptions { include_tests },
        })
        .unwrap()
}

/// 排除发生在定义查询和调用层级查询之前，首个测试函数不会遮掉后面的业务函数。
#[test]
fn skips_test_reference_queries_and_can_restore_them() {
    let source = "#[test] fn checks(){ test_only(); } fn business(){ normal(); }";
    for include_tests in [false, true] {
        let count = if include_tests { 5 } else { 3 };
        let mut results = vec![json!([]); count];
        results.push(Value::Null);
        let server = FakeServer::new(steps(results));
        fs::write(server.workspace().join("lib.rs"), source).unwrap();
        let graph = analyze(&server, "lib.rs", include_tests);
        assert_eq!(
            graph.root().contains.len(),
            if include_tests { 2 } else { 1 }
        );
        let requests = server.requests();
        let definitions: Vec<_> = requests
            .iter()
            .filter(|request| request["method"] == "textDocument/definition")
            .collect();
        assert_eq!(definitions.len(), if include_tests { 2 } else { 1 });
        if !include_tests {
            assert_eq!(
                definitions[0]["params"]["position"]["character"],
                source.find("normal").unwrap()
            );
        }
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "textDocument/prepareCallHierarchy")
                .count(),
            if include_tests { 2 } else { 1 }
        );
    }
}

/// cfg(test) 父模块即使不在选中文件中，也会排除整个文件中的无属性辅助声明。
#[test]
fn excludes_helpers_through_unselected_parent_modules() {
    for nested in [false, true] {
        let root_source = "#[cfg(test)] #[path=\"support.rs\"] mod support;";
        let middle_source = "mod checks;";
        let server = FakeServer::with_workspace(|root| {
            let mut results = if nested {
                vec![
                    json!([location(root, "middle.rs", 4)]),
                    json!([location(
                        root,
                        "lib.rs",
                        root_source.find("mod support").unwrap() + 4
                    )]),
                ]
            } else {
                vec![json!([location(
                    root,
                    "lib.rs",
                    root_source.find("mod support").unwrap() + 4
                )])]
            };
            results.push(Value::Null);
            steps(results)
        });
        fs::write(server.workspace().join("lib.rs"), root_source).unwrap();
        fs::write(server.workspace().join("middle.rs"), middle_source).unwrap();
        fs::write(
            server.workspace().join("checks.rs"),
            "pub struct Fixture; pub fn setup(){ only_tests(); }",
        )
        .unwrap();
        assert!(analyze(&server, "checks.rs", false)
            .root()
            .contains
            .is_empty());
        let requests = server.requests();
        assert!(!requests.iter().any(|request| matches!(
            request["method"].as_str(),
            Some("textDocument/definition" | "textDocument/prepareCallHierarchy")
        )));
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "experimental/parentModule")
                .count(),
            if nested { 2 } else { 1 }
        );
    }
}

/// 同一源码同时归属生产和测试模块时保留业务视角；文件名 tests.rs 不影响判断。
#[test]
fn preserves_files_shared_with_a_production_module_and_caches_parents() {
    let parent =
        "#[cfg(test)] #[path=\"tests.rs\"] mod tests; #[path=\"tests.rs\"] mod production;";
    let server = FakeServer::with_workspace(|root| {
        steps(vec![
            json!([
                location(root, "lib.rs", parent.find("mod tests").unwrap() + 4),
                location(root, "lib.rs", parent.find("mod production").unwrap() + 4)
            ]),
            json!([]),
            json!([]),
            json!([]),
            Value::Null,
        ])
    });
    fs::write(server.workspace().join("lib.rs"), parent).unwrap();
    fs::write(
        server.workspace().join("tests.rs"),
        "pub fn shared(){ live(); }",
    )
    .unwrap();
    let graph = analyze(&server, "tests.rs", false);
    assert_eq!(graph.root().contains.len(), 1);
    let node = graph.node(&graph.root().contains[0]).unwrap();
    assert_eq!(node.data().name.as_deref(), Some("shared"));
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|request| request["method"] == "experimental/parentModule")
            .count(),
        2
    );
}

/// 文件顶层的测试条件不需要继续追溯模块，也不能留下空的 crate 图节点。
#[test]
fn excludes_file_inner_cfg_without_requesting_its_relationships() {
    let server = FakeServer::new(steps(vec![Value::Null]));
    fs::write(
        server.workspace().join("lib.rs"),
        "#![cfg(test)] fn helper(){ only_tests(); }",
    )
    .unwrap();
    assert!(analyze(&server, "lib.rs", false).root().contains.is_empty());
    assert_eq!(
        server
            .requests()
            .iter()
            .filter(|request| request["method"] == "experimental/parentModule")
            .count(),
        0
    );
}

/// 调用层级返回测试子范围时不读取目标文件，也不将关系转移给外层业务函数。
#[test]
fn ignores_call_hierarchy_ranges_inside_excluded_tests() {
    let source = "fn live(){ #[test] fn checks(){ hidden(); } }";
    let server = FakeServer::with_workspace(|root| {
        let range = |offset| json!({"start":{"line":0,"character":offset},"end":{"line":0,"character":offset+1}});
        steps(vec![
            json!([]),
            json!([{"name":"live"}]),
            json!([{
                "to":{"name":"hidden", "uri":file_uri(&root.canonicalize().unwrap().join("missing.rs")).unwrap(),"selectionRange":range(0)},
                "fromRanges":[range(source.find("hidden").unwrap())]
            }]),
            Value::Null,
        ])
    });
    fs::write(server.workspace().join("lib.rs"), source).unwrap();
    let graph = analyze(&server, "lib.rs", false);
    assert_eq!(graph.root().contains.len(), 1);
    assert!(graph
        .node(&graph.root().contains[0])
        .unwrap()
        .data()
        .references
        .is_empty());
    assert!(!server
        .requests()
        .iter()
        .any(|request| request["method"] == "textDocument/definition"));
}

/// 通配导入不触发属性身份查询，关闭开关时也不查询测试体的关系。
#[test]
fn skips_test_attributes_and_bodies_without_semantic_confirmation() {
    let source = "use scope::*; #[test] fn checks(){ only_test(); } fn live() {}";
    for include_tests in [false, true] {
        let mut results = vec![json!([]), json!([])]; // 父模块和 use 的定义查询。
        if include_tests {
            results.extend([json!([]), json!([])]); // 测试体调用及其调用层级。
        }
        results.extend([json!([]), Value::Null]); // 业务函数调用层级和 shutdown。
        let server = FakeServer::new(steps(results));
        fs::write(server.workspace().join("lib.rs"), source).unwrap();
        let graph = analyze(&server, "lib.rs", include_tests);
        assert_eq!(
            graph
                .root()
                .contains
                .iter()
                .any(|id| graph.node(id).unwrap().data().name.as_deref() == Some("checks")),
            include_tests
        );
        let requests = server.requests();
        let definitions: Vec<_> = requests
            .iter()
            .filter(|request| request["method"] == "textDocument/definition")
            .collect();
        assert!(!definitions
            .iter()
            .any(|request| request["params"]["position"]["character"]
                == source.find("[test]").unwrap() + 1));
        assert_eq!(
            definitions
                .iter()
                .any(|request| request["params"]["position"]["character"]
                    == source.find("only_test").unwrap()),
            include_tests
        );
        assert_eq!(
            requests
                .iter()
                .filter(|request| request["method"] == "textDocument/prepareCallHierarchy")
                .count(),
            if include_tests { 2 } else { 1 }
        );
    }
}
