use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use crate::structure_review::{
    code_graph::{
        AnalysisInput, ChangeKind, CodeCategory, CodeGraph, CodeNode, LanguageAnalyzer, NodeId,
        ReferenceTarget, SetChange,
    },
    codegraph_client::CodeGraphAnalyzer,
};

/// 直接调用公共 CLI 与已构建 helper；工具链在测试边界隔离，不注册生产工厂。
pub(super) fn analyzer() -> CodeGraphAnalyzer {
    CodeGraphAnalyzer::from_pack("rust", &super::candidate::root("rust")).unwrap()
}

/// 通过公共根和节点查询遍历结果，验收不依赖图内部存储。
pub(super) fn nodes(graph: &CodeGraph) -> BTreeMap<NodeId, &CodeNode> {
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

/// 用真实 CodeGraph 包 协议与公共 CLI确认体内变化、闭包归属、跨文件范围和 cfg/宏边界。
#[test]
#[ignore = "需要已构建 Rust helper 和项目工具链，单独串行运行"]
fn rust_helper_compares_prepared_repositories() {
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
                options: crate::structure_review::code_graph::AnalysisOptions {
                    include_tests: true,
                },
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
    assert!(all
        .values()
        .flat_map(|node| &node.data().references)
        .all(|reference| reference.semantic != "code.writes"));
    assert!(find("update")
        .data()
        .references
        .iter()
        .any(|reference| reference.semantic == "code.calls"
            && reference.target == ReferenceTarget::Node(helper.clone())));
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

/// 闭包不建模，仍经真实原生查询 保留跨文件关联，并将闭包编辑准确计入其外层对象。
#[test]
#[ignore = "需要已构建 Rust helper 和项目工具链，单独串行运行"]
fn rust_helper_keeps_closure_references_without_closure_nodes() {
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
                options: crate::structure_review::code_graph::AnalysisOptions {
                    include_tests: true,
                },
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

/// 真实语义区分别名、同名类型和泛型参数，字段换类型直接形成关系集合 diff。
#[test]
#[ignore = "需要已构建 Rust helper 和项目工具链，单独串行运行"]
fn rust_helper_resolves_graph_local_field_types() {
    let source = r#"
mod outside;
mod types;
mod other { pub struct Item; }
use types::Item as Imported;
type Alias = types::Item;
#[cfg(test)] struct TestOnly;
struct Group {
    items: Vec<Imported>,
    alias: Option<Alias>,
    compound: (types::Item, &'static other::Item, [types::Item; 2]),
    callback: fn(types::Item) -> other::Item,
    hidden: outside::Hidden,
    recursive: Option<Box<Self>>,
    #[cfg(test)] omitted: TestOnly,
}
struct Shadow<Imported> { generic: Imported }
struct Tuple(types::Item);
enum Choice { Named { named: types::Item }, Tuple(other::Item) }
union Storage { stored: std::mem::ManuallyDrop<types::Item> }
"#;
    let before = repository(source);
    let after = repository(
        &source
            .replace("items: Vec<Imported>", "items: Vec<other::Item>")
            .replace("alias: Option<Alias>", "alias: Vec<Alias>"),
    );
    let analyze = |directory: &Path| {
        fs::write(directory.join("src/types.rs"), "pub struct Item;").unwrap();
        fs::write(directory.join("src/outside.rs"), "pub struct Hidden;").unwrap();
        analyzer()
            .analyze(&AnalysisInput {
                repository_root: directory.to_path_buf(),
                files: vec!["src/lib.rs".into(), "src/types.rs".into()],
                options: Default::default(),
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
    let child = |parent: &str| old.node(&find(parent).data().contains[0]).unwrap();
    let item = child("types").data().id.clone();
    let other = child("other").data().id.clone();
    let targets = |node: &CodeNode| {
        let mut targets = node
            .data()
            .references
            .iter()
            .filter(|reference| reference.semantic == "code.type_use")
            .map(|reference| match &reference.target {
                ReferenceTarget::Node(id) => {
                    assert_eq!(old.node(id).unwrap().category(), CodeCategory::Type);
                    id.clone()
                }
                target => panic!("字段类型不应生成图外目标: {target:?}"),
            })
            .collect::<Vec<_>>();
        targets.sort();
        targets
    };
    assert_ne!(item, other);
    assert_eq!(targets(find("items")), std::slice::from_ref(&item));
    assert_eq!(targets(find("alias")), [find("Alias").data().id.clone()]);
    let mut compound = vec![item.clone(), other.clone()];
    compound.sort();
    assert_eq!(targets(find("compound")), compound);
    assert_eq!(targets(find("callback")), compound);
    assert_eq!(
        targets(find("recursive")),
        [find("Group").data().id.clone()]
    );
    assert!(targets(find("hidden")).is_empty());
    assert!(targets(find("generic")).is_empty());
    assert!(
        targets(find("Alias")).is_empty(),
        "不展开别名定义中的类型关系"
    );
    assert_eq!(targets(find("named")), std::slice::from_ref(&item));
    assert_eq!(targets(find("stored")), std::slice::from_ref(&item));
    let tuple = all
        .values()
        .find(|node| {
            node.category() == CodeCategory::Type && node.data().name.as_deref() == Some("Tuple")
        })
        .unwrap();
    assert_eq!(
        targets(old.node(&tuple.data().contains[0]).unwrap()),
        std::slice::from_ref(&item)
    );
    assert!(!all.values().any(|node| matches!(
        node.data().name.as_deref(),
        Some("TestOnly" | "omitted" | "Hidden")
    )));
    for node in all.values() {
        if !targets(node).is_empty() {
            assert_eq!(node.category(), CodeCategory::Value);
        }
    }
    let diff = old.diff(&new).unwrap();
    let changed = diff
        .nodes
        .iter()
        .find(|node| node.id == find("items").data().id)
        .unwrap();
    assert_eq!(changed.content, Some(ChangeKind::Modified));
    assert_eq!(changed.references.len(), 2);
    assert!(changed.references.iter().any(|change| matches!(change, SetChange::Removed(reference)
        if reference.semantic == "code.type_use" && reference.target == ReferenceTarget::Node(item.clone()))));
    assert!(changed.references.iter().any(|change| matches!(change, SetChange::Added(reference)
        if reference.semantic == "code.type_use" && reference.target == ReferenceTarget::Node(other.clone()))));
    let wrapper = diff
        .nodes
        .iter()
        .find(|node| node.id == find("alias").data().id)
        .unwrap();
    assert_eq!(wrapper.content, Some(ChangeKind::Modified));
    assert!(wrapper.references.is_empty());
    save("field-types-before", &old);
    save("field-types-after", &new);
}

/// 省略非字段类型与读写关系，同时保留调用、函数值、实现归属及源码 diff。
#[test]
#[ignore = "需要已构建 Rust helper 和项目工具链，单独串行运行"]
fn rust_helper_omits_non_field_type_uses_and_data_accesses() {
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
                options: crate::structure_review::code_graph::AnalysisOptions {
                    include_tests: true,
                },
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
    assert!(references.iter().all(|reference| !matches!(
        reference.semantic.as_str(),
        "code.type_use" | "code.reads" | "code.writes"
    )));
    assert_eq!(find("State").category(), CodeCategory::Type);
    assert_eq!(find("value").category(), CodeCategory::Value);
    assert!(find("mentions_type").data().references.is_empty());
    assert!(find("mentions_unit").data().references.is_empty());
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

/// 真实 Cargo 项目的库和集成测试各有根模块，成员变更不误报根自身修改。
#[test]
#[ignore = "需要已构建 Rust helper 和项目工具链，单独串行运行"]
fn rust_helper_preserves_crate_roots() {
    let source = "mod store; pub fn run() -> u32 { store::helper() }";
    let prepare = || {
        let directory = repository(source);
        fs::create_dir(directory.path().join("tests")).unwrap();
        fs::write(
            directory.path().join("tests/github_discovery.rs"),
            "#[test] fn checks() { code_graph_fixture::run(); }",
        )
        .unwrap();
        directory
    };
    let analyze = |directory: &Path, files: &[&str]| {
        analyzer()
            .analyze(&AnalysisInput {
                options: crate::structure_review::code_graph::AnalysisOptions {
                    include_tests: true,
                },
                repository_root: directory.to_owned(),
                files: files.iter().map(PathBuf::from).collect(),
            })
            .unwrap()
    };
    let files = ["src/lib.rs", "src/store.rs", "tests/github_discovery.rs"];
    let before = prepare();
    let graph = analyze(before.path(), &files);
    let all = nodes(&graph);
    let find = |name: &str| {
        *all.values()
            .find(|node| node.data().name.as_deref() == Some(name))
            .unwrap()
    };
    let library = find("src/lib.rs");
    let tests = find("tests/github_discovery.rs");
    assert_eq!(graph.root().contains.len(), 2);
    for root in [library, tests] {
        assert_eq!(root.category(), CodeCategory::Container);
        assert_eq!(root.data().native_kind, "crate");
        assert!(graph.root().contains.contains(&root.data().id));
    }
    assert!(library.data().contains.contains(&find("run").data().id));
    assert!(library.data().contains.contains(&find("store").data().id));
    assert_eq!(
        find("store").data().contains,
        vec![find("helper").data().id.clone()]
    );
    assert_eq!(
        tests.data().contains,
        vec![find("checks").data().id.clone()]
    );
    assert!(find("checks")
        .data()
        .references
        .iter()
        .any(|reference| reference.semantic == "code.calls"
            && reference.target == ReferenceTarget::Node(find("run").data().id.clone())));
    let repeat = prepare();
    assert!(graph
        .diff(&analyze(repeat.path(), &files))
        .unwrap()
        .nodes
        .is_empty());
    fs::write(
        repeat.path().join("tests/github_discovery.rs"),
        "#[test] fn checks() { code_graph_fixture::run(); } #[test] fn added() {}",
    )
    .unwrap();
    let after = analyze(repeat.path(), &files);
    let diff = graph.diff(&after).unwrap();
    let root_change = diff
        .nodes
        .iter()
        .find(|change| change.id == tests.data().id)
        .unwrap();
    assert_eq!(root_change.content, None);
    assert_eq!(root_change.contains.len(), 1);
    assert!(!diff
        .nodes
        .iter()
        .any(|change| change.id == library.data().id));
    let scoped = analyze(before.path(), &["src/lib.rs", "src/store.rs"]);
    assert_eq!(scoped.root().contains, vec![library.data().id.clone()]);
    save("crate-roots-before", &graph);
    save("crate-roots-after", &after);
}

/// 真实服务器验证跨文件测试继承、共享源码和开关恢复；测试修改不污染排除后的 diff。
#[test]
#[ignore = "需要已构建 Rust helper 和项目工具链，单独串行运行"]
fn rust_helper_excludes_test_code_before_graph_analysis() {
    let prepare = |value| {
        let directory = repository(
            r#"
pub fn run() {}
#[cfg(test)] #[path = "specs.rs"] mod specifications;
#[path = "shared.rs"] pub mod live;
#[cfg(test)] #[path = "shared.rs"] mod test_copy;
pub mod runtime;
"#,
        );
        fs::write(directory.path().join("src/specs.rs"), format!("#[path = \"helpers.rs\"] mod deep; struct Fixture; fn setup() -> u32 {{ {value} }} #[test] fn checks() {{ crate::run(); }}")).unwrap();
        fs::write(
            directory.path().join("src/helpers.rs"),
            "pub fn helper() { crate::run(); }",
        )
        .unwrap();
        fs::write(
            directory.path().join("src/shared.rs"),
            "pub fn shared() { crate::run(); }",
        )
        .unwrap();
        fs::write(directory.path().join("src/runtime.rs"), format!("pub fn run_live() {{ crate::run(); }} #[test] fn direct_test() {{ assert_eq!({value}, {value}); }}")).unwrap();
        directory
    };
    let before = prepare(1);
    let after = prepare(2);
    let analyze = |root: &Path, include_tests| {
        analyzer()
            .analyze(&AnalysisInput {
                repository_root: root.to_owned(),
                files: [
                    "src/specs.rs",
                    "src/helpers.rs",
                    "src/shared.rs",
                    "src/runtime.rs",
                ]
                .into_iter()
                .map(Into::into)
                .collect(),
                options: crate::structure_review::code_graph::AnalysisOptions { include_tests },
            })
            .unwrap()
    };
    let old = analyze(before.path(), false);
    let new = analyze(after.path(), false);
    assert!(old.diff(&new).unwrap().nodes.is_empty());
    let retained = nodes(&new);
    let names: Vec<_> = retained
        .values()
        .filter_map(|node| node.data().name.as_deref())
        .collect();
    assert!(
        names.contains(&"shared") && names.contains(&"run_live"),
        "{names:?}"
    );
    for name in ["Fixture", "setup", "checks", "helper", "direct_test"] {
        assert!(!names.contains(&name), "{names:?}");
    }
    let complete = analyze(after.path(), true);
    let complete_nodes = nodes(&complete);
    for name in ["Fixture", "setup", "checks", "helper", "direct_test"] {
        assert!(
            complete_nodes
                .values()
                .any(|node| node.data().name.as_deref() == Some(name)),
            "{name}"
        );
    }
    save("test-scope-excluded", &new);
    save("test-scope-included", &complete);
}

/// 当前仓库回归覆盖较大的真实类型、模块与关联，不依赖穿刺时固定的 crate 名。
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

/// 三档均通过同一协议/Host 完整构图；标准库和全部依赖使用实际项目 sysroot。
#[test]
#[ignore = "需要真实 Rust helper 与项目 rust-src"]
fn rust_helper_all_dependency_profiles_use_real_project_platform() {
    let root =
        repository("pub struct Value; pub fn run() { let _ = std::mem::size_of::<Value>(); }");
    let input = AnalysisInput {
        repository_root: root.path().into(),
        files: vec!["src/lib.rs".into()],
        options: Default::default(),
    };
    let repository = analyzer().analyze(&input).unwrap();
    for scope in [
        crate::structure_review::code_graph::RustDependencyScope::StandardLibrary,
        crate::structure_review::code_graph::RustDependencyScope::AllDependencies,
    ] {
        let analyzer = analyzer().with_dependency_scope(scope);
        let now = std::time::Instant::now();
        let graph = analyzer.analyze(&input).unwrap();
        assert!(repository.diff(&graph).unwrap().nodes.is_empty());
        eprintln!("真实 {scope:?} 完整图耗时 {:?}", now.elapsed());
        assert!(!root.path().join("Cargo.lock").exists());
        assert!(!root.path().join("target").exists());
    }
}

/// 旧语法断言通过真实 CodeGraph 包 与公共 CLI 构图，diff 不绕过统一 CodeGraph 校验。
fn syntax_graph(source: &str, include_tests: bool) -> CodeGraph {
    let root = repository(source);
    analyzer()
        .analyze(&AnalysisInput {
            repository_root: root.path().into(),
            files: vec!["src/lib.rs".into()],
            options: crate::structure_review::code_graph::AnalysisOptions { include_tests },
        })
        .unwrap()
}

/// 类型表达式中的嵌套声明独立比较，不连带修改字段或类型内容。
#[test]
#[ignore = "需要实际 Rust helper，单独串行运行"]
fn rust_helper_syntax_diff_nested_items_in_type_expressions() {
    let before = syntax_graph("struct S { data: [u8; { const N: usize = 1; N }] }", false);
    let after = syntax_graph("struct S { data: [u8; { const N: usize = 2; N }] }", false);
    let diff = before.diff(&after).unwrap();
    assert_eq!(diff.nodes.len(), 1, "{diff:?}");
    assert_eq!(
        after
            .node(&diff.nodes[0].id)
            .unwrap()
            .data()
            .name
            .as_deref(),
        Some("N")
    );
}

/// 注释与排版不改变结构，字符串内有意义空白保持可见。
#[test]
#[ignore = "需要实际 Rust helper，单独串行运行"]
fn rust_helper_syntax_diff_preserves_literal_contents() {
    let old = syntax_graph("fn f() -> &'static str { r#\"a // b\"# }", false);
    let formatted = syntax_graph(
        "// 😀\nfn /* 中 */ f ( ) -> & 'static str {\n r#\"a // b\"#\n }",
        false,
    );
    assert!(old.diff(&formatted).unwrap().nodes.is_empty());
    let changed = syntax_graph("fn f() -> &'static str { r#\"a  // b\"# }", false);
    assert_eq!(old.diff(&changed).unwrap().nodes.len(), 1);
}

/// 成员重排不修改父内容，但闭包使用顺序改变仍归属父函数。
#[test]
#[ignore = "需要实际 Rust helper，单独串行运行"]
fn rust_helper_syntax_diff_preserves_usage_but_ignores_item_order() {
    let before = "fn f() { fn a() {} const N: u8 = 1; let x = || 1; let y = || 2; choose(x, y); }";
    let reordered = before.replace("fn a() {} const N: u8 = 1;", "const N: u8 = 1; fn a() {}");
    assert!(syntax_graph(before, false)
        .diff(&syntax_graph(&reordered, false))
        .unwrap()
        .nodes
        .is_empty());
    let after = syntax_graph(&before.replace("choose(x, y)", "choose(y, x)"), false);
    let diff = syntax_graph(before, false).diff(&after).unwrap();
    assert_eq!(diff.nodes.len(), 1, "{diff:?}");
    assert_eq!(
        after
            .node(&diff.nodes[0].id)
            .unwrap()
            .data()
            .name
            .as_deref(),
        Some("f")
    );
}

/// 顶层宏实参编辑只修改宏调用，原生加载不把展开生成项加入图。
#[test]
#[ignore = "需要实际 Rust helper，单独串行运行"]
fn rust_helper_syntax_diff_top_level_macro_arguments() {
    let old = syntax_graph(
        "macro_rules! declare { ($n:ident) => { fn $n() {} }; } declare!(A);",
        false,
    );
    let after = syntax_graph(
        "macro_rules! declare { ($n:ident) => { fn $n() {} }; } declare!(B);",
        false,
    );
    let diff = old.diff(&after).unwrap();
    assert_eq!(diff.nodes.len(), 1, "{diff:?}");
    assert_eq!(
        after.node(&diff.nodes[0].id).unwrap().data().native_kind,
        "macro_invocation"
    );
    let after_nodes = nodes(&after);
    let crate_node = after_nodes
        .values()
        .find(|node| node.data().native_kind == "crate")
        .unwrap();
    assert_eq!(crate_node.data().contains.len(), 2);
}

/// 宏标点粘连参与比较，其他 token tree 空白不产生伪差异。
#[test]
#[ignore = "需要实际 Rust helper，单独串行运行"]
fn rust_helper_syntax_diff_macro_punctuation_jointness() {
    let before = syntax_graph("m!(a >= b);", false);
    let after = syntax_graph("m!(a > = b);", false);
    assert_eq!(before.diff(&after).unwrap().nodes.len(), 1);
    assert!(syntax_graph("m!([1,], {a},);", false)
        .diff(&syntax_graph("m!( [ 1, ], { a }, );", false))
        .unwrap()
        .nodes
        .is_empty());
}

/// 通配导入下新增嵌套测试整体排除，父图与包含关系不产生变化。
#[test]
#[ignore = "需要实际 Rust helper，单独串行运行"]
fn rust_helper_syntax_diff_excludes_nested_tests_under_glob_imports() {
    let before = "use scope::*; fn live() {}";
    let after = "use scope::*; fn live() { #[test] fn checks() { #[cfg(test)] struct Fixture; } }";
    assert!(syntax_graph(before, false)
        .diff(&syntax_graph(after, false))
        .unwrap()
        .nodes
        .is_empty());
    assert!(!syntax_graph(before, true)
        .diff(&syntax_graph(after, true))
        .unwrap()
        .nodes
        .is_empty());
}

/// 新增和编辑嵌套测试均不得修改过滤后的业务父函数内容或包含关系。
#[test]
#[ignore = "需要实际 Rust helper，单独串行运行"]
fn rust_helper_syntax_diff_excludes_nested_test_edits() {
    let before = "fn run() { live(); }";
    let after = "fn run() { #[test] fn nested() { check(); } live(); }";
    assert!(syntax_graph(before, false)
        .diff(&syntax_graph(after, false))
        .unwrap()
        .nodes
        .is_empty());
    assert!(!syntax_graph(before, true)
        .diff(&syntax_graph(after, true))
        .unwrap()
        .nodes
        .is_empty());
    assert!(syntax_graph(after, false)
        .diff(&syntax_graph(&after.replace("check", "changed"), false))
        .unwrap()
        .nodes
        .is_empty());
}

/// 保留退役前完整图矩阵的业务范围；多 Cargo、JSON、自定义根均覆盖三档和测试开关。
#[test]
#[ignore = "需要真实 CodeGraph 包 与 rust-src"]
fn rust_helper_whole_graph_project_form_matrix() {
    use crate::structure_review::code_graph::RustDependencyScope;
    for shape in ["multiple", "json", "custom"] {
        let root = tempfile::tempdir().unwrap();
        let source = "pub struct Value; fn target() {} pub fn run() { target(); } #[cfg(any())] fn inactive() {} #[cfg(test)] mod tests { #[test] fn check() { super::target(); } }";
        let files = match shape {
            "multiple" => {
                for name in ["a", "b"] {
                    fs::create_dir(root.path().join(name)).unwrap();
                    fs::write(root.path().join(name).join("Cargo.toml"), format!("[package]\nname=\"{name}\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\npath=\"entry.rs\"\n")).unwrap();
                    fs::write(root.path().join(name).join("entry.rs"), source).unwrap();
                }
                vec!["a/entry.rs".into(), "b/entry.rs".into()]
            }
            "json" => {
                fs::write(root.path().join("entry.rs"), source).unwrap();
                fs::write(root.path().join("rust-project.json"), r#"{"crates":[{"root_module":"entry.rs","edition":"2021","deps":[],"cfg":[]}]}"#).unwrap();
                vec!["entry.rs".into()]
            }
            _ => {
                fs::write(root.path().join("entry.rs"), source).unwrap();
                fs::write(root.path().join("Cargo.toml"), "[package]\nname=\"custom\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\nname=\"custom_library\"\npath=\"entry.rs\"\n").unwrap();
                vec!["entry.rs".into()]
            }
        };
        let mut baseline = std::collections::BTreeMap::<bool, CodeGraph>::new();
        for (scope, profile) in [
            (RustDependencyScope::RepositoryOnly, "rust-repository-v1"),
            (
                RustDependencyScope::StandardLibrary,
                "rust-standard-library-v1",
            ),
            (
                RustDependencyScope::AllDependencies,
                "rust-all-dependencies-v1",
            ),
        ] {
            for include_tests in [false, true] {
                let input = AnalysisInput {
                    repository_root: root.path().into(),
                    files: files.clone(),
                    options: crate::structure_review::code_graph::AnalysisOptions { include_tests },
                };
                let native = analyzer().with_dependency_scope(scope);
                let graph = native.analyze(&input).unwrap();
                let expected_count = match (shape, include_tests) {
                    ("multiple", false) => 10,
                    ("multiple", true) => 14,
                    ("json", false) => 4,
                    ("json", true) => 6,
                    (_, false) => 5,
                    (_, true) => 7,
                };
                assert_eq!(nodes(&graph).len(), expected_count);
                assert_eq!(
                    graph.root().contains.len(),
                    if shape == "multiple" {
                        2
                    } else if shape == "json" {
                        if include_tests {
                            5
                        } else {
                            4
                        }
                    } else {
                        1
                    }
                );
                assert!(graph.external_symbols().is_empty());
                assert!(nodes(&graph)
                    .values()
                    .all(|node| node.data().source.is_some()));
                assert!(nodes(&graph).values().any(|node| node.data().name.as_deref() == Some("run")
                    && node.data().references.iter().any(|reference| matches!(&reference.target,
                        ReferenceTarget::Node(node_id) if graph.node(node_id).unwrap().data().name.as_deref() == Some("target")))));
                if let Some(expected) = baseline.get(&include_tests) {
                    assert_eq!(
                        format!("{expected:#?}"),
                        format!("{graph:#?}"),
                        "{shape}/{profile}/{include_tests}"
                    );
                    assert!(expected.diff(&graph).unwrap().nodes.is_empty());
                } else {
                    baseline.insert(include_tests, graph.clone());
                }
                assert!(nodes(&graph)
                    .values()
                    .any(|node| node.data().name.as_deref() == Some("inactive")));
                assert_eq!(
                    nodes(&graph)
                        .values()
                        .any(|node| node.data().name.as_deref() == Some("check")),
                    include_tests
                );
                eprintln!(
                    "project-form business matrix {shape}/{profile}/tests={include_tests}: {} nodes",
                    nodes(&graph).len()
                );
                save(
                    &format!("project-form-{shape}-{profile}-{include_tests}"),
                    &graph,
                );
            }
        }
    }
}
