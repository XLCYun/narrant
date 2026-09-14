//! 文档契约的聚焦回归：公共图验证与 diff 必须同样接受语法结果。
use super::*;
use crate::structure_review::code_graph::{CodeGraph, CodeRoot};
use std::path::Path;

/// 使用内存文件稳定构造文档，无需语言服务或真实业务文件。
fn parse(path: &str, source: &str) -> Document {
    Document::parse(
        Path::new("/repo"),
        Path::new("/repo").join(path),
        source.into(),
        AnalysisOptions::default(),
    )
    .unwrap()
}

/// 查找给定具名对象，调用点明确区分同名类型和重载。
fn named<'a>(document: &'a Document, name: &str) -> &'a Declaration {
    document
        .declarations
        .iter()
        .find(|d| d.node.data().name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("missing {name}: {:?}", document.declarations))
}

/// 通过公开图入口验证唯一身份、包含关系和可达性。
fn graph(documents: &[&Document]) -> CodeGraph {
    CodeGraph::new(
        CodeRoot {
            contains: documents
                .iter()
                .flat_map(|d| d.declarations.iter())
                .filter(|d| d.parent.is_none())
                .map(|d| d.node.data().id.clone())
                .collect(),
        },
        documents
            .iter()
            .flat_map(|d| d.declarations.iter().map(|d| d.node.clone()))
            .collect(),
    )
    .unwrap()
}

/// 全部扩展名和声明文件共享解析器，JS 文件也接受 JSX。
#[test]
fn supports_file_family_and_declarations() {
    for extension in ["ts", "tsx", "js", "jsx", "mts", "cts", "mjs", "cjs"] {
        let document = parse(&format!("a.{extension}"), "function f() { return 1; }");
        assert_eq!(document.declarations.len(), 1);
        graph(&[&document]);
    }
    for path in ["a.d.ts", "a.d.mts", "a.d.cts"] {
        let document = parse(
            path,
            "declare namespace N { interface I { p: string; } function f(): void; }",
        );
        assert_eq!(document.declarations.len(), 4);
        graph(&[&document]);
    }
    assert!(
        parse("a.js", "const view = () => <div/>;")
            .declarations
            .len()
            == 1
    );
}

/// 只有直接 const 标识符箭头被提升；命名的内部声明仍独立建模。
#[test]
fn direct_const_arrows_and_named_callback_declarations() {
    let document = parse("a.ts", "const direct = ((x: number) => x + 1) satisfies Fn; const asserted = (((x: number) => x) as Fn)!; let mutable = () => 0; var legacy = () => 0; const wrapped = memo(() => 1); const conditional = flag ? (() => 1) : (() => 2); const { a } = { a: () => 1 }; function outer() { run(() => { function inner() { return 1; } inner(); }); }");
    for name in ["direct", "asserted"] {
        assert_eq!(
            named(&document, name).node.category(),
            CodeCategory::Callable
        );
    }
    for name in ["mutable", "legacy", "wrapped", "conditional"] {
        assert_eq!(named(&document, name).node.category(), CodeCategory::Value);
    }
    assert_eq!(
        named(&document, "inner").parent.as_ref(),
        Some(&named(&document, "outer").node.data().id)
    );
    assert_eq!(
        document
            .declarations
            .iter()
            .filter(|d| d.node.data().name.as_deref() == Some("direct"))
            .count(),
        1
    );
    assert_eq!(
        document
            .occurrences
            .iter()
            .find(|o| o.kind == OccurrenceKind::Call && o.text == "inner")
            .unwrap()
            .owner,
        named(&document, "outer").node.data().id
    );
    graph(&[&document]);
}

/// 类成员与命名嵌套声明各有自己的内容，父内容不会重复嵌入成员。
#[test]
fn own_content_excludes_children_but_keeps_callbacks() {
    let before = parse("a.ts", "class C { p: T = make(); constructor() {} get value() { return 1; } set value(v: number) {} m() { function nested() { return 1; } run(() => 1); } static { run(); } }");
    let after = parse("a.ts", "class C { p: T = make(); constructor() {} get value() { return 2; } set value(v: number) {} m() { function nested() { return 2; } run(() => 1); } static { run(); } }");
    let diff = graph(&[&before]).diff(&graph(&[&after])).unwrap();
    assert_eq!(diff.nodes.len(), 2, "{diff:?}");
    assert!(diff
        .nodes
        .iter()
        .all(|d| d.id != named(&before, "C").node.data().id
            && d.id != named(&before, "m").node.data().id));
    let callback = parse(
        "a.ts",
        &before.source.replace("run(() => 1)", "run(() => 2)"),
    );
    let diff = graph(&[&before]).diff(&graph(&[&callback])).unwrap();
    assert_eq!(diff.nodes.len(), 1);
    assert_eq!(diff.nodes[0].id, named(&before, "m").node.data().id);
}

