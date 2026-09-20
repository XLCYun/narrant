//! 通过文档与图 diff 验证成员裁剪，表达式顺序及原始源码不受比较投影影响。
use super::*;

/// 声明主体只扣除独立成员，调用签名、未建模的属性和变量仍完整参与比较。
#[test]
fn declaration_lists_preserve_unmodeled_members() {
    for (owner, before, after) in [
        (
            "C",
            "class C { accessor kept = 1; }",
            "class C { first() {} accessor kept = 1; last = 2; }",
        ),
        (
            "I",
            "interface I { (): string; new(): I; }",
            "interface I { a: string; (): string; method(): void; new(): I; [name: string]: unknown; }",
        ),
        (
            "T",
            "type T = ({ (): string } | { new(): T }) & Base",
            "type T = ({ field: string; (): string } | { new(): T; method(): void }) & Base",
        ),
        (
            "N",
            "namespace N { const kept = 1; }",
            "namespace N { export const first = () => 1; const kept = 1; export function last() {} }",
        ),
        (
            "external",
            "declare module 'external' {}",
            "declare module 'external' { export interface I { a: number } export function f(): void; }",
        ),
        (
            "global",
            "export {}; declare global {}",
            "export {}; declare global { interface I { a: number } }",
        ),
        (
            "N",
            "namespace N {}",
            "namespace N.Inner { export function f() {} }",
        ),
    ] {
        let before = parse("a.ts", before);
        let after = parse("a.ts", after);
        for (before, after) in [(&before, &after), (&after, &before)] {
            let diff = graph(&[before]).diff(&graph(&[after])).unwrap();
            let owner_id = &named(before, owner).node.data().id;
            let change = diff.nodes.iter().find(|d| &d.id == owner_id).unwrap();
            assert!(change.content.is_none(), "{owner}: {diff:?}");
            assert!(!change.contains.is_empty(), "{owner}: {diff:?}");
        }
    }
}

/// 一个 const/export 语句里删除独立函数绑定，不能把其余普通值一并扣掉。
#[test]
fn mixed_variable_statements_keep_noncallable_bindings() {
    for export in ["", "export "] {
        let before = parse(
            "a.ts",
            &format!("namespace N {{ {export}const kept = 1; }}"),
        );
        let after = parse("a.ts", &format!("namespace N {{ {export}const first = () => 1, kept = 1, last = function() {{}}; }}"));
        let id = &named(&before, "N").node.data().id;
        let diff = graph(&[&before]).diff(&graph(&[&after])).unwrap();
        assert!(
            diff.nodes
                .iter()
                .find(|d| &d.id == id)
                .unwrap()
                .content
                .is_none(),
            "{diff:?}"
        );

        let changed = parse("a.ts", &after.source.replace("kept = 1", "kept = 2"));
        let diff = graph(&[&after]).diff(&graph(&[&changed])).unwrap();
        assert_eq!(diff.nodes.len(), 1, "{diff:?}");
        assert_eq!(&diff.nodes[0].id, id);
        assert!(diff.nodes[0].content.is_some());
    }
}

/// 已排除的成员之外，容器头部、匿名签名和未建模成员的变化仍要被识别。
#[test]
fn retained_declaration_content_still_changes() {
    for (owner, before, after) in [
        (
            "C",
            "class C extends Before { field = 1; }",
            "class C extends After { field = 1; }",
        ),
        (
            "C",
            "class C { accessor kept = 1; }",
            "class C { accessor kept = 2; }",
        ),
        (
            "I",
            "interface I { (): string; field: number }",
            "interface I { (): number; field: number }",
        ),
        (
            "T",
            "type T = { field: number } & Before",
            "type T = { field: number } & After",
        ),
        (
            "T",
            "type T = Array<{ field: string }>",
            "type T = Array<{ field: number }>",
        ),
        (
            "N",
            "namespace N { const object = { run() { return 1; } }; }",
            "namespace N { const object = { run() { return 2; } }; }",
        ),
    ] {
        let before = parse("a.ts", before);
        let after = parse("a.ts", after);
        let diff = graph(&[&before]).diff(&graph(&[&after])).unwrap();
        let id = &named(&before, owner).node.data().id;
        assert!(
            diff.nodes
                .iter()
                .find(|d| &d.id == id)
                .unwrap()
                .content
                .is_some(),
            "{owner}: {diff:?}"
        );
    }
}

/// 同样存在独立子对象时，表达式及函数实现完整比较，声明容器才排除成员。
#[test]
fn embedded_declaration_edits_remain_in_complete_expressions() {
    for (owner, source) in [
        (
            "value",
            "class C { value = [class Inner { field = 1; }, fallback]; }",
        ),
        (
            "C",
            "class C extends factory(class Inner { field = 1; }) {}",
        ),
        ("f", "function f(value = class Inner { field = 1; }) {}"),
        (
            "f",
            "function f() { class Inner { field = 1; } return Inner; }",
        ),
        (
            "Item",
            "enum E { Item = factory(class Inner { field = 1; }) }",
        ),
    ] {
        let before = parse("a.ts", source);
        let after = parse("a.ts", &source.replace("field = 1", "field = 2"));
        let diff = graph(&[&before]).diff(&graph(&[&after])).unwrap();
        for name in [owner, "field"] {
            let id = &named(&before, name).node.data().id;
            assert!(
                diff.nodes
                    .iter()
                    .find(|d| &d.id == id)
                    .unwrap()
                    .content
                    .is_some(),
                "{source}: {diff:?}"
            );
        }
        assert!(
            !diff
                .nodes
                .iter()
                .any(|d| d.id == named(&before, "Inner").node.data().id),
            "{diff:?}"
        );
    }
}

/// 纯格式及普通尾逗号由 Oxc 统一，不再需要手工识别数组、对象和参数列表。
#[test]
fn full_expressions_ignore_formatting_and_optional_trailing_commas() {
    for (before, after) in [
        ("[keep]", "[keep,]"),
        ("{ key: 1 }", "{ key: 1, }"),
        ("factory(a)", "factory(a,)"),
        ("new Model(a)", "new Model(a,)"),
        ("{ method(a) {} }", "{ method(a,) {} }"),
    ] {
        let before = parse("a.ts", &format!("class C {{ value = {before}; }}"));
        let after = parse("a.ts", &format!("class C{{/* comment */ value={after};}}"));
        let diff = graph(&[&before]).diff(&graph(&[&after])).unwrap();
        assert!(
            diff.nodes.is_empty(),
            "{} -> {}: {diff:?}",
            before.source,
            after.source
        );
    }
}

/// 裁剪只影响比较签名，原始文档、展示源码和 LSP 定位仍包含完整成员。
#[test]
fn projected_containers_keep_original_source_and_positions() {
    let source = "/** docs */\nexport class C {\n  value = [class Inner { field = 1; }, fallback];\n  method() { return this.value; }\n}";
    let document = parse("a.ts", source);
    let class = named(&document, "C");
    assert_eq!(class.node.data().source.as_deref(), Some(source));
    assert_eq!(class.node.data().contains.len(), 2);
    assert!(!class
        .node
        .data()
        .signature
        .as_deref()
        .unwrap()
        .contains("value"));
    assert!(named(&document, "value")
        .node
        .data()
        .code
        .as_deref()
        .unwrap()
        .contains("field = 1"));
    for name in ["value", "Inner", "field", "method"] {
        let offset = source.find(name).unwrap() as u32;
        assert_eq!(
            document.declared_at(offset).unwrap().node.data().id,
            named(&document, name).node.data().id
        );
    }
    assert_eq!(&*document.source, source);
}
