use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crate::structure_review::{
    code_graph::{
        AnalysisInput, ChangeKind, CodeCategory, CodeGraph, CodeNode, LanguageAnalyzer, NodeId,
        ReferenceTarget, SymbolScope,
    },
    rust_analyzer::RustLanguageAnalyzer,
};

/// 从明确提供的二进制路径运行集成验收，不在测试中下载服务器。
fn analyzer() -> RustLanguageAnalyzer {
    let executable = std::env::var_os("NARRANT_RUST_ANALYZER")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join(if cfg!(windows) {
                    ".tools/rust-analyzer.exe"
                } else {
                    ".tools/rust-analyzer"
                })
        });
    RustLanguageAnalyzer::new(executable)
}

/// 通过公共根和节点查询遍历结果，验收不依赖图内部存储。
fn nodes(graph: &CodeGraph) -> BTreeMap<NodeId, &CodeNode> {
    let mut result = BTreeMap::new();
    let mut pending = graph.root().contains.clone();
    while let Some(id) = pending.pop() {
        let node = graph.node(&id).unwrap();
        pending.extend(node.data().contains.clone());
        result.insert(id, node);
    }
    result
}

/// 构造两个真正独立的磁盘快照，代码仅来自临时目录而非用户编辑缓冲区。
fn repository(source: &str) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("src")).unwrap();
    fs::write(
        directory.path().join("Cargo.toml"),
        "[package]\nname = \"code_graph_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::write(directory.path().join("src/lib.rs"), source).unwrap();
    fs::write(
        directory.path().join("src/store.rs"),
        "pub fn helper() -> u32 { 1 }\n",
    )
    .unwrap();
    directory
}