/// 同词法所有者的重载保留顺序和每个定义名，跨文件合并声明不会丢失。
#[test]
fn overloads_and_merged_declarations_keep_source_identity() {
    let a = parse("a.ts", "export function f(x: string): string; export function f(x: number): number; export function f(x: any) { return x; } interface I { a: string; } namespace N { export const value = 1; } interface I { c: string; }");
    let b = parse(
        "b.ts",
        "interface I { b: number; } namespace N { export const value = 2; }",
    );
    let f = named(&a, "f");
    assert_eq!(f.selections.len(), 3);
    assert_eq!(f.parts.len(), 3);
    assert!(f.node.data().code.as_ref().unwrap().contains("return x"));
    for selection in &f.selections {
        assert_eq!(
            a.declared_at(selection.start).unwrap().node.data().id,
            f.node.data().id
        );
    }
    assert_eq!(
        a.declarations
            .iter()
            .filter(|d| d.node.data().name.as_deref() == Some("I"))
            .count(),
        2
    );
    assert_ne!(named(&a, "I").node.data().id, named(&b, "I").node.data().id);
    graph(&[&a, &b]);
}

/// 普通注释与格式不产生内容差异，原始 JSDoc 仍可展示和供服务推断。
#[test]
fn formatting_and_comments_do_not_change_diff() {
    let before = parse("a.tsx", "/** original docs */\nexport const f = (x: number) => { // ordinary\n return /a b/g.test(`hello ${x}`) ? <Widget text=\"x\"/> : null; }; ");
    let after = parse("a.tsx", "/** changed docs */\nexport const f=(x:number)=>{return /a b/g.test(`hello ${x}`)?<Widget text=\"x\" />:null;};");
    assert!(graph(&[&before])
        .diff(&graph(&[&after]))
        .unwrap()
        .nodes
        .is_empty());
    assert!(before.declarations.iter().any(|d| d
        .node
        .data()
        .source
        .as_ref()
        .unwrap()
        .contains("original docs")));
}

/// 规范化必须保留 ASI、正则、模板以及 JSX 文本的实际语义。
#[test]
fn meaningful_javascript_text_survives_normalization() {
    for (before, after) in [
        (
            "function f() { return\n{ x: 1 }; }",
            "function f() { return { x: 1 }; }",
        ),
        ("const x = `a b`;", "const x = `a  b`;"),
        ("const x = /a b/;", "const x = /a  b/;"),
        ("const x = <div>a b</div>;", "const x = <div>a  b</div>;"),
        ("const x = tag`a\\nb`;", "const x = tag`a\nb`;"),
    ] {
        let before = parse("a.tsx", before);
        let after = parse("a.tsx", after);
        assert!(
            !graph(&[&before])
                .diff(&graph(&[&after]))
                .unwrap()
                .nodes
                .is_empty(),
            "{}",
            before.source
        );
    }
}

/// intrinsic 标签没有组件关联；属性和 JSX 子表达式照常产生函数候选。
#[test]
fn jsx_components_and_expression_occurrences() {
    let document = parse("a.tsx", "function View() { return <div onClick={() => run()}><UI.Button value={format()} /><Widget /></div>; }");
    let components: Vec<_> = document
        .occurrences
        .iter()
        .filter(|o| o.kind == OccurrenceKind::JsxComponent)
        .map(|o| o.text.as_str())
        .collect();
    assert_eq!(components, ["Button", "Widget"]);
    assert_eq!(
        document
            .occurrences
            .iter()
            .filter(|o| o.kind == OccurrenceKind::Call)
            .count(),
        2
    );
    assert!(document
        .occurrences
        .iter()
        .all(|o| o.owner == named(&document, "View").node.data().id));
}

