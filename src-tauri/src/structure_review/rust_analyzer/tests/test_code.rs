use std::path::Path;

use crate::structure_review::{
    code_graph::{AnalysisOptions, CodeGraph, CodeRoot},
    rust_analyzer::syntax::Document,
};

/// 使用固定仓库身份验证提取结果，默认选项必须排除测试代码。
fn document(source: &str, include_tests: bool) -> Document {
    let root = Path::new(if cfg!(windows) { "C:/repo" } else { "/repo" });
    Document::parse(
        root,
        root.join("lib.rs"),
        source.into(),
        AnalysisOptions { include_tests },
    )
    .unwrap()
}

/// 直接测试入口和所有测试专用子声明都不生成对象，包括模块、类型、字段和函数。
#[test]
fn excludes_test_attributes_and_inherited_local_scopes() {
    assert!(!AnalysisOptions::default().include_tests);
    let source = r#"
#[test] fn checks() { struct Fixture; fn helper() {} }
#[cfg(any())] #[test] fn disabled_test() {}
#[cfg(test)] mod checks_module { pub struct Helper; pub fn setup() {} }
mod inner { #![cfg(test)] pub const VALUE: u8 = 1; }
struct Live { #[cfg(test)] helper: u8, value: u8 }
impl Live { #[cfg(all(test, unix))] fn debug() {} fn run() {} }
fn business() { #[cfg(test)] { fn nested() {} only_test(); } normal(); }
"#;
    let document = document(source, false);
    let names: Vec<_> = document
        .declarations
        .iter()
        .filter_map(|item| item.node.data().name.as_deref())
        .collect();
    assert_eq!(names, ["Live", "value", "run", "business"]);
    let business = document
        .declarations
        .iter()
        .find(|item| item.node.data().name.as_deref() == Some("business"))
        .unwrap();
    assert!(!business
        .node
        .data()
        .code
        .as_ref()
        .unwrap()
        .contains("only_test"));
    assert!(business
        .node
        .data()
        .code
        .as_ref()
        .unwrap()
        .contains("normal"));
    assert!(document
        .owner((source.find("only_test").unwrap() as u32).into())
        .is_none());
}

/// test 必须是声明存在的必要条件，不能仅搜索属性文本里是否出现 test。
#[test]
fn distinguishes_required_test_conditions_from_shared_or_unknown_conditions() {
    for attribute in [
        "#[cfg(test)]",
        "#[cfg(all(test, unix))]",
        "#[cfg(not(not(test)))]",
        "#[cfg(any(all(test, unix), all(test, windows)))]",
        "#[cfg_attr(not(test), cfg(any()))]",
        "#[cfg_attr(all(), cfg(test))]",
    ] {
        assert!(
            document(&format!("{attribute} fn f() {{}}"), false)
                .declarations
                .is_empty(),
            "{attribute}"
        );
    }
    for attribute in [
        "",
        "#[cfg(unix)]",
        "#[cfg(not(test))]",
        "#[cfg(any(test, unix))]",
        "#[cfg(any())]",
        "#[cfg(all(test, any()))]",
        "#[cfg(test = \"yes\")]",
        "#[cfg_attr(test, test)]",
        "#[cfg_attr(unix, cfg(test))]",
        "#[tokio::test]",
        "#[ignore]",
        "#[doc = \"#[test]\"]",
    ] {
        assert_eq!(
            document(&format!("{attribute} fn test_example() {{}}"), false)
                .declarations
                .len(),
            1,
            "{attribute}"
        );
    }
}

/// 文件内属性作用于整个源文件；显式启用开关后保留全部原有对象和实现内容。
#[test]
fn includes_tests_only_when_requested() {
    let source = "#![cfg(test)] struct Fixture; fn setup() {} #[test] fn checks() { setup(); }";
    assert!(document(source, false).declarations.is_empty());
    assert_eq!(document(source, true).declarations.len(), 3);
    let source = "fn run() { #[test] fn nested() { fail(); } keep(); }";
    assert_eq!(document(source, false).declarations.len(), 1);
    assert_eq!(document(source, true).declarations.len(), 2);
}

/// 通配、同名或别名导入都不影响 #[test] 的语法规则，开启开关才恢复测试声明。
#[test]
fn excludes_test_attributes_regardless_of_imports() {
    for import in [
        "use framework::test;",
        "use framework::check as test;",
        "use framework::*;",
        "#[macro_use] extern crate framework;",
    ] {
        let source = format!(
            "{import} #[test] fn checks() {{}} #[cfg(test)] fn helper() {{}} fn live() {{}}"
        );
        for include_tests in [false, true] {
            let parsed = document(&source, include_tests);
            let names: Vec<_> = parsed
                .declarations
                .iter()
                .filter_map(|item| item.node.data().name.as_deref())
                .collect();
            assert!(names.contains(&"live"), "{import}");
            for name in ["checks", "helper"] {
                assert_eq!(names.contains(&name), include_tests, "{import}: {name}");
            }
        }
    }
}

/// 所有层级中的 #[test] 都被排除，导入出现的位置不改变过滤结果。
#[test]
fn excludes_test_attributes_in_nested_import_scopes() {
    let source = r#"
#[test] fn direct_test() {}
mod checks { use framework::*; #[test] fn maybe_custom() {} }
mod plain { #[test] fn nested_test() {} }
fn production() { use framework::test; #[test] fn local_custom() {} }
#[test] fn body_import() { use framework::test; }
"#;
    let parsed = document(source, false);
    let names: Vec<_> = parsed
        .declarations
        .iter()
        .filter_map(|item| item.node.data().name.as_deref())
        .collect();
    for name in [
        "direct_test",
        "nested_test",
        "body_import",
        "maybe_custom",
        "local_custom",
    ] {
        assert!(!names.contains(&name), "{name}: {names:?}");
    }
    assert!(names.contains(&"production"));
}

/// 构造真实公共图，验证被排除的测试改动不会泄漏为父对象的内容或包含变化。
fn graph(source: &str, include_tests: bool) -> CodeGraph {
    let document = document(source, include_tests);
    CodeGraph::new(
        CodeRoot {
            contains: document
                .declarations
                .iter()
                .filter(|item| item.parent.is_none())
                .map(|item| item.node.data().id.clone())
                .collect(),
        },
        document
            .declarations
            .into_iter()
            .map(|item| item.node)
            .collect(),
    )
    .unwrap()
}

/// 通配导入后的嵌套测试整体排除，不得留下父对象修改或悬空包含。
#[test]
fn excluded_tests_under_glob_imports_do_not_change_parent_graph() {
    let before = "use scope::*; fn live() {}";
    let after = "use scope::*; fn live() { #[test] fn checks() { #[cfg(test)] struct Fixture; } }";
    assert!(graph(before, false)
        .diff(&graph(after, false))
        .unwrap()
        .nodes
        .is_empty());
    assert!(!graph(before, true)
        .diff(&graph(after, true))
        .unwrap()
        .nodes
        .is_empty());
}

/// 单独新增或修改嵌套测试都不应产生业务函数的结构 diff。
#[test]
fn excluded_test_edits_do_not_change_parent_content_or_containment() {
    let before = "fn run() { live(); }";
    let after = "fn run() { #[test] fn nested() { check(); } live(); }";
    assert!(graph(before, false)
        .diff(&graph(after, false))
        .unwrap()
        .nodes
        .is_empty());
    assert!(!graph(before, true)
        .diff(&graph(after, true))
        .unwrap()
        .nodes
        .is_empty());
    assert!(graph(after, false)
        .diff(&graph(&after.replace("check", "changed"), false))
        .unwrap()
        .nodes
        .is_empty());
}
