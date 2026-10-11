use crate::structure_review::code_graph::{
    AnalysisInput, ChangeKind, CodeGraph, LanguageAnalyzer, ReferenceTarget,
};

/// 真实 JSON 项目通过单侧 helper 与公共 Host，保持旧语法断言的顶层对象范围。
fn graph(source: &str) -> CodeGraph {
    try_graph(source).unwrap()
}

/// 有效声明与语法失败均走相同原生加载路径，不使用宿主 parser。
fn try_graph(
    source: &str,
) -> Result<CodeGraph, crate::structure_review::code_graph::AnalysisError> {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("lib.rs"), source).unwrap();
    std::fs::write(
        root.path().join("rust-project.json"),
        r#"{"crates":[{"root_module":"lib.rs","edition":"2024","deps":[],"cfg":[]}]}"#,
    )
    .unwrap();
    super::rust_graph::analyzer().analyze(&AnalysisInput {
        repository_root: root.path().into(),
        files: vec!["lib.rs".into()],
        options: Default::default(),
    })
}

/// 原文保留属性、中文注释、CRLF、首行缩进和子声明，规范化比较仍只关注自身语义。
#[test]
#[ignore = "需要真实 Rust 优化 helper"]
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
    let attributed = graph(&source.replace("#[inline]", "#[cold]"));
    assert_eq!(
        before.diff(&attributed).unwrap().nodes.len(),
        1,
        "真正属性仍参与自身内容比较"
    );
    let difference = before.diff(&after).unwrap();
    assert!(
        difference.nodes.is_empty(),
        "{difference:#?}\nbefore={before:#?}\nafter={after:#?}"
    );
}

/// 同行多个声明与多字节标识符按字节范围准确截取，不拼接签名、等号或实现。
#[test]
#[ignore = "需要真实 Rust 优化 helper"]
fn source_ranges_keep_complete_declarations_without_neighboring_items() {
    let source = "fn 甲() {} const 值: u8 = 7; struct S { pub 字段: u8, other: u8 }";
    let graph = graph(source);
    let nodes: Vec<_> = graph
        .root()
        .contains
        .iter()
        .map(|id| graph.node(id).unwrap())
        .collect();
    assert_eq!(
        nodes
            .iter()
            .find(|node| node.data().name.as_deref() == Some("甲"))
            .unwrap()
            .data()
            .source
            .as_deref(),
        Some("fn 甲() {}")
    );
    assert_eq!(
        nodes
            .iter()
            .find(|node| node.data().name.as_deref() == Some("值"))
            .unwrap()
            .data()
            .source
            .as_deref(),
        Some("const 值: u8 = 7;")
    );
    let structure = nodes
        .iter()
        .find(|node| node.data().name.as_deref() == Some("S"))
        .unwrap();
    let field = structure
        .data()
        .contains
        .iter()
        .map(|id| graph.node(id).unwrap())
        .find(|node| node.data().name.as_deref() == Some("字段"))
        .unwrap();
    assert_eq!(field.data().source.as_deref(), Some("pub 字段: u8"));
}

/// 具名子声明独立比较，闭包内容变化归属于外层函数。
#[test]
#[ignore = "需要真实 Rust 优化 helper"]
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
#[ignore = "需要真实 Rust 优化 helper"]
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
#[ignore = "需要真实 Rust 优化 helper"]
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

/// 真实闭包关系仍归属最近的具名声明，未建模的闭包不拥有节点。
#[test]
#[ignore = "需要真实 Rust 优化 helper"]
fn closure_references_belong_to_the_nearest_modeled_declaration() {
    let graph = graph("fn run() { let f = || { fn helper() { inside(); } outside(); }; }");
    let nodes = super::rust_graph::nodes(&graph);
    for (name, owner) in [("inside", "helper"), ("outside", "run")] {
        let node = nodes
            .values()
            .find(|node| node.data().name.as_deref() == Some(owner))
            .unwrap();
        assert!(node.data().references.iter().any(|reference| matches!(&reference.target,ReferenceTarget::Unresolved { name: actual } if actual==name)));
    }
}

/// 字段列表中的逗号属于成员布局，新增字段只改变父节点的 contains。
#[test]
#[ignore = "需要真实 Rust 优化 helper"]
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
#[ignore = "需要真实 Rust 优化 helper"]
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
#[ignore = "需要真实 Rust 优化 helper"]
fn normalization_preserves_literal_contents() {
    let old = graph("fn f() -> &'static str { r#\"a // b\"# }");
    let formatted = graph("// 😀\nfn /* 中 */ f ( ) -> & 'static str {\n r#\"a // b\"#\n }");
    assert!(old.diff(&formatted).unwrap().nodes.is_empty());
    let changed = graph("fn f() -> &'static str { r#\"a  // b\"# }");
    assert_eq!(old.diff(&changed).unwrap().nodes.len(), 1);
}

/// 声明成员的顺序不影响父代码，闭包的使用顺序则属于父代码。
#[test]
#[ignore = "需要真实 Rust 优化 helper"]
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
#[ignore = "需要真实 Rust 优化 helper"]
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
#[ignore = "需要真实 Rust 优化 helper"]
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
#[ignore = "需要真实 Rust 优化 helper"]
fn preserves_cfg_declarations_and_rejects_parse_failures() {
    assert_eq!(
        graph("#[cfg(any())] fn inactive() {} fn active() {}")
            .root()
            .contains
            .len(),
        2
    );
    assert!(try_graph("fn broken( {").is_err());
}

/// 注释中的 CR、LS、PS 不截断后续声明，公共 Host 仍验证真实 UTF-8 源码锚点。
#[test]
#[ignore = "需要真实 Rust 优化 helper"]
fn source_line_breaks_keep_complete_declarations() {
    let source = "/* 中😀\r\u{2028}\u{2029} */ fn after() {}\r\nfn next() {}";
    let graph = graph(source);
    let nodes = super::rust_graph::nodes(&graph);
    for (name, excerpt) in [("after", "fn after() {}"), ("next", "fn next() {}")] {
        let declaration = nodes
            .values()
            .find(|node| node.data().name.as_deref() == Some(name))
            .unwrap();
        assert!(declaration
            .data()
            .source
            .as_deref()
            .unwrap()
            .ends_with(excerpt));
    }
}

/// 多字节名称、代理对与 CRLF 保留独立完整声明，排版调整不会影响公共图 diff。
#[test]
#[ignore = "需要真实 Rust 优化 helper"]
fn unicode_and_crlf_keep_precise_source_and_stable_identity() {
    let source = "// 中文😀\r\nconst 提示: &str = \"😀\"; fn 运行() {}\r\n";
    let graph = graph(source);
    let nodes = super::rust_graph::nodes(&graph);
    for (name, excerpt) in [
        ("提示", "const 提示: &str = \"😀\";"),
        ("运行", "fn 运行() {}"),
    ] {
        let declaration = nodes
            .values()
            .find(|node| node.data().name.as_deref() == Some(name))
            .unwrap();
        assert!(declaration
            .data()
            .source
            .as_deref()
            .unwrap()
            .ends_with(excerpt));
    }
    assert!(graph
        .diff(&self::graph(&source.replace("\r\n", "\n")))
        .unwrap()
        .nodes
        .is_empty());
}