/// 引用保留缺少调用层级的语法证据，并明确区分动态调用、别名和类型位置。
#[test]
fn reference_intents_are_explicit() {
    let document = parse("a.ts", "import type { T } from './types'; import { run as alias } from './run'; export { alias as again }; class C extends Base implements Contract { field: Map<T, Other>; m() { alias(); new C(); obj[key](); factory()(); value = alias; this.field++; accept(alias); } }");
    for kind in [
        OccurrenceKind::Call,
        OccurrenceKind::Construct,
        OccurrenceKind::Write,
        OccurrenceKind::Reference,
        OccurrenceKind::Import,
        OccurrenceKind::Reexport,
        OccurrenceKind::Extends,
        OccurrenceKind::Implements,
        OccurrenceKind::TypeUse,
    ] {
        assert!(
            document.occurrences.iter().any(|o| o.kind == kind),
            "missing {kind:?}"
        );
    }
    assert!(document
        .occurrences
        .iter()
        .any(|o| o.kind == OccurrenceKind::Call && o.dynamic));
    assert!(document
        .occurrences
        .iter()
        .any(|o| o.import_alias && o.type_only));
    assert!(document
        .occurrences
        .iter()
        .filter(|o| o.kind == OccurrenceKind::Call)
        .all(|o| !o.import_alias && !o.type_only));
}

/// 只排除明确测试路径，混合生产文件里的 test 调用不触发排除。
#[test]
fn tests_toggle_and_invalid_inputs() {
    for path in ["a.test.ts", "a.spec.jsx", "__tests__/a.mts"] {
        let excluded = parse(path, "test(() => run());");
        assert!(excluded.excluded);
        assert!(excluded.declarations.is_empty());
        assert!(excluded.occurrences.is_empty());
        let included = Document::parse(
            Path::new("/repo"),
            Path::new("/repo").join(path),
            "test(() => run());".into(),
            AnalysisOptions {
                include_tests: true,
            },
        )
        .unwrap();
        assert!(!included.declarations.is_empty());
    }
    assert!(!parse("production.ts", "test(() => run());")
        .declarations
        .is_empty());
    for path in ["/outside/a.ts", "/repo/../a.ts", "/repo/a.rs", "a.ts"] {
        assert!(Document::parse(
            Path::new("/repo"),
            path.into(),
            "".into(),
            AnalysisOptions::default()
        )
        .is_err());
    }
    assert!(Document::parse(
        Path::new("/repo"),
        "/repo/a.ts".into(),
        "function {".into(),
        AnalysisOptions::default()
    )
    .is_err());
}

/// checkout 根目录与选择其他文件都不会改变已有声明 ID。
#[test]
fn ids_are_stable_across_roots_and_selection() {
    let source = "namespace N { export class C { method() {} } }";
    let a = parse("目录/a.ts", source);
    let b = Document::parse(
        Path::new("/other"),
        "/other/目录/a.ts".into(),
        source.into(),
        AnalysisOptions::default(),
    )
    .unwrap();
    assert_eq!(
        a.declarations
            .iter()
            .map(|d| &d.node.data().id)
            .collect::<Vec<_>>(),
        b.declarations
            .iter()
            .map(|d| &d.node.data().id)
            .collect::<Vec<_>>()
    );
    let extra = parse("extra.ts", "const another = 1;");
    let selected = graph(&[&a, &extra]);
    for d in &a.declarations {
        assert_eq!(selected.node(&d.node.data().id), Some(&d.node));
    }
    assert!(a.uri.contains("%E7%9B%AE"));
}

/// Unicode、CRLF 和非 BMP 的双向坐标必须精确往返并拒绝非法边界。
#[test]
fn checked_utf16_and_crlf_conversion() {
    let source = "中😀x\r\nconst 名 = 1;\n";
    for (offset, line, character) in [
        (0, 0, 0),
        (3, 0, 1),
        (7, 0, 3),
        (8, 0, 4),
        (10, 1, 0),
        (16, 1, 6),
    ] {
        let expected = Position { line, character };
        assert_eq!(text::position(source, offset).unwrap(), expected);
        assert_eq!(text::byte_offset(source, &expected).unwrap(), offset);
    }
    assert!(text::position(source, 4).is_err());
    assert!(text::position(source, 9).is_err());
    assert!(text::byte_offset(
        source,
        &Position {
            line: 0,
            character: 2
        }
    )
    .is_err());
    assert!(text::byte_offset(
        source,
        &Position {
            line: 0,
            character: 5
        }
    )
    .is_err());
    assert!(text::byte_offset(
        source,
        &Position {
            line: 9,
            character: 0
        }
    )
    .is_err());
}

