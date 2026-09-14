#![cfg(unix)]

use std::{fs, os::unix::fs::symlink, path::Path};

use serde_json::{json, Value};

use crate::{
    lsp::{file_uri, test_support::FakeServer},
    structure_review::{
        code_graph::{
            AnalysisInput, AnalysisOptions, CodeGraph, LanguageAnalyzer, ReferenceTarget,
            SymbolScope,
        },
        rust_analyzer::{RustDependencyScope, RustLanguageAnalyzer},
    },
};

/// 经公开入口接收真实 stdio 定义响应，验证图外文件的读取边界。
fn analyze_target(target: impl FnOnce(&Path) -> std::path::PathBuf) -> CodeGraph {
    let reply = |id, result| json!({"jsonrpc":"2.0","id":id,"result":result});
    let server = FakeServer::with_workspace(|root| {
        fs::write(root.join("lib.rs"), "fn run() { helper(); }").unwrap();
        let target = target(&root.canonicalize().unwrap());
        vec![
            vec![reply(
                1,
                json!({"capabilities":{
                    "definitionProvider":true,"callHierarchyProvider":true,
                    "experimental":{"parentModule":true}
                }}),
            )],
            vec![],
            vec![json!({"jsonrpc":"2.0","method":"experimental/serverStatus",
                "params":{"quiescent":true,"health":"ok"}})],
            vec![reply(2, json!([]))],
            vec![reply(
                3,
                json!([{"uri":file_uri(&target).unwrap(),"range":{
                    "start":{"line":0,"character":7},"end":{"line":0,"character":13}
                }}]),
            )],
            vec![reply(4, json!([]))],
            vec![reply(5, Value::Null)],
        ]
    });
    RustLanguageAnalyzer::new(server.workspace().join("server"))
        .analyze(&AnalysisInput {
            repository_root: server.workspace().to_owned(),
            files: vec!["lib.rs".into()],
            options: AnalysisOptions {
                include_tests: true,
            },
        })
        .unwrap()
}

/// 仓库内未选文件仍可提供真实目标身份，但不会扩大输出节点范围。
#[test]
fn resolves_unselected_repository_sources() {
    let graph = analyze_target(|root| {
        let path = root.join("helper.rs");
        fs::write(&path, "pub fn helper() {}").unwrap();
        path
    });
    assert_eq!(graph.root().contains.len(), 1);
    let symbol = graph.external_symbols().values().next().unwrap();
    assert_eq!(symbol.name, "helper");
    assert_eq!(symbol.scope, SymbolScope::Repository);
}

/// 非 UTF-8 的仓库外文件及指向它的符号链接均不可读取，更不可生成已解析目标。
#[test]
fn never_reads_outside_sources_or_escaping_symlinks() {
    let outside = tempfile::tempdir().unwrap();
    let source = outside
        .path()
        .join("registry/src/index/library-1.0/src/lib.rs");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, [0xff]).unwrap();
    for through_symlink in [false, true] {
        let graph = analyze_target(|root| {
            if through_symlink {
                let link = root.join("helper.rs");
                symlink(&source, &link).unwrap();
                link
            } else {
                source.clone()
            }
        });
        assert!(graph.external_symbols().is_empty());
        let node = graph.node(&graph.root().contains[0]).unwrap();
        assert!(node.data().references.iter().any(|reference| {
            reference.semantic == "code.calls"
                && reference.target
                    == ReferenceTarget::Unresolved {
                        name: "helper".into(),
                    }
        }));
    }
}

/// 两档均跳过不可获取的项目依赖；标准库档还需恢复 Option 和 From 关联。
#[test]
#[ignore = "需要通过 NARRANT_RUST_ANALYZER 提供服务器，单独串行运行"]
fn real_server_analyzes_repository_without_fetching_or_building_dependencies() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(
        root.join("Cargo.toml"),
        r#"
[package]
name = "repository_scope_fixture"
version = "0.1.0"
edition = "2021"
[dependencies]
serde = "=999.0.0"
"#,
    )
    .unwrap();
    fs::write(
        root.join("build.rs"),
        r#"fn main() {
        std::fs::write("build-script-ran", "unexpected").unwrap();
        panic!("repository analysis must not run build.rs");
    }"#,
    )
    .unwrap();
    fs::write(
        root.join("src/lib.rs"),
        r#"
mod local;
pub struct Item;
pub struct Group { pub items: Vec<Item> }
pub fn run() -> u32 { local::helper() }
impl Item { pub fn identity(&self) -> u32 { 42 } }
impl From<u32> for Item { fn from(_: u32) -> Self { Self } }
pub fn option_item(value: Option<Item>) -> u32 {
    match value { Some(item) => item.identity(), None => 0 }
}
pub fn constructor() -> fn(u32) -> Item { Item::from }
"#,
    )
    .unwrap();
    fs::write(root.join("src/local.rs"), "pub fn helper() -> u32 { 42 }").unwrap();
    let executable = std::env::var_os("NARRANT_RUST_ANALYZER").unwrap();
    for scope in [
        RustDependencyScope::RepositoryOnly,
        RustDependencyScope::StandardLibrary,
    ] {
        let graph = RustLanguageAnalyzer::new(executable.clone())
            .with_dependency_scope(scope)
            .analyze(&AnalysisInput {
                repository_root: root.to_owned(),
                files: vec!["src/lib.rs".into(), "src/local.rs".into()],
                options: Default::default(),
            })
            .unwrap();
        let mut all = Vec::new();
        let mut pending = graph.root().contains.clone();
        while let Some(id) = pending.pop() {
            let node = graph.node(&id).unwrap();
            pending.extend(node.data().contains.clone());
            all.push(node);
        }
        let find = |name| {
            *all.iter()
                .find(|node| node.data().name.as_deref() == Some(name))
                .unwrap()
        };
        for (source, semantic, target) in [
            ("run", "code.calls", "helper"),
            ("items", "code.type_use", "Item"),
        ] {
            assert!(
                find(source).data().references.iter().any(|reference| {
                    reference.semantic == semantic
                        && reference.target == ReferenceTarget::Node(find(target).data().id.clone())
                }),
                "{source} -> {target}"
            );
        }
        for (source, semantic, target) in [
            ("option_item", "code.calls", "identity"),
            ("constructor", "code.references", "from"),
        ] {
            let resolved = find(source).data().references.iter().any(|reference| {
                reference.semantic == semantic
                    && reference.target == ReferenceTarget::Node(find(target).data().id.clone())
            });
            assert_eq!(
                resolved,
                scope == RustDependencyScope::StandardLibrary,
                "{scope:?}: {source} -> {target}"
            );
        }
        assert!(graph
            .external_symbols()
            .values()
            .all(|symbol| symbol.scope == SymbolScope::Repository));
        assert!(!root.join("build-script-ran").exists());
        assert!(!root.join("Cargo.lock").exists());
    }
}
