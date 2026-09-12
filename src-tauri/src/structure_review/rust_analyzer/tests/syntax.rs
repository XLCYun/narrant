use std::path::Path;

use crate::lsp::Position;

use super::{byte_offset, position, Document};
use crate::structure_review::code_graph::{ChangeKind, CodeGraph, CodeRoot};

/// 使用同一源码身份构造图，直接验证归一化结果在公共 diff 上的行为。
fn graph(source: &str) -> CodeGraph {
    let repository = Path::new(if cfg!(windows) { "C:/repo" } else { "/repo" });
    let document =
        Document::parse(repository, repository.join("src/lib.rs"), source.to_owned()).unwrap();
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

/// 成员、嵌套函数、局部常量和闭包都单独比较，不重复修改整个外层函数。
#[test]
fn nested_body_edits_belong_only_to_their_owner() {
    let before = "struct S { n: u32 } impl S { fn run() { const N: u32 = 1; fn helper() -> u32 { 2 } let f = || 3; consume(N + helper() + f()); } }";
    for (from, to, name, kind) in [
        ("= 1", "= 9", Some("N"), "const"),
        ("{ 2 }", "{ 8 }", Some("helper"), "fn"),
        ("|| 3", "|| 7", None, "closure"),
    ] {
        let old = graph(before);
        let after = graph(&before.replace(from, to));
        let diff = old.diff(&after).unwrap();
        assert_eq!(diff.nodes.len(), 1, "{diff:?}");
        assert_eq!(diff.nodes[0].content, Some(ChangeKind::Modified));
        let changed = after.node(&diff.nodes[0].id).unwrap().data();
        assert_eq!(changed.name.as_deref(), name);
        assert_eq!(changed.native_kind, kind);
    }
}

/// 字段列表中的逗号属于成员布局，新增字段只改变父节点的 contains。
#[test]
fn adding_fields_does_not_modify_the_type_own_content() {
    for (before, after) in [
        ("struct S { a: u32 }", "struct S { a: u32, b: u32 }"),
        ("struct S(u32);", "struct S(u32, u32);"),
        ("enum E { A }", "enum E { A, B }"),
    ] {
        let old = graph(before);
        let new = graph(after);
        let diff = old.diff(&new).unwrap();
        let parent = diff
            .nodes
            .iter()
            .find(|change| change.id == new.root().contains[0])
            .unwrap();
        assert_eq!(parent.content, None, "{diff:?}");
        assert_eq!(parent.contains.len(), 1);
    }
}

/// 类型表达式也可能包含独立声明；子常量的实现不重复嵌入字段签名。
#[test]
fn nested_items_in_type_expressions_have_separate_content() {
    let before = graph("struct S { data: [u8; { const N: usize = 1; N }] }");
    let after = graph("struct S { data: [u8; { const N: usize = 2; N }] }");
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

/// 格式和普通注释不变更图；字符串中的空白和注释样式内容仍有意义。
#[test]
fn normalization_preserves_literal_contents() {
    let old = graph("fn f() -> &'static str { r#\"a // b\"# }");
    let formatted = graph("// 😀\nfn /* 中 */ f ( ) -> & 'static str {\n r#\"a // b\"#\n }");
    assert!(old.diff(&formatted).unwrap().nodes.is_empty());
    let changed = graph("fn f() -> &'static str { r#\"a  // b\"# }");
    assert_eq!(old.diff(&changed).unwrap().nodes.len(), 1);
}

/// 声明成员的顺序不影响父代码，闭包的使用顺序则属于父代码。
#[test]
fn preserves_child_expression_usage_but_ignores_item_order() {
    let before = "fn f() { fn a() {} const N: u8 = 1; let x = || 1; let y = || 2; choose(x, y); }";
    let reordered = before.replace("fn a() {} const N: u8 = 1;", "const N: u8 = 1; fn a() {}");
    assert!(graph(before)
        .diff(&graph(&reordered))
        .unwrap()
        .nodes
        .is_empty());
    let after = graph(&before.replace("choose(x, y)", "choose(y, x)"));
    let diff = graph(before).diff(&after).unwrap();
    assert_eq!(diff.nodes.len(), 1);
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

/// 不展开生成项；顶层宏调用参数变化仍有独立比较对象。
#[test]
fn top_level_macro_invocations_have_content() {
    let old = graph("macro_rules! declare { ($n:ident) => { fn $n() {} }; } declare!(A);");
    let after = graph("macro_rules! declare { ($n:ident) => { fn $n() {} }; } declare!(B);");
    let diff = old.diff(&after).unwrap();
    assert_eq!(diff.nodes.len(), 1);
    assert_eq!(
        after.node(&diff.nodes[0].id).unwrap().data().native_kind,
        "macro_invocation"
    );
    assert_eq!(after.root().contains.len(), 2);
}

/// 宏 token tree 的标点粘连可影响含义，不能一律删除空格。
#[test]
fn macro_punctuation_jointness_is_significant() {
    let before = graph("m!(a >= b);");
    let after = graph("m!(a > = b);");
    assert_eq!(before.diff(&after).unwrap().nodes.len(), 1);
    assert!(graph("m!([1,], {a},);")
        .diff(&graph("m!( [ 1, ], { a }, );"))
        .unwrap()
        .nodes
        .is_empty());
}

/// 未启用分支也完整列出显式声明，而源代码语法错误不能返回部分图。
#[test]
fn preserves_cfg_declarations_and_rejects_parse_failures() {
    assert_eq!(
        graph("#[cfg(any())] fn inactive() {} fn active() {}")
            .root()
            .contains
            .len(),
        2
    );
    let repository = Path::new(if cfg!(windows) { "C:/repo" } else { "/repo" });
    assert!(Document::parse(repository, repository.join("lib.rs"), "fn broken( {".into()).is_err());
}

/// UTF-16 转换覆盖代理对、CRLF、末尾空行以及所有越界情形。
#[test]
fn positions_round_trip_and_reject_invalid_boundaries() {
    let source = "中😀x\r\nz\n";
    for byte in [0, 3, 7, 8, 10, 11, 12] {
        let offset = ra_ap_syntax::TextSize::from(byte);
        assert_eq!(
            byte_offset(source, &position(source, offset)).unwrap(),
            offset
        );
    }
    for invalid in [
        Position {
            line: 0,
            character: 2,
        },
        Position {
            line: 0,
            character: 99,
        },
        Position {
            line: 9,
            character: 0,
        },
    ] {
        assert!(byte_offset(source, &invalid).is_err());
    }
}