/// 变量语句的 const/逗号包装不将子变量变动混入父函数内容。
#[test]
fn variable_children_and_object_members_have_own_content() {
    let before = parse("a.ts", "function f() { const a = 1, b = 2; } const obj = { field: 1, method() { return 1; }, get value() { return 1; } };");
    let after = parse("a.ts", "function f() { const a = 3, b = 4; } const obj = { field: 2, method() { return 2; }, get value() { return 1; } };");
    let diff = graph(&[&before]).diff(&graph(&[&after])).unwrap();
    assert_eq!(diff.nodes.len(), 4, "{diff:?}");
    assert!(diff
        .nodes
        .iter()
        .all(|d| d.id != named(&before, "f").node.data().id
            && d.id != named(&before, "obj").node.data().id));
    assert_eq!(
        named(&before, "method").node.category(),
        CodeCategory::Callable
    );
}

/// 解构声明的每个绑定能精确定位，解构赋值仅记录真正写入的名字。
#[test]
fn destructuring_names_and_writes_are_precise() {
    let document = parse("a.ts", "const { a, renamed: b = initial } = input; function f() { ({ a, renamed: b = initial } = input); [a, b] = input; }");
    let binding = document
        .declarations
        .iter()
        .find(|d| d.node.data().native_kind == "variable")
        .unwrap();
    assert_eq!(binding.selections.len(), 2);
    assert!(binding.selections.iter().all(|s| document
        .declared_at(s.start)
        .unwrap()
        .node
        .data()
        .id
        == binding.node.data().id));
    assert!(document
        .declared_at(document.source.find("renamed").unwrap() as u32)
        .is_none());
    let writes: Vec<_> = document
        .occurrences
        .iter()
        .filter(|o| o.kind == OccurrenceKind::Write)
        .map(|o| o.text.as_str())
        .collect();
    assert_eq!(writes, ["a", "b", "a", "b"]);
}

/// Oxc 延迟检查的早期语法错误和非法正则也必须报告失败。
#[test]
fn invalid_early_errors_and_regex_are_not_empty_success() {
    for source in [
        "const x;",
        "let x; let x;",
        "function f() { break; }",
        "const regex = /[/;",
    ] {
        assert!(
            Document::parse(
                Path::new("/repo"),
                "/repo/a.ts".into(),
                source.into(),
                AnalysisOptions::default()
            )
            .is_err(),
            "{source}"
        );
    }
}

/// JSX intrinsic 判断对齐固定 TypeScript，包括大写含连字符和 this 单标签。
#[test]
fn jsx_intrinsics_match_typescript_and_member_queries_remain() {
    let document = parse(
        "a.tsx",
        "const view = () => <><X-widget/><button/><this/><UI.Card/><this.Card/><_Card/><组件/></>;",
    );
    let components: Vec<_> = document
        .occurrences
        .iter()
        .filter(|o| o.kind == OccurrenceKind::JsxComponent)
        .map(|o| o.text.as_str())
        .collect();
    assert_eq!(components, ["Card", "Card", "_Card", "组件"]);
}

/// 原始声明展示保留变量关键字、导出修饰和紧邻 JSDoc。
#[test]
fn source_display_keeps_declaration_wrappers_and_jsdoc() {
    let document = parse("a.ts", "/** identity */\nexport const f = (x: number) => x;\n/** helper */\nexport function helper() { return f(1); }");
    assert_eq!(
        named(&document, "f").node.data().source.as_deref(),
        Some("/** identity */\nexport const f = (x: number) => x;")
    );
    assert_eq!(
        named(&document, "helper").node.data().source.as_deref(),
        Some("/** helper */\nexport function helper() { return f(1); }")
    );
}