/// 用真正的 LSP 会话确认体内变化、闭包归属、跨文件范围和 cfg/宏边界。
#[test]
#[ignore = "需要已提供的 rust-analyzer 和 Rust 工具链，单独串行运行"]
fn real_server_compares_prepared_repositories() {
    let source = r#"
#[path = "store.rs"] mod storage;
use storage::helper;
#[derive(Clone)] pub struct Counter { value: u32 }
impl Counter {
    pub fn update(&mut self) { self.value += helper(); }
    pub fn copy(&self) -> Self { self.clone() }
}
pub fn run() -> u32 { let f = || helper(); f() + 10 }
pub fn function_value() -> fn() -> u32 { helper }
pub fn direct_closure() { (|| helper())(); }
#[cfg(any())] fn inactive() { missing_target(); }
macro_rules! declare { ($name:ident) => { fn $name() {} }; }
declare!(generated);
"#;
    let before = repository(source);
    let repeated = repository(source);
    let after = repository(&source.replace("f() + 10", "f() + 20"));
    let analyze = |directory: &Path, files: &[&str]| {
        analyzer()
            .analyze(&AnalysisInput {
                repository_root: directory.to_path_buf(),
                files: files.iter().map(PathBuf::from).collect(),
            })
            .unwrap()
    };
    let old = analyze(before.path(), &["src/lib.rs", "src/store.rs"]);
    let repeat = analyze(repeated.path(), &["src/lib.rs", "src/store.rs"]);
    assert!(old.diff(&repeat).unwrap().nodes.is_empty());
    let new = analyze(after.path(), &["src/lib.rs", "src/store.rs"]);
    let diff = old.diff(&new).unwrap();
    assert_eq!(diff.nodes.len(), 1, "{diff:?}");
    assert_eq!(diff.nodes[0].content, Some(ChangeKind::Modified));
    assert_eq!(
        new.node(&diff.nodes[0].id).unwrap().data().name.as_deref(),
        Some("run")
    );

    let all = nodes(&old);
    let find = |name: &str| {
        *all.values()
            .find(|node| node.data().name.as_deref() == Some(name))
            .unwrap()
    };
    let helper = find("helper").data().id.clone();
    assert!(find("storage").data().contains.contains(&helper));
    assert!(!all
        .values()
        .any(|node| node.data().native_kind == "closure"));
    assert!(find("run")
        .data()
        .references
        .iter()
        .any(|reference| reference.semantic == "code.calls"
            && reference.target == ReferenceTarget::Node(helper.clone())));
    assert!(find("inactive").data().references.iter().any(|reference| matches!(&reference.target, ReferenceTarget::Unresolved { name } if name == "missing_target")));
    assert!(all
        .values()
        .any(|node| node.data().native_kind == "macro_invocation"));
    assert!(!all
        .values()
        .any(|node| node.data().name.as_deref() == Some("generated")));
    assert!(find("update")
        .data()
        .references
        .iter()
        .any(|reference| reference.semantic == "code.writes"));
    assert!(find("function_value")
        .data()
        .references
        .iter()
        .any(|reference| reference.semantic == "code.references"
            && reference.target == ReferenceTarget::Node(helper.clone())));
    assert!(find("direct_closure")
        .data()
        .references
        .iter()
        .any(|reference| reference.semantic == "code.calls"
            && reference.target == ReferenceTarget::Node(helper.clone())));
    assert!(find("copy")
        .data()
        .references
        .iter()
        .any(|reference| reference.semantic == "code.calls"
            && !matches!(reference.target, ReferenceTarget::Node(_))));

    let scoped = analyze(before.path(), &["src/lib.rs"]);
    let scoped_nodes = nodes(&scoped);
    assert!(scoped_nodes
        .values()
        .flat_map(|node| &node.data().references)
        .any(|reference| reference.target
            == ReferenceTarget::External {
                symbol: helper.0.clone()
            }));
    assert_eq!(
        fs::read_to_string(before.path().join("src/lib.rs")).unwrap(),
        source
    );
    save("fixture-before", &old);
    save("fixture-after", &new);

    let shared_source = source.replace(
        "mod storage;",
        "mod storage; #[path = \"store.rs\"] mod mirror;",
    );
    let shared = repository(&shared_source);
    let graph = analyze(shared.path(), &["src/lib.rs", "src/store.rs"]);
    let shared_nodes = nodes(&graph);
    let helper_node = graph.node(&helper).unwrap();
    assert!(graph.root().contains.contains(&helper));
    for name in ["storage", "mirror"] {
        let parent = shared_nodes
            .values()
            .find(|node| node.data().name.as_deref() == Some(name))
            .unwrap();
        assert!(helper_node
            .data()
            .references
            .iter()
            .any(|reference| reference.semantic == "code.member_of"
                && reference.target == ReferenceTarget::Node(parent.data().id.clone())));
    }

    // 第二个父模块在未选文件内时，也不能把共享声明任意放到唯一可见的父模块下。
    let scoped_shared = repository(&source.replace("mod storage;", "mod storage; mod other;"));
    fs::write(
        scoped_shared.path().join("src/other.rs"),
        "#[path = \"store.rs\"] mod mirror;",
    )
    .unwrap();
    let graph = analyze(scoped_shared.path(), &["src/lib.rs", "src/store.rs"]);
    assert!(graph.root().contains.contains(&helper));
    let parents: Vec<_> = graph
        .node(&helper)
        .unwrap()
        .data()
        .references
        .iter()
        .filter(|reference| reference.semantic == "code.member_of")
        .collect();
    assert_eq!(parents.len(), 2);
    assert!(parents
        .iter()
        .any(|reference| matches!(&reference.target, ReferenceTarget::Node(_))));
    assert!(parents.iter().any(|reference| matches!(
        &reference.target,
        ReferenceTarget::External { symbol } if symbol.contains("mirror")
    )));
}

