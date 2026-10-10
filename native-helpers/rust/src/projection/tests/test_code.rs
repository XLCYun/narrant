use super::super::{model::AnalysisOptions, syntax::Document};

/// 真实加载临时 Cargo 源码，以同源 AST 复用旧实现的测试属性断言。
fn document(source: &str, include_tests: bool) -> Document {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("Cargo.toml"), "[package]\nname=\"test_scope\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\npath=\"lib.rs\"\n").unwrap();
    std::fs::write(root.path().join("lib.rs"), source).unwrap();
    let repository = root.path().canonicalize().unwrap();
    let project = crate::RustProject::load(
        &repository.join("Cargo.toml"),
        &crate::LoadPolicy::repository_only(),
    )
    .unwrap();
    Document::parse(
        &repository,
        repository.join("lib.rs"),
        &project,
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
    assert!(
        !business
            .node
            .data()
            .code
            .as_ref()
            .unwrap()
            .contains("only_test")
    );
    assert!(
        business
            .node
            .data()
            .code
            .as_ref()
            .unwrap()
            .contains("normal")
    );
    assert!(
        document
            .owner((source.find("only_test").unwrap() as u32).into())
            .is_none()
    );
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