/// 简写对象属性仍是函数值引用，私有字段与动态计算键不会误用声明名范围。
#[test]
fn shorthand_private_and_computed_names_keep_reference_evidence() {
    let document = parse("a.ts", "function handler() {} const obj = { handler }; class C { #secret = 1; [makeKey()]() { return this.#secret; } }");
    assert!(document
        .occurrences
        .iter()
        .any(|o| o.kind == OccurrenceKind::Reference && o.text == "handler"));
    assert_eq!(named(&document, "#secret").node.data().native_kind, "field");
    let key_offset = document.source.find("makeKey").unwrap() as u32;
    assert!(document.declared_at(key_offset).is_none());
    assert!(document
        .occurrences
        .iter()
        .any(|o| o.kind == OccurrenceKind::Call && o.text == "makeKey"));
}

/// namespace 限定的继承和字段类型必须查询最后一个类型名字。
#[test]
fn qualified_type_occurrences_select_terminal_names() {
    let document = parse(
        "a.ts",
        "interface I extends NS.Base {} class C implements NS.Contract { field: NS.Value; }",
    );
    for (kind, name) in [
        (OccurrenceKind::Extends, "Base"),
        (OccurrenceKind::Implements, "Contract"),
        (OccurrenceKind::TypeUse, "Value"),
    ] {
        assert!(document
            .occurrences
            .iter()
            .any(|o| o.kind == kind && o.text == name));
    }
}

/// 顶层指令与构造参数属性也是源码对象，普通参数仍留在方法签名里。
#[test]
fn directives_and_parameter_properties_are_modeled() {
    let document = parse("a.ts", "'use strict'; class C { constructor(public field: Model, readonly other = make(), ordinary: string) {} }");
    assert!(document
        .declarations
        .iter()
        .any(|d| d.node.data().native_kind == "directive"));
    for name in ["field", "other"] {
        assert_eq!(
            named(&document, name).node.data().native_kind,
            "parameter_property"
        );
    }
    assert!(document
        .declarations
        .iter()
        .all(|d| d.node.data().name.as_deref() != Some("ordinary")));
    assert!(document
        .occurrences
        .iter()
        .any(|o| o.kind == OccurrenceKind::TypeUse && o.text == "Model"));
    graph(&[&document]);
}

/// 复合动态被调表达式中的函数值读取必须保留，不能被外层调用的大范围吞掉。
#[test]
fn compound_callees_preserve_independent_function_value_references() {
    let document = parse("a.ts", "function f() { (() => handler)(); (() => accept(callback))(); factory(argument)(); object[keyHandler](); direct(); }");
    for name in ["handler", "callback", "argument", "keyHandler"] {
        assert!(
            document
                .occurrences
                .iter()
                .any(|o| o.kind == OccurrenceKind::Reference && o.text == name),
            "missing {name}"
        );
    }
    assert!(
        document
            .occurrences
            .iter()
            .filter(|o| o.kind == OccurrenceKind::Call && o.dynamic)
            .count()
            >= 4
    );
    assert!(!document
        .occurrences
        .iter()
        .any(|o| o.kind == OccurrenceKind::Reference && o.text == "direct"));
    assert!(document
        .occurrences
        .iter()
        .all(|o| o.owner == named(&document, "f").node.data().id));
}

/// 标签模板是运行时调用，直接、成员与动态标签保留相同的保守分类和插值遍历。
#[test]
fn tagged_templates_capture_calls_and_substitution_evidence() {
    let document = parse("a.ts", "function f() { tag`hello ${format(valueHandler)}`; object.tag`member ${memberHandler}`; object[keyTag]`computed ${compute()}`; factory(factoryHandler)`dynamic ${dynamicHandler}`; (() => returnedTag)`iife ${substitutionHandler}`; }");
    for name in ["tag", "format", "compute", "factory"] {
        assert!(
            document
                .occurrences
                .iter()
                .any(|o| o.kind == OccurrenceKind::Call && o.text == name && !o.dynamic),
            "missing call {name}"
        );
    }
    assert_eq!(
        document
            .occurrences
            .iter()
            .filter(|o| o.kind == OccurrenceKind::Call && o.text == "tag")
            .count(),
        2
    );
    for name in ["keyTag", "factory(factoryHandler)", "() => returnedTag"] {
        assert!(
            document
                .occurrences
                .iter()
                .any(|o| o.kind == OccurrenceKind::Call && o.text == name && o.dynamic),
            "missing dynamic tag {name}"
        );
    }
    for name in [
        "valueHandler",
        "memberHandler",
        "keyTag",
        "factoryHandler",
        "dynamicHandler",
        "returnedTag",
        "substitutionHandler",
    ] {
        assert!(
            document
                .occurrences
                .iter()
                .any(|o| o.kind == OccurrenceKind::Reference && o.text == name),
            "missing reference {name}"
        );
    }
    assert!(document
        .occurrences
        .iter()
        .all(|o| o.owner == named(&document, "f").node.data().id));
}