/// 闭包不建模，仍经真实 LSP 保留跨文件关联，并将闭包编辑准确计入其外层对象。
#[test]
#[ignore = "需要已提供的 rust-analyzer 和 Rust 工具链，单独串行运行"]
fn real_server_keeps_closure_references_without_closure_nodes() {
    let source = r#"
mod store;
pub fn run() -> u32 { let f = |x| store::helper() + x; f(1) }
pub fn immediate() -> u32 { (|| store::helper())() }
pub fn wrapped() -> u32 { let f = (|| store::helper()) as fn() -> u32; f() }
pub const CALLBACK: fn() -> u32 = || store::helper();
pub fn constant() -> u32 { CALLBACK() }
pub fn parameter() { let invoke = |f: fn() -> u32| f(); invoke(store::helper); }
"#;
    let before = repository(source);
    let after = repository(&source.replace("store::helper() + x", "store::replacement() + x"));
    let analyze = |directory: &Path| {
        fs::write(
            directory.join("src/store.rs"),
            "pub fn helper() -> u32 { 1 } pub fn replacement() -> u32 { 2 }",
        )
        .unwrap();
        analyzer()
            .analyze(&AnalysisInput {
                repository_root: directory.to_path_buf(),
                files: vec!["src/lib.rs".into()],
            })
            .unwrap()
    };
    let old = analyze(before.path());
    let new = analyze(after.path());
    let all = nodes(&old);
    assert!(all.values().all(|node| node.data().native_kind != "closure"
        && !matches!(node, CodeNode::ExecutionRegion(_))));
    let find = |name: &str| {
        *all.values()
            .find(|node| node.data().name.as_deref() == Some(name))
            .unwrap()
    };
    let helper = old
        .external_symbols()
        .iter()
        .find(|(_, symbol)| symbol.name == "helper")
        .unwrap()
        .0;
    let calls_helper = |node: &CodeNode| {
        node.data().references.iter().any(|reference| {
            reference.semantic == "code.calls"
                && reference.target
                    == ReferenceTarget::External {
                        symbol: helper.clone(),
                    }
        })
    };
    for name in ["run", "immediate", "wrapped", "CALLBACK"] {
        assert!(calls_helper(find(name)), "{name}: {:?}", find(name));
    }
    for name in ["run", "immediate", "wrapped", "constant"] {
        let node = find(name);
        assert!(
            node.data().references.iter().all(|reference| !matches!(
                reference.target,
                ReferenceTarget::Unresolved { .. }
            ) && reference.target
                != ReferenceTarget::Node(node.data().id.clone())),
            "{name}: {node:?}"
        );
    }
    assert!(find("parameter")
        .data()
        .references
        .iter()
        .any(|reference| reference.semantic == "code.calls"
            && reference.target == ReferenceTarget::Unresolved { name: "f".into() }));
    let changes = old.diff(&new).unwrap().nodes;
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(changes[0].id, find("run").data().id);
    assert_eq!(changes[0].content, Some(ChangeKind::Modified));
    assert!(changes[0].contains.is_empty());
    assert_eq!(changes[0].references.len(), 2, "{changes:?}");
    save("closures-before", &old);
    save("closures-after", &new);
}

/// 省略类型使用和读取关系，同时保留调用、写入、函数值、实现归属及源码 diff。
#[test]
#[ignore = "需要已提供的 rust-analyzer 和 Rust 工具链，单独串行运行"]
fn real_server_omits_type_uses_and_reads() {
    let source = r#"
mod store;
trait Run {}
struct Unit;
struct State { value: u32 }
const LOCAL: u32 = 1;
impl Run for State {}
impl State {
    fn update(&mut self) -> u32 {
        self.value += store::helper();
        self.value + LOCAL + store::VALUE
    }
}
fn callback() -> fn() -> u32 { store::helper }
fn mentions_type(_: State) -> State { State { value: LOCAL } }
fn mentions_unit() { let _ = Unit; let _ = store::Unit; }
fn closure(state: &State) -> u32 { let f = || store::helper() + state.value + LOCAL; f() }
"#;
    let before = repository(source);
    let after = repository(&source.replace("LOCAL + store::VALUE", "LOCAL + 2"));
    let analyze = |directory: &Path| {
        fs::write(
            directory.join("src/store.rs"),
            "pub fn helper() -> u32 { 1 } pub const VALUE: u32 = 2; pub struct Unit;",
        )
        .unwrap();
        analyzer()
            .analyze(&AnalysisInput {
                repository_root: directory.to_path_buf(),
                files: vec![PathBuf::from("src/lib.rs")],
            })
            .unwrap()
    };
    let old = analyze(before.path());
    let new = analyze(after.path());
    let all = nodes(&old);
    let find = |name: &str| {
        *all.values()
            .find(|node| node.data().name.as_deref() == Some(name))
            .unwrap()
    };
    let references: Vec<_> = all
        .values()
        .flat_map(|node| &node.data().references)
        .collect();
    assert!(references
        .iter()
        .all(|reference| !matches!(reference.semantic.as_str(), "code.type_use" | "code.reads")));
    assert_eq!(find("State").category(), CodeCategory::Type);
    assert_eq!(find("value").category(), CodeCategory::Value);
    assert!(find("mentions_type").data().references.is_empty());
    assert!(find("mentions_unit").data().references.is_empty());
    assert!(find("update")
        .data()
        .references
        .iter()
        .any(|reference| reference.semantic == "code.writes"));
    assert!(find("update")
        .data()
        .references
        .iter()
        .any(|reference| reference.semantic == "code.calls"));
    assert!(find("closure")
        .data()
        .references
        .iter()
        .any(|reference| reference.semantic == "code.calls"));
    assert!(find("callback")
        .data()
        .references
        .iter()
        .any(|reference| reference.semantic == "code.references"));
    assert!(references
        .iter()
        .any(|reference| reference.semantic == "code.implements"));
    assert!(references
        .iter()
        .any(|reference| reference.semantic == "code.applies_to"));
    assert!(old
        .external_symbols()
        .values()
        .all(|symbol| symbol.name == "helper"));
    let diff = old.diff(&new).unwrap();
    assert_eq!(diff.nodes.len(), 1, "{diff:?}");
    assert_eq!(diff.nodes[0].id, find("update").data().id);
    assert_eq!(diff.nodes[0].content, Some(ChangeKind::Modified));
    assert!(diff.nodes[0].references.is_empty());
    save("omitted-relations-before", &old);
    save("omitted-relations-after", &new);
}

