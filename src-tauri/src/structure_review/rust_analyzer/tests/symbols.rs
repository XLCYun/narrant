use super::describe;
use crate::structure_review::{code_graph::SymbolScope, rust_analyzer::syntax::Document};
use std::{collections::BTreeSet, path::Path};

/// 图外仓库声明仍标记为仓库内，名称保留源码字符，文件 ID 中的转义保持不变。
#[test]
fn repository_symbols_have_readable_names_without_rewriting_ids() {
    let repository = Path::new(if cfg!(windows) { "C:/repo" } else { "/repo" });
    let document = Document::parse(
        repository,
        repository.join("src/未选中.rs"),
        "mod helpers { pub struct Cache; impl Cache { pub fn 读取() {} } }".into(),
        Default::default(),
    )
    .unwrap();
    let declaration = document
        .declarations
        .iter()
        .find(|item| item.node.data().name.as_deref() == Some("读取"))
        .unwrap();
    let symbol = declaration.node.data().id.0.as_str();
    assert!(symbol.contains("%2F"));
    let descriptions = describe(repository, &document, &BTreeSet::from([symbol]));
    assert_eq!(descriptions.len(), 1);
    assert_eq!(descriptions[symbol].name, "helpers::Cache::读取");
    assert_eq!(descriptions[symbol].scope, SymbolScope::Repository);
}

/// 标准库、registry 和 Git 依赖统一位于仓库外，不按符号显示名猜测来源。
#[test]
fn standard_library_and_third_party_symbols_are_dependencies() {
    let repository = Path::new(if cfg!(windows) { "C:/repo" } else { "/repo" });
    let outside = Path::new(if cfg!(windows) { "C:/tools" } else { "/tools" });
    for path in [
        "lib/rustlib/src/rust/library/alloc/src/sync.rs",
        "registry/src/index/library-1.0/src/lib.rs",
        "git/checkouts/library/revision/src/lib.rs",
    ] {
        let document = Document::parse(repository, outside.join(path), "pub struct Arc<T>(T); impl<T> Arc<T> { pub fn new(value: T) -> Self { Self(value) } pub fn unused() {} }".into(), Default::default()).unwrap();
        let declaration = document
            .declarations
            .iter()
            .find(|item| item.node.data().name.as_deref() == Some("new"))
            .unwrap();
        let symbol = declaration.node.data().id.0.as_str();
        let descriptions = describe(repository, &document, &BTreeSet::from([symbol]));
        assert_eq!(descriptions.len(), 1);
        assert_eq!(descriptions[symbol].name, "Arc<T>::new");
        assert_eq!(descriptions[symbol].scope, SymbolScope::Dependency);
    }
}