/// CR、CRLF、LF 和两个 Unicode 分隔符全部按固定语言服务规则往返转换。
#[test]
fn native_line_boundaries_round_trip_and_reject_invalid_positions() {
    for separator in ["\r", "\r\n", "\n", "\u{2028}", "\u{2029}"] {
        let source = format!("中😀x{separator}尾{separator}");
        let second_start = 8 + separator.len();
        for (offset, line, character) in [
            (0, 0, 0),
            (3, 0, 1),
            (7, 0, 3),
            (8, 0, 4),
            (second_start, 1, 0),
            (second_start + 3, 1, 1),
            (source.len(), 2, 0),
        ] {
            let expected = Position { line, character };
            assert_eq!(
                text::position(&source, offset as u32).unwrap(),
                expected,
                "{separator:?}"
            );
            assert_eq!(
                text::byte_offset(&source, &expected).unwrap(),
                offset as u32,
                "{separator:?}"
            );
        }
        for offset in [1, 4, source.len() + 1] {
            assert!(text::position(&source, offset as u32).is_err());
        }
        for position in [
            Position {
                line: 0,
                character: 2,
            },
            Position {
                line: 0,
                character: 5,
            },
            Position {
                line: 1,
                character: 2,
            },
            Position {
                line: 2,
                character: 1,
            },
            Position {
                line: 3,
                character: 0,
            },
        ] {
            assert!(
                text::byte_offset(&source, &position).is_err(),
                "{separator:?}: {position:?}"
            );
        }
        for offset in 9..8 + separator.len() {
            assert!(text::position(&source, offset as u32).is_err());
        }
        let document = parse(
            "a.ts",
            &format!("function f() {{{separator}tag`hello`;{separator}}}"),
        );
        assert_eq!(
            document
                .occurrences
                .iter()
                .find(|o| o.kind == OccurrenceKind::Call)
                .unwrap()
                .position,
            Position {
                line: 1,
                character: 0
            }
        );
    }
    // U+0085 是一个普通 UTF-16 单元，不是固定 TypeScript 接受的换行字符。
    let source = "a\u{85}b\r\nc\rd\ne\u{2028}f\u{2029}";
    assert_eq!(
        text::position(source, 3).unwrap(),
        Position {
            line: 0,
            character: 2
        }
    );
    assert_eq!(
        text::byte_offset(
            source,
            &Position {
                line: 0,
                character: 2
            }
        )
        .unwrap(),
        3
    );
    assert_eq!(
        text::position(source, source.len() as u32).unwrap(),
        Position {
            line: 5,
            character: 0
        }
    );
    assert_eq!(
        text::byte_offset(
            source,
            &Position {
                line: 5,
                character: 0
            }
        )
        .unwrap(),
        source.len() as u32
    );
}

/// 独立子对象增删只改变父包含关系，不得产生空容器与带空格容器的内容差异。
#[test]
fn first_child_insertion_and_last_child_removal_preserve_owner_content() {
    for (before, after, owner) in [
        ("class C {}", "class C { method() {} }", "C"),
        ("interface I {}", "interface I { field: string; }", "I"),
        ("namespace N {}", "namespace N { function child() {} }", "N"),
        (
            "function f() {}",
            "function f() { function child() {} }",
            "f",
        ),
        ("const value = {};", "const value = { field: 1 };", "value"),
    ] {
        let before = parse("a.ts", before);
        let after = parse("a.ts", after);
        for (before, after) in [(&before, &after), (&after, &before)] {
            let id = &named(before, owner).node.data().id;
            let diff = graph(&[before]).diff(&graph(&[after])).unwrap();
            let owner_change = diff.nodes.iter().find(|d| &d.id == id).unwrap();
            assert!(
                owner_change.content.is_none(),
                "{owner}: {:?} -> {:?}: {diff:?}",
                named(before, owner).node.data(),
                named(after, owner).node.data()
            );
            assert!(!owner_change.contains.is_empty());
        }
    }
}