/// 真实 LSP 解析得到的目标同时携带可读名称与仓库范围，保持未选文件和标准库可区分。
#[test]
#[ignore = "需要已提供的 rust-analyzer、Rust 工具链和 rust-src，单独串行运行"]
fn real_server_describes_external_symbols() {
    let repository = repository(
        "mod store; use std::sync::Arc; pub fn run() -> Arc<u32> { Arc::new(store::helper()) }",
    );
    let graph = analyzer()
        .analyze(&AnalysisInput {
            repository_root: repository.path().to_path_buf(),
            files: vec![PathBuf::from("src/lib.rs")],
        })
        .unwrap();
    let all = nodes(&graph);
    let references = all.values().flat_map(|node| &node.data().references);
    for reference in references {
        if let ReferenceTarget::External { symbol } = &reference.target {
            let description = graph
                .external_symbols()
                .get(symbol)
                .expect("每个已解析图外目标都应有描述");
            assert!(!description.name.contains("%2F"), "{description:?}");
        }
    }
    assert!(graph
        .external_symbols()
        .values()
        .any(|symbol| symbol.name == "helper" && symbol.scope == SymbolScope::Repository));
    assert!(
        graph
            .external_symbols()
            .values()
            .any(|symbol| symbol.name.starts_with("Arc<")
                && symbol.name.ends_with("::new")
                && symbol.scope == SymbolScope::Dependency),
        "{:#?}",
        graph.external_symbols()
    );
    assert!(!all
        .values()
        .any(|node| node.data().name.as_deref() == Some("helper")));
    save("external-symbols", &graph);
}

/// 当前仓库回归覆盖较大的真实类型、模块与关联，不依赖穿刺时固定的 crate 名。
#[test]
#[ignore = "需要已提供的 rust-analyzer 和当前仓库的完整依赖，单独串行运行"]
fn real_server_analyzes_current_repository() {
    let repository = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let files = [
        "src-tauri/src/structure_review/mod.rs",
        "src-tauri/src/structure_review/code_graph/mod.rs",
        "src-tauri/src/structure_review/code_graph/node.rs",
        "src-tauri/src/structure_review/code_graph/graph.rs",
        "src-tauri/src/structure_review/code_graph/analyzer.rs",
        "src-tauri/src/structure_review/code_graph/diff.rs",
        "src-tauri/src/structure_review/code_graph/error.rs",
    ];
    let input = AnalysisInput {
        repository_root: repository.to_path_buf(),
        files: files.into_iter().map(PathBuf::from).collect(),
    };
    let graph = analyzer().analyze(&input).unwrap();
    let all = nodes(&graph);
    assert!(all
        .values()
        .any(|node| node.data().name.as_deref() == Some("CodeGraph")));
    assert!(!all
        .values()
        .any(|node| node.data().native_kind == "closure"));
    assert!(all.values().flat_map(|node| &node.data().references).any(|reference| matches!(&reference.target, ReferenceTarget::External { symbol } if symbol.contains("validate"))));
    eprintln!(
        "当前仓库：{} 个节点，{} 条关联",
        all.len(),
        all.values()
            .map(|node| node.data().references.len())
            .sum::<usize>()
    );
    save("current-repository", &graph);
}

/// 可选地保存本次真实验收的完整图证据；输出目录由测试调用者指定。
fn save(name: &str, graph: &CodeGraph) {
    if let Some(directory) = std::env::var_os("NARRANT_CODE_GRAPH_OUTPUT") {
        let directory = PathBuf::from(directory);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join(format!("{name}.txt")),
            format!("{graph:#?}\n"),
        )
        .unwrap();
    }
}
