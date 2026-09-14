use std::path::Path;

use crate::lsp::Position;

use super::{byte_offset, position, Document};
use crate::structure_review::code_graph::{ChangeKind, CodeGraph, CodeRoot};

/// 只提取字段的类型结构；泛型嵌套可见，数组长度、常量泛型和宏 token 中的类型不混入。
#[test]
fn field_types_walk_nested_types_without_entering_executable_code() {
    use ra_ap_syntax::{AstNode, Edition, SourceFile, SyntaxKind};
    let parsed = SourceFile::parse(
        r#"
struct Fields {
    nested: Option<Vec<(one::Item, &'static two::Item, *const Other)>>,
    array: [Element; { core::mem::size_of::<NotAFieldType>() }],
    constant: Buffer<Element, { compute::<NotAFieldType>() }>,
    callback: fn(Input) -> Output,
    opaque: make_type!(NotAFieldType),
}
struct Tuple(Slice, &'static [Element]);
enum Choice { Named { item: Item }, Tuple(Item) }
union Storage { item: Item }
fn run(_: Parameter) -> Returned { let _: Local; }
type Alias = Underlying;
const VALUE: ConstantType = make();
"#,
        Edition::Edition2024,
    );
    assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
    let tree = parsed.tree();
    let mut names = Vec::new();
    for node in tree.syntax().descendants() {
        let found = super::field_type_names(&node);
        if !matches!(
            node.kind(),
            SyntaxKind::RECORD_FIELD | SyntaxKind::TUPLE_FIELD
        ) {
            assert!(found.is_empty());
        }
        names.extend(found.iter().map(|name| name.text().to_string()));
    }
    assert_eq!(
        names,
        [
            "Option", "Vec", "Item", "Item", "Other", "Element", "Buffer", "Element", "Input",
            "Output", "Slice", "Element", "Item", "Item", "Item"
        ]
    );
}

/// 使用同一源码身份构造图，直接验证归一化结果在公共 diff 上的行为。
fn graph(source: &str) -> CodeGraph {
    let repository = Path::new(if cfg!(windows) { "C:/repo" } else { "/repo" });
    let document = Document::parse(
        repository,
        repository.join("src/lib.rs"),
        source.to_owned(),
        Default::default(),
    )
    .unwrap();
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

/// 原文保留属性、中文注释、CRLF、首行缩进和子声明，规范化比较仍只关注自身语义。
#[test]
fn preserves_original_source_without_changing_structural_comparison() {
    let source = "mod demo {\r\n    /// 原始注释\r\n    #[inline]\r\n    fn run() {\r\n        // 函数内注释\r\n        const N: u8 = 1;\r\n        consume(N);\r\n    }\r\n}";
    let before = graph(source);
    let module = before.node(&before.root().contains[0]).unwrap();
    assert_eq!(module.data().source.as_deref(), Some(source));
    let function = before.node(&module.data().contains[0]).unwrap();
    assert_eq!(
        function.data().source.as_deref(),
        Some("    /// 原始注释\r\n    #[inline]\r\n    fn run() {\r\n        // 函数内注释\r\n        const N: u8 = 1;\r\n        consume(N);\r\n    }")
    );
    assert!(!function.data().code.as_ref().unwrap().contains("const"));
    let changed = source
        .replace("原始注释", "修改后的注释")
        .replace("\r\n", "\n")
        .replace("    ", "\t");
    let after = graph(&changed);
    assert_ne!(
        function.data().source,
        after.node(&function.data().id).unwrap().data().source
    );
    assert!(before.diff(&after).unwrap().nodes.is_empty());
}

/// 同行多个声明与多字节标识符按字节范围准确截取，不拼接签名、等号或实现。
#[test]
fn source_ranges_keep_complete_declarations_without_neighboring_items() {
    let source = "fn 甲() {} const 值: u8 = 7; struct S { pub 字段: u8, other: u8 }";
    let graph = graph(source);
    let nodes: Vec<_> = graph
        .root()
        .contains
        .iter()
        .map(|id| graph.node(id).unwrap())
        .collect();
    assert_eq!(nodes[0].data().source.as_deref(), Some("fn 甲() {}"));
    assert_eq!(nodes[1].data().source.as_deref(), Some("const 值: u8 = 7;"));
    let field = graph.node(&nodes[2].data().contains[0]).unwrap();
    assert_eq!(field.data().source.as_deref(), Some("pub 字段: u8"));
}

/// 具名子声明独立比较，闭包内容变化归属于外层函数。
#[test]
fn nested_body_edits_belong_only_to_their_owner() {
    let before = "struct S { n: u32 } impl S { fn run() { const N: u32 = 1; fn helper() -> u32 { 2 } let f = || 3; consume(N + helper() + f()); } }";
    for (from, to, name, kind) in [
        ("= 1", "= 9", Some("N"), "const"),
        ("{ 2 }", "{ 8 }", Some("helper"), "fn"),
        ("|| 3", "|| 7", Some("run"), "fn"),
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

/// 不为闭包或嵌套闭包创建节点、执行区域或占位；其参数和实现完整参与外层比较。
#[test]
fn closures_are_code_of_their_enclosing_function_or_value() {
    for (before, from, to) in [
        (
            "fn run() { let f = |x: u32| x + 1; f(2); }",
            "x + 1",
            "x + 9",
        ),
        ("const F: fn(u32) -> u32 = |x| x + 1;", "x + 1", "x + 9"),
        ("static F: fn(u32) -> u32 = |x| x + 1;", "x + 1", "x + 9"),
        (
            "fn run() { let f = move |x: u32| move |y: u32| x + y; }",
            "x + y",
            "x - y",
        ),
        ("fn run() { let f = |x: u32| x; }", "x: u32", "x: u64"),
    ] {
        let old = graph(before);
        let new = graph(&before.replace(from, to));
        assert_eq!(old.root().contains.len(), 1);
        assert_eq!(new.root().contains.len(), 1);
        let owner = new.node(&new.root().contains[0]).unwrap();
        assert!(owner.data().contains.is_empty());
        let code = owner.data().code.as_ref().unwrap();
        assert!(!code.contains("@code_graph_child"));
        assert!(code.contains('|'), "{code}");
        let changes = old.diff(&new).unwrap().nodes;
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].id, owner.data().id);
        assert_eq!(changes[0].content, Some(ChangeKind::Modified));
        assert!(changes[0].contains.is_empty());
    }
    assert!(graph("fn run(){let f=|x| { x + 1 };}")
        .diff(&graph("fn run() { let f = |x| { /*注释*/ x+1 }; }"))
        .unwrap()
        .nodes
        .is_empty());
}

/// 闭包不截断具名声明的提取；最近的实际声明继续拥有自己的实现和关联。
#[test]
fn named_items_inside_closures_keep_their_own_content() {
    let before = "fn run() { let f = || { fn helper() -> u32 { 1 } helper() }; }";
    let after = graph(&before.replace("{ 1 }", "{ 2 }"));
    let changes = graph(before).diff(&after).unwrap().nodes;
    assert_eq!(changes.len(), 1);
    let helper = after.node(&changes[0].id).unwrap();
    assert_eq!(helper.data().name.as_deref(), Some("helper"));
    let run = after.node(&after.root().contains[0]).unwrap();
    assert_eq!(run.data().contains, vec![helper.data().id.clone()]);
}

/// 闭包内部语义查询的源码位置归属最近的声明，不转移到其他同名对象。
#[test]
fn closure_references_belong_to_the_nearest_modeled_declaration() {
    let repository = Path::new(if cfg!(windows) { "C:/repo" } else { "/repo" });
    let source = "fn run() { let f = || { fn helper() { inside(); } outside(); }; }";
    let document = Document::parse(
        repository,
        repository.join("lib.rs"),
        source.into(),
        Default::default(),
    )
    .unwrap();
    for (name, owner) in [("inside", "helper"), ("outside", "run")] {
        let offset = ra_ap_syntax::TextSize::from(source.find(name).unwrap() as u32);
        assert_eq!(
            document.owner(offset).unwrap().node.data().name.as_deref(),
            Some(owner)
        );
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
    assert!(Document::parse(
        repository,
        repository.join("lib.rs"),
        "fn broken( {".into(),
        Default::default()
    )
    .is_err());
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