/// 子声明在剩余语句两侧增删和换序不能改变剩余语句的排版投影。
#[test]
fn child_reordering_and_neighboring_statements_preserve_owner_content() {
    let base = parse("a.ts", "function f() { before(); after(); }");
    let one = parse(
        "a.ts",
        "function f() { before(); function a() {} after(); function b() {} }",
    );
    let reordered = parse(
        "a.ts",
        "function f() { function b() {} before(); function a() {} after(); }",
    );
    let id = &named(&base, "f").node.data().id;
    for document in [&one, &reordered] {
        let diff = graph(&[&base]).diff(&graph(&[document])).unwrap();
        assert!(
            diff.nodes
                .iter()
                .find(|d| &d.id == id)
                .unwrap()
                .content
                .is_none(),
            "{diff:?}"
        );
    }
    assert!(graph(&[&one])
        .diff(&graph(&[&reordered]))
        .unwrap()
        .nodes
        .is_empty());
}

/// 对象属性间的逗号属于成员列表，删去子属性后不能污染剩余 spread 初始化内容。
#[test]
fn object_children_preserve_surviving_spreads_and_list_punctuation() {
    let before = parse("a.ts", "const value = { ...left, ...right };");
    let after = parse(
        "a.ts",
        "const value = { first: 1, ...left, middle() {}, ...right, last: 3 };",
    );
    let reordered = parse(
        "a.ts",
        "const value = { last: 3, first: 1, ...left, ...right, middle() {} };",
    );
    let id = &named(&before, "value").node.data().id;
    for document in [&after, &reordered] {
        let diff = graph(&[&before]).diff(&graph(&[document])).unwrap();
        assert!(
            diff.nodes
                .iter()
                .find(|d| &d.id == id)
                .unwrap()
                .content
                .is_none(),
            "{diff:?}"
        );
    }
    assert!(graph(&[&after])
        .diff(&graph(&[&reordered]))
        .unwrap()
        .nodes
        .is_empty());
}

/// 相同分隔符规则覆盖枚举、参数属性和作为参数的具名声明，保留普通列表成员。
#[test]
fn child_list_separators_preserve_nonchild_values() {
    for (before, after, owner) in [
        ("enum E {}", "enum E { A = 1, B = 2 }", "E"),
        ("class C { constructor(ordinary: string) {} }", "class C { constructor(public field: Type, ordinary: string, readonly other = 0) {} }", "constructor"),
        ("const value = factory(before, after);", "const value = factory(function first() {}, before, function middle() {}, after, function last() {});", "value"),
    ] {
        let before = parse("a.ts", before);
        let after = parse("a.ts", after);
        let id = &named(&before, owner).node.data().id;
        for (before, after) in [(&before, &after), (&after, &before)] {
            let diff = graph(&[before]).diff(&graph(&[after])).unwrap();
            assert!(diff.nodes.iter().find(|d| &d.id == id).unwrap().content.is_none(), "{owner}: {diff:?}");
        }
    }
}

/// 列表清理不能擦掉数组空位、逗号表达式或字面量中的有意义内容。
#[test]
fn own_content_keeps_semantic_punctuation_and_literal_text() {
    let before = parse("a.ts", "const value = { ...build((a, b), c), ...right };");
    let with_child = parse(
        "a.ts",
        "const value = { child: 1, ...build((a, b), c), ...right, last: 2 };",
    );
    let id = &named(&before, "value").node.data().id;
    let diff = graph(&[&before]).diff(&graph(&[&with_child])).unwrap();
    assert!(diff
        .nodes
        .iter()
        .find(|d| &d.id == id)
        .unwrap()
        .content
        .is_none());
    for (before, after) in [
        ("const value = [];", "const value = [,];"),
        ("const value = [a];", "const value = [a,,];"),
        (
            "const value = build((a, b), c);",
            "const value = build(a, b, c);",
        ),
        ("const value = /a,b/;", "const value = /ab/;"),
        ("const value = tag`a, b`;", "const value = tag`a,b`;"),
        (
            "const value = <div>a, b</div>;",
            "const value = <div>a,b</div>;",
        ),
    ] {
        let before = parse("a.tsx", before);
        let after = parse("a.tsx", after);
        assert!(
            !graph(&[&before])
                .diff(&graph(&[&after]))
                .unwrap()
                .nodes
                .is_empty(),
            "{}",
            before.source
        );
    }
}
