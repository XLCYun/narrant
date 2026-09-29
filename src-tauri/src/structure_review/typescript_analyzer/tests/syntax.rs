//! 文档契约的聚焦回归：公共图验证与 diff 必须同样接受语法结果。
use super::*;
use crate::structure_review::typescript_analyzer::LINE_BREAKS;
use crate::structure_review::{
    code_graph::{CodeGraph, CodeRoot},
    source_text::SourceText,
};
use std::path::Path;

#[path = "syntax/content.rs"]
mod content;
#[path = "syntax/parameter_properties.rs"]
mod parameter_properties;

/// 使用内存文件稳定构造文档，无需语言服务或真实业务文件。
fn parse(path: &str, source: &str) -> Document {
    Document::parse(
        Path::new("/repo"),
        Path::new("/repo").join(path),
        SourceText::new(source, LINE_BREAKS),
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

/// 每个文件提供独立文件容器，子对象源码变化不能扩散成文件自身的内容变化。
#[test]
fn file_modules_contain_declarations_and_stay_stable_on_child_edits() {
    let source =
        "export class Model { value: number; } export function load() { return 1; } load();";
    let before = parse("src/model.ts", source);
    let after = parse("src/model.ts", &source.replace("return 1", "return 2"));
    let other = parse("scripts/model.js", "export function load() {}");
    let module = named(&before, "src/model.ts");
    assert_eq!(module.node.category(), CodeCategory::Container);
    assert_eq!(module.node.data().native_kind, "file");
    assert_eq!(module.node.data().source.as_deref(), Some(source));
    for name in ["Model", "load", "顶层执行"] {
        let declaration = named(&before, name);
        assert_eq!(declaration.parent.as_ref(), Some(&module.node.data().id));
        assert!(module
            .node
            .data()
            .contains
            .contains(&declaration.node.data().id));
    }
    let combined = graph(&[&before, &other]);
    assert_eq!(combined.root().contains.len(), 2);
    assert!(combined
        .root()
        .contains
        .iter()
        .all(|id| combined.node(id).unwrap().category() == CodeCategory::Container));
    let diff = graph(&[&before]).diff(&graph(&[&after])).unwrap();
    assert_eq!(diff.nodes.len(), 1, "{diff:?}");
    assert_eq!(diff.nodes[0].id, named(&before, "load").node.data().id);
}

/// 普通脚本和声明文件都使用 file 容器；显式 namespace/module 保持语言种类。
#[test]
fn file_containers_do_not_claim_language_module_semantics() {
    for (path, source) in [
        ("scripts/start.js", "function start() {} start();"),
        (
            "types/model.d.ts",
            "declare namespace N { interface Shape {} } declare module 'remote' { export interface Other {} }",
        ),
    ] {
        let document = parse(path, source);
        let file = named(&document, path);
        assert_eq!(file.node.data().native_kind, "file");
        assert!(file.parent.is_none());
        assert!(file.node.data().code.is_none());
        assert!(file.node.data().signature.is_none());
        for declaration in document.declarations.iter().filter(|d| {
            d.node.category() == CodeCategory::Container && d.node.data().id != file.node.data().id
        }) {
            assert!(matches!(
                declaration.node.data().native_kind.as_str(),
                "module" | "namespace"
            ));
            assert_eq!(declaration.parent.as_ref(), Some(&file.node.data().id));
        }
    }
}

/// 初始化、普通语句与回调合并为所在模块的一个区域，namespace 与函数保留各自归属。
#[test]
fn top_level_statements_and_initializers_share_scope_execution() {
    let document = parse(
        "a.ts",
        r#"
        'use strict';
        function target() {} function outer() { if (enabled) target(); }
        const a = target(); const b = target();
        target(); if (enabled) target();
        [1].forEach(() => { target(); });
        namespace N { const c = target(); target(); export function inner() { target(); } }
        class C { static { target(); } }
    "#,
    );
    let execution = document
        .declarations
        .iter()
        .filter(|d| d.node.data().native_kind == "top_level")
        .collect::<Vec<_>>();
    assert_eq!(
        execution.len(),
        2,
        "one file region and one namespace region"
    );
    let file = named(&document, "a.ts");
    let namespace = named(&document, "N");
    for (parent, count) in [(file, 5), (namespace, 2)] {
        let region = execution
            .iter()
            .find(|d| d.parent.as_ref() == Some(&parent.node.data().id))
            .unwrap();
        assert_eq!(
            document
                .occurrences
                .iter()
                .filter(|o| o.owner == region.node.data().id
                    && o.kind == OccurrenceKind::Call
                    && o.text == "target")
                .count(),
            count
        );
    }
    assert!(document.declarations.iter().all(|d| !matches!(
        d.node.data().native_kind.as_str(),
        "statement" | "directive"
    )));
    for name in ["outer", "inner"] {
        assert_eq!(
            document
                .occurrences
                .iter()
                .filter(|o| o.owner == named(&document, name).node.data().id
                    && o.kind == OccurrenceKind::Call)
                .count(),
            1
        );
    }
    assert_eq!(
        document
            .declarations
            .iter()
            .filter(|d| d.node.data().native_kind == "static_block")
            .count(),
        1
    );
    graph(&[&document]);
}

/// 全部扩展名和声明文件共享解析器，JS 文件也接受 JSX。
#[test]
fn supports_file_family_and_declarations() {
    for extension in ["ts", "tsx", "js", "jsx", "mts", "cts", "mjs", "cjs"] {
        let document = parse(&format!("a.{extension}"), "function f() { return 1; }");
        assert_eq!(document.declarations.len(), 2);
        graph(&[&document]);
    }
    for path in ["a.d.ts", "a.d.mts", "a.d.cts"] {
        let document = parse(
            path,
            "declare namespace N { interface I { p: string; } function f(): void; }",
        );
        assert_eq!(document.declarations.len(), 5);
        graph(&[&document]);
    }
    assert!(
        parse("a.js", "const view = () => <div/>;")
            .declarations
            .len()
            == 2
    );
}

/// 顶层具名函数值统一为 Callable，工厂/条件表达式和函数内声明不提升。
#[test]
fn direct_const_arrows_and_named_callback_declarations() {
    let document = parse("a.ts", "const direct = ((x: number) => x + 1) satisfies Fn; const asserted = (((x: number) => x) as Fn)!; let mutable = () => 0; var legacy = () => 0; const wrapped = memo(() => 1); const conditional = flag ? (() => 1) : (() => 2); const { a } = { a: () => 1 }; function outer() { run(() => { function inner() { return 1; } inner(); }); }");
    for name in ["direct", "asserted", "mutable", "legacy"] {
        assert_eq!(
            named(&document, name).node.category(),
            CodeCategory::Callable
        );
    }
    for name in ["wrapped", "conditional", "inner"] {
        assert!(document
            .declarations
            .iter()
            .all(|d| d.node.data().name.as_deref() != Some(name)));
    }
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

/// 数据节点仅保留类型的字段（含参数属性）、属性、索引签名与枚举成员。
#[test]
fn data_declarations_keep_type_members_only() {
    let document = parse(
        "a.ts",
        r#"
        const plain = 1; let mutable = 2; var legacy = 3;
        const { value: renamed } = input; const [first] = list;
        const object = { plain, property: 1, nested: { leaf: 2 } };
        function outer() { let local = 1; for (const entry of list) { const item = entry; }
            run(() => { const callbackLocal = 2; }); }
        class Model { field: string; static total = 1;
            constructor(public parameter: string) {} accessor automatic = ""; }
        interface Contract { property: Model; [key: string]: Model; }
        type RecordValue = { property: Model; [key: number]: Model };
        enum Choice { One, Two }
    "#,
    );
    let values = document
        .declarations
        .iter()
        .filter(|d| d.node.category() == CodeCategory::Value)
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 9, "{values:?}");
    assert!(values.iter().all(|d| matches!(
        d.node.data().native_kind.as_str(),
        "field" | "static_field" | "property" | "index_signature" | "enum_member"
    )));
    for name in [
        "plain",
        "mutable",
        "legacy",
        "renamed",
        "first",
        "object",
        "nested",
        "leaf",
        "local",
        "entry",
        "item",
        "callbackLocal",
        "automatic",
    ] {
        assert!(
            document
                .declarations
                .iter()
                .all(|d| d.node.data().name.as_deref() != Some(name)),
            "{name}"
        );
    }
    assert_eq!(
        document
            .occurrences
            .iter()
            .filter(|o| o.kind == OccurrenceKind::TypeUse && o.text == "Model")
            .count(),
        4
    );
    graph(&[&document]);
}

/// 命名类型只展开直接成员，嵌套匿名类型的内容保留在所属属性中。
#[test]
fn data_declarations_require_named_type_members() {
    let document = parse(
        "a.ts",
        r#"
        interface Contract {
            field: string;
            run: () => void;
            [key: string]: unknown;
            child: { nested: string; [key: number]: string };
        }
        type RecordValue = { label: string; [key: number]: string };
        type Combined = Contract & ({ active: boolean });
        type Choice = { first: string } | ({ second: number });
        namespace N { export interface Nested { member: string; } }
        "#,
    );
    for (owner, expected) in [
        (
            "Contract",
            vec![Some("field"), Some("run"), None, Some("child")],
        ),
        ("RecordValue", vec![Some("label"), None]),
        ("Combined", vec![Some("active")]),
        ("Choice", vec![Some("first"), Some("second")]),
        ("Nested", vec![Some("member")]),
    ] {
        let owner = named(&document, owner);
        let values = document
            .declarations
            .iter()
            .filter(|d| {
                d.node.category() == CodeCategory::Value
                    && d.parent.as_ref() == Some(&owner.node.data().id)
            })
            .map(|d| d.node.data().name.as_deref())
            .collect::<Vec<_>>();
        assert_eq!(values, expected);
    }
    assert_eq!(
        document
            .declarations
            .iter()
            .filter(|d| d.node.category() == CodeCategory::Value)
            .count(),
        10,
    );
    assert_eq!(named(&document, "run").node.data().native_kind, "property");
    assert!(named(&document, "child")
        .node
        .data()
        .signature
        .as_deref()
        .unwrap()
        .contains("nested"));
    graph(&[&document]);
}

/// 函数内部及非命名类型主体中的匿名属性和索引签名不生成独立数据节点。
#[test]
fn data_declarations_exclude_function_and_anonymous_type_contexts() {
    let document = parse(
        "a.tsx",
        r#"
        const value: { variable: number; [key: string]: number } = { variable: 1 };
        const arrow = (props: { arrowProp: number }): { arrowResult: number } => ({ arrowResult: 1 });
        function render(props: { input: string; [key: string]: string }): { output: string } {
            const [state] = useState<{ stateValue: number; [key: string]: number }>();
            interface LocalInterface { localField: number; [key: string]: number; }
            type LocalAlias = { localAliasField: number; [key: string]: number };
            run(() => { type CallbackLocal = { callbackField: number }; });
            return { output: props.input };
        }
        type Wrapped = Wrapper<{ wrapped: string; [key: string]: string }>;
        type Factory = (props: { argument: number }) => { result: number };
        type ArrayValue = { element: number }[];
        type Generic<T extends { bound: number } = { fallback: number }> = T;
        interface GenericInterface<T extends { interfaceBound: number }> {}
        "#,
    );
    assert!(document
        .declarations
        .iter()
        .all(|d| d.node.category() != CodeCategory::Value));
    // 本次只收窄数据成员；局部命名类型仍保留原有的独立声明身份。
    for name in ["LocalInterface", "LocalAlias", "CallbackLocal"] {
        assert_eq!(named(&document, name).node.category(), CodeCategory::Type);
    }
    graph(&[&document]);
}

/// React 参数与 state 的匿名属性不泄漏为函数成员，类型修改仍计入函数自身差异。
#[test]
fn comparison_view_state_change_does_not_add_data_members() {
    let source = "function ComparisonView({ comparison }: { comparison: StructureComparison }) { return comparison; }";
    let before = parse("view.tsx", source);
    let after = parse(
        "view.tsx",
        &source.replace("return comparison;", "const [layoutResult, setLayoutResult] = useState<{ comparison: StructureComparison; layout: StructureLayout }>(); return comparison;"),
    );
    let changed_type = parse(
        "view.tsx",
        &source.replace("StructureComparison", "OtherComparison"),
    );
    for document in [&before, &after, &changed_type] {
        assert!(named(document, "ComparisonView")
            .node
            .data()
            .contains
            .is_empty());
        assert!(document
            .declarations
            .iter()
            .all(|d| d.node.category() != CodeCategory::Value));
    }
    for document in [&after, &changed_type] {
        let diff = graph(&[&before]).diff(&graph(&[document])).unwrap();
        assert_eq!(diff.nodes.len(), 1, "{diff:?}");
        assert_eq!(
            diff.nodes[0].id,
            named(&before, "ComparisonView").node.data().id
        );
        assert_eq!(
            diff.nodes[0].content,
            Some(crate::structure_review::code_graph::ChangeKind::Modified)
        );
    }
}

/// 各种具名函数值使用外部名字且不重复建模，任何函数或回调内部的函数均被合并。
#[test]
fn named_function_values_exclude_function_interiors() {
    let document = parse(
        "a.ts",
        r#"
        const a = () => {}; const b = function internalName() {};
        let mutable = function() {}; var legacy = () => {};
        namespace N { export const member = () => {}; }
        class C { handler = () => {}; static staticHandler = function() {}; #privateHandler = () => {}; method() {} }
        const object = { arrow: () => {}, expression: function internalProperty() {}, method() {}, get getter() { return 1; } };
        function outer() { function nested() { target(); } const local = () => target();
            const localObject = { hidden() { target(); }, field: function namedLocal() { target(); } };
            run(() => { function callbackInner() { target(); } const callbackValue = () => target(); }); }
    "#,
    );
    for name in [
        "a",
        "b",
        "mutable",
        "legacy",
        "member",
        "handler",
        "staticHandler",
        "#privateHandler",
        "arrow",
        "expression",
        "getter",
        "outer",
    ] {
        assert_eq!(
            named(&document, name).node.category(),
            CodeCategory::Callable,
            "{name}"
        );
    }
    for name in [
        "internalName",
        "internalProperty",
        "nested",
        "local",
        "hidden",
        "namedLocal",
        "callbackInner",
        "callbackValue",
    ] {
        assert!(
            document
                .declarations
                .iter()
                .all(|d| d.node.data().name.as_deref() != Some(name)),
            "{name}"
        );
    }
    let outer = named(&document, "outer");
    assert!(outer.node.data().contains.is_empty());
    assert_eq!(
        document
            .occurrences
            .iter()
            .filter(|o| o.text == "target"
                && o.kind == OccurrenceKind::Call
                && o.owner == outer.node.data().id)
            .count(),
        6
    );
    graph(&[&document]);
}

/// 被省略的普通初始化声明共用一个执行区域，调用归属不吞掉同语句中的具名方法。
#[test]
fn omitted_initializers_share_one_execution_region() {
    let before = parse("a.ts", "function load() {} const first = load(); let second = load(); const object = { value: load(), method() { load(); } }; const plain = 1;");
    let execution = named(&before, "顶层执行");
    assert_eq!(execution.node.category(), CodeCategory::ExecutionRegion);
    assert_eq!(
        before
            .declarations
            .iter()
            .filter(|d| d.node.category() == CodeCategory::ExecutionRegion)
            .count(),
        1
    );
    assert_eq!(execution.parts.len(), 3);
    let method = named(&before, "method");
    for (owner, count) in [(execution, 3), (method, 1)] {
        assert_eq!(
            before
                .occurrences
                .iter()
                .filter(|o| o.kind == OccurrenceKind::Call && o.owner == owner.node.data().id)
                .count(),
            count
        );
    }
    let after = parse(
        "a.ts",
        &before
            .source
            .replace("method() { load(); }", "method() { load(); load(); }"),
    );
    let diff = graph(&[&before]).diff(&graph(&[&after])).unwrap();
    assert_eq!(diff.nodes.len(), 2, "{diff:?}");
    for owner in [method, execution] {
        assert!(diff
            .nodes
            .iter()
            .any(|change| { change.id == owner.node.data().id && change.content.is_some() }));
    }
    assert!(
        parse("a.ts", "const first = 1; const object = { value: 2 };")
            .declarations
            .is_empty()
    );
}

/// 省略对象包装仍保留词法身份，增加同名方法不会挤动已有对象的 ID。
#[test]
fn object_method_identity_survives_omitted_wrappers() {
    let before = parse(
        "a.ts",
        "const first = { run() { return 1; } }; const second = { run: () => 2 };",
    );
    let after = parse("a.ts", "const another = { run() {} }; const first = { run() { return 1; } }; const second = { run: () => 2 };");
    let after_graph = graph(&[&after]);
    for declaration in before
        .declarations
        .iter()
        .filter(|d| d.node.category() == CodeCategory::Callable)
    {
        assert_eq!(
            after_graph.node(&declaration.node.data().id),
            Some(&declaration.node)
        );
    }
    let diff = graph(&[&before]).diff(&after_graph).unwrap();
    assert_eq!(diff.nodes.len(), 2);
    let module = diff
        .nodes
        .iter()
        .find(|d| d.id == named(&before, "a.ts").node.data().id)
        .unwrap();
    assert!(module.content.is_none());
    assert_eq!(module.contains.len(), 1);
}

/// 类成员独立比较；未建模的嵌套函数及回调变动计入外层方法。
#[test]
fn own_content_excludes_children_but_keeps_callbacks() {
    let before = parse("a.ts", "class C { p: T = make(); constructor() {} get value() { return 1; } set value(v: number) {} m() { function nested() { return 1; } run(() => 1); } static { run(); } }");
    let after = parse("a.ts", "class C { p: T = make(); constructor() {} get value() { return 2; } set value(v: number) {} m() { function nested() { return 2; } run(() => 1); } static { run(); } }");
    let diff = graph(&[&before]).diff(&graph(&[&after])).unwrap();
    assert_eq!(diff.nodes.len(), 2, "{diff:?}");
    assert!(diff
        .nodes
        .iter()
        .all(|d| d.id != named(&before, "C").node.data().id));
    assert!(diff
        .nodes
        .iter()
        .any(|d| d.id == named(&before, "m").node.data().id));
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
        ("class C { x = `a b`; }", "class C { x = `a  b`; }"),
        ("class C { x = /a b/; }", "class C { x = /a  b/; }"),
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
        OccurrenceKind::Reference,
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
        .aliases
        .iter()
        .any(|alias| alias.import && alias.type_only));
    assert!(document
        .occurrences
        .iter()
        .filter(|o| o.kind == OccurrenceKind::Call)
        .all(|o| !o.type_only));
}

/// 导入和纯导出只保留内部别名信息，不生成节点或模块关系候选。
#[test]
fn module_routes_do_not_create_graph_objects() {
    for (path, source) in [
        ("a.ts", "import './style.css'; import {run as alias} from './target'; import type {T} from './types'; export {alias}; export {run as forwarded} from './target'; export type {T} from './types'; export * from './other'; export * as scope from './target'; export default alias;"),
        ("a.cts", "import fs = require('node:fs'); export = fs;"),
        ("a.d.ts", "export as namespace Public; export {};"),
    ] {
        let document = parse(path, source);
        assert!(document.declarations.is_empty(), "{path}: {:?}", document.declarations);
        assert!(document.occurrences.is_empty(), "{path}: {:?}", document.occurrences);
        assert!(graph(&[&document]).root().contains.is_empty());
    }
    let document = parse(
        "a.ts",
        "import {run as alias} from './target'; export {alias as again};",
    );
    assert_eq!(document.aliases.len(), 2);
    for alias in &document.aliases {
        let selection = alias.selection.unwrap();
        assert_eq!(
            &document.source[selection.start as usize..selection.end as usize],
            "alias"
        );
    }
}

/// export 修饰属于真实声明；修改导出可见性仍能产生该对象的差异。
#[test]
fn exported_declarations_keep_single_nodes_and_own_modifiers() {
    for (before, after, name) in [
        ("function f() {}", "export function f() {}", "f"),
        ("class C {}", "export default class C {}", "C"),
        ("const f = () => 1;", "export const f = () => 1;", "f"),
    ] {
        let before = parse("a.ts", before);
        let after = parse("a.ts", after);
        assert_eq!(after.declarations.len(), 2);
        let declaration = named(&after, name);
        assert!(declaration
            .node
            .data()
            .signature
            .as_ref()
            .unwrap()
            .starts_with("export "));
        let diff = graph(&[&before]).diff(&graph(&[&after])).unwrap();
        assert_eq!(diff.nodes.len(), 1);
        assert_eq!(diff.nodes[0].id, declaration.node.data().id);
    }
    let before = parse("a.ts", "namespace N {}");
    let after = parse("a.ts", "namespace N { export function f() {} }");
    assert_eq!(
        named(&before, "N").node.data().signature,
        named(&after, "N").node.data().signature
    );
    assert_eq!(
        named(&after, "f").parent.as_ref(),
        Some(&named(&after, "N").node.data().id)
    );
}

/// 删除模块语句节点不会吞掉导出表达式或 CommonJS 普通执行代码中的调用。
#[test]
fn export_expressions_and_require_calls_keep_execution_owners() {
    for source in [
        "function make() {} export default make();",
        "function make() {} export = make();",
        "function make() {} export default () => make();",
    ] {
        let document = parse("a.ts", source);
        let call = document
            .occurrences
            .iter()
            .find(|o| o.kind == OccurrenceKind::Call)
            .unwrap();
        assert_eq!(
            graph(&[&document]).node(&call.owner).unwrap().category(),
            CodeCategory::ExecutionRegion
        );
        assert!(document
            .declarations
            .iter()
            .all(|d| d.node.category() != CodeCategory::Extension));
    }
    let document = parse("a.cjs", "const fs = require('node:fs'); require('./setup'); function read() { return require('./local'); }");
    assert!(document
        .declarations
        .iter()
        .all(|d| d.node.data().name.as_deref() != Some("fs")));
    let graph = graph(&[&document]);
    let owners = document
        .occurrences
        .iter()
        .filter(|o| o.kind == OccurrenceKind::Call && o.text == "require")
        .map(|o| graph.node(&o.owner).unwrap().category())
        .collect::<Vec<_>>();
    assert_eq!(
        owners,
        [
            CodeCategory::ExecutionRegion,
            CodeCategory::ExecutionRegion,
            CodeCategory::Callable
        ]
    );
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
            SourceText::new("test(() => run());", LINE_BREAKS),
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
            SourceText::new("", LINE_BREAKS),
            AnalysisOptions::default()
        )
        .is_err());
    }
    assert!(Document::parse(
        Path::new("/repo"),
        "/repo/a.ts".into(),
        SourceText::new("function {", LINE_BREAKS),
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
        SourceText::new(source, LINE_BREAKS),
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
    assert!(a.declarations.iter().all(|declaration| declaration
        .node
        .data()
        .id
        .0
        .contains("%E7%9B%AE")));
}

/// 普通变量和对象属性不建模，其函数内变动属于外层函数，顶层方法独立比较。
#[test]
fn variable_children_and_object_members_have_own_content() {
    let before = parse("a.ts", "function f() { const a = 1, b = 2; } const obj = { field: 1, method() { return 1; }, get value() { return 1; } };");
    let after = parse("a.ts", "function f() { const a = 3, b = 4; } const obj = { field: 2, method() { return 2; }, get value() { return 1; } };");
    let diff = graph(&[&before]).diff(&graph(&[&after])).unwrap();
    assert_eq!(diff.nodes.len(), 2, "{diff:?}");
    for name in ["f", "method"] {
        assert!(diff
            .nodes
            .iter()
            .any(|d| d.id == named(&before, name).node.data().id));
    }
    assert_eq!(
        named(&before, "method").node.category(),
        CodeCategory::Callable
    );
}

/// 解构绑定不生成节点或写入引用，默认值和右侧的读取证据仍保留。
#[test]
fn destructuring_assignment_targets_are_omitted() {
    let document = parse("a.ts", "const { a, renamed: b = initial } = input; function f() { ({ a, renamed: b = initial } = input); [a, b] = input; }");
    assert_eq!(document.declarations.len(), 2);
    assert_eq!(
        named(&document, "f").node.category(),
        CodeCategory::Callable
    );
    assert!(document
        .declared_at(document.source.find("renamed").unwrap() as u32)
        .is_none());
    assert!(document
        .occurrences
        .iter()
        .all(|o| !matches!(o.text.as_str(), "a" | "b")));
    for name in ["initial", "input"] {
        assert!(document
            .occurrences
            .iter()
            .any(|o| o.kind == OccurrenceKind::Reference && o.text == name));
    }
}

/// 写入目标不回退为普通引用，赋值各部分的调用及右侧函数值仍保留。
#[test]
fn assignments_keep_calls_without_target_references() {
    let source = r#"
function replacement() {}
class Model {
    value = 0;
    #secret = 0;
    handler = () => {};
    update() {
        this.value = rhs();
        this.value += rhs();
        this.value++;
        this.#secret = rhs();
        this.handler = replacement;
        ({ item: this.value = fallback() } = receiver());
        [this.value = fallback()] = [rhs()];
        receiver().value = rhs();
        receiver()[key()] = rhs();
        let handler;
        ({ handler = replacement } = receiver());
        handler = replacement;
    }
}
"#;
    for path in ["a.ts", "a.js"] {
        let document = parse(path, source);
        assert!(
            document
                .occurrences
                .iter()
                .all(|o| { !matches!(o.text.as_str(), "value" | "#secret" | "handler") }),
            "{path}: {:?}",
            document.occurrences
        );
        for name in ["rhs", "fallback", "receiver", "key"] {
            assert!(
                document.occurrences.iter().any(|o| {
                    o.kind == OccurrenceKind::Call
                        && o.text == name
                        && o.owner == named(&document, "update").node.data().id
                }),
                "{path}: missing call {name}"
            );
        }
        assert_eq!(
            document
                .occurrences
                .iter()
                .filter(|o| { o.kind == OccurrenceKind::Reference && o.text == "replacement" })
                .count(),
            3
        );
    }
    let document = parse("a.ts", "class Model { handler = () => {}; update() { (this.handler as unknown) = replacement; this.handler! = replacement; } }");
    assert!(document.occurrences.iter().all(|o| o.text != "handler"));
}

/// 省略写入关系不影响函数实现的源码展示与自身修改判定。
#[test]
fn assignment_changes_remain_in_callable_diff() {
    for path in ["a.ts", "a.js"] {
        let before = parse(
            path,
            "class Model { value = 0; update() { this.value = 1; } }",
        );
        let after = parse(
            path,
            "class Model { value = 0; update() { this.value = 2; } }",
        );
        let diff = graph(&[&before]).diff(&graph(&[&after])).unwrap();
        assert_eq!(diff.nodes.len(), 1, "{diff:?}");
        assert_eq!(diff.nodes[0].id, named(&after, "update").node.data().id);
        assert!(named(&after, "update")
            .node
            .data()
            .source
            .as_deref()
            .unwrap()
            .contains("this.value = 2"));
    }
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
                SourceText::new(source, LINE_BREAKS),
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
    let document = parse("a.ts", "function handler() {} function owner() { const obj = { handler }; } class C { #secret = 1; [makeKey()]() { return this.#secret; } }");
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

/// 指令合入顶层执行区域；参数属性单独承载字段类型和默认值调用。
#[test]
fn directives_and_parameter_properties_keep_separate_execution_owners() {
    let document = parse("a.ts", "'use strict'; class C { constructor(public field: Model, readonly other = make(), ordinary: string) {} }");
    assert!(named(&document, "顶层执行")
        .node
        .data()
        .code
        .as_ref()
        .unwrap()
        .contains("use strict"));
    for name in ["field", "other"] {
        assert_eq!(named(&document, name).node.category(), CodeCategory::Value);
        assert_eq!(
            named(&document, name).parent.as_ref(),
            Some(&named(&document, "C").node.data().id)
        );
    }
    assert!(document
        .declarations
        .iter()
        .all(|d| d.node.data().name.as_deref() != Some("ordinary")));
    assert!(document
        .occurrences
        .iter()
        .any(|o| o.kind == OccurrenceKind::TypeUse
            && o.text == "Model"
            && o.owner == named(&document, "field").node.data().id));
    assert!(document
        .occurrences
        .iter()
        .any(|o| o.kind == OccurrenceKind::Call
            && o.text == "make"
            && o.owner == named(&document, "other").node.data().id));
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

/// 实际调用的选区同时保留 UTF-8 字节和 TS 行列，混合换行及非 BMP 字符不能使定位偏移。
#[test]
fn occurrence_selections_preserve_typescript_byte_and_utf16_positions() {
    let source =
        "function 调用() {}\r\nfunction f() {\u{2028}  \"😀中\"; 调用();\u{2029}  调用();\r  调用();\r\n}";
    let document = parse("a.ts", source);
    assert_eq!(document.source.as_str(), source);
    let calls = document
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.kind == OccurrenceKind::Call)
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 3);
    for (occurrence, line, character) in [(calls[0], 2, 9), (calls[1], 3, 2), (calls[2], 4, 2)] {
        let start = occurrence.selection.start as usize;
        let end = occurrence.selection.end as usize;
        assert_eq!(&document.source[start..end], "调用");
        assert_eq!(occurrence.owner, named(&document, "f").node.data().id);
        assert_eq!(occurrence.position, Position { line, character });
        assert_eq!(
            document.source.position(start).unwrap(),
            occurrence.position
        );
        assert_eq!(
            document.source.byte_offset(&occurrence.position).unwrap(),
            start
        );
        let end_position = Position {
            line,
            character: character + 2,
        };
        assert_eq!(document.source.position(end).unwrap(), end_position);
        assert_eq!(document.source.byte_offset(&end_position).unwrap(), end);
    }
}

/// 独立子对象增删只改变父包含关系，不得产生空容器与带空格容器的内容差异。
#[test]
fn first_child_insertion_and_last_child_removal_preserve_owner_content() {
    for (before, after, owner) in [
        ("class C {}", "class C { method() {} }", "C"),
        ("interface I {}", "interface I { field: string; }", "I"),
        ("namespace N {}", "namespace N { function child() {} }", "N"),
        ("enum E {}", "enum E { A = 1, B = 2 }", "E"),
        (
            "type T = {}",
            "type T = { field: string; method(): void }",
            "T",
        ),
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
    let base = parse("a.ts", "namespace N { before(); after(); }");
    let one = parse(
        "a.ts",
        "namespace N { before(); function a() {} after(); function b() {} }",
    );
    let reordered = parse(
        "a.ts",
        "namespace N { function b() {} before(); function a() {} after(); }",
    );
    let id = &named(&base, "N").node.data().id;
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

/// 对象初始化器完整比较，增加或重排具名方法仍会改变字段值。
#[test]
fn object_members_and_spreads_participate_in_initializer_content() {
    let before = parse("a.ts", "class C { value = { ...left, ...right }; }");
    let after = parse(
        "a.ts",
        "class C { value = { first() {}, ...left, middle() {}, ...right, last() {} }; }",
    );
    let reordered = parse(
        "a.ts",
        "class C { value = { last() {}, first() {}, ...left, ...right, middle() {} }; }",
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
                .is_some(),
            "{diff:?}"
        );
    }
    let diff = graph(&[&after]).diff(&graph(&[&reordered])).unwrap();
    assert_eq!(diff.nodes.len(), 1, "{diff:?}");
    assert_eq!(&diff.nodes[0].id, id);
    assert!(diff.nodes[0].content.is_some());
    assert!(diff.nodes[0].contains.is_empty());
}

/// 字段初始化中的对象、函数实参和条件表达式完整保留，不扣除独立子声明。
#[test]
fn initializer_expressions_keep_independent_declarations() {
    for (before, after) in [
        ("{}", "{ method() {} }"),
        (
            "factory(before, after)",
            "factory(function first() {}, before, function middle() {}, after, function last() {})",
        ),
        (
            "enabled ? fallback : null",
            "enabled ? class Child {} : null",
        ),
    ] {
        let before = parse("a.ts", &format!("class C {{ value = {before}; }}"));
        let after = parse("a.ts", &format!("class C {{ value = {after}; }}"));
        let id = &named(&before, "value").node.data().id;
        for (before, after) in [(&before, &after), (&after, &before)] {
            let diff = graph(&[before]).diff(&graph(&[after])).unwrap();
            let owner = diff.nodes.iter().find(|d| &d.id == id).unwrap();
            assert!(owner.content.is_some(), "{diff:?}");
            assert!(!owner.contains.is_empty());
            assert!(!diff
                .nodes
                .iter()
                .any(|d| d.id == named(before, "C").node.data().id));
        }
    }
}

/// 完整表达式比较保留数组空位、逗号表达式或字面量中的有意义内容。
#[test]
fn own_content_keeps_semantic_punctuation_and_literal_text() {
    let before = parse(
        "a.ts",
        "class C { value = { ...build((a, b), c), ...right }; }",
    );
    let with_child = parse(
        "a.ts",
        "class C { value = { child() {}, ...build((a, b), c), ...right, last() {} }; }",
    );
    let id = &named(&before, "value").node.data().id;
    let diff = graph(&[&before]).diff(&graph(&[&with_child])).unwrap();
    assert!(diff
        .nodes
        .iter()
        .find(|d| &d.id == id)
        .unwrap()
        .content
        .is_some());
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
        let before = parse("a.tsx", &format!("function owner() {{ {before} }}"));
        let after = parse("a.tsx", &format!("function owner() {{ {after} }}"));
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

/// 数组中的具名函数和类仍独立建模，增删同时改变字段初始化值与包含关系。
#[test]
fn array_child_insertion_and_removal_change_initializer_content() {
    for (before, after) in [
        ("[]", "[function Child() {}]"),
        ("[keep]", "[function Child() {}, keep]"),
        ("[keep, ...rest]", "[keep, class Child {}, ...rest]"),
        ("[keep, ...rest]", "[keep, ...rest, function Child() {}]"),
        ("[,]", "[function Child() {}, ,]"),
        ("[,]", "[, class Child {}]"),
        ("[keep, ,]", "[keep, function Child() {}, ,]"),
        ("[keep, ,]", "[keep, , class Child {}]"),
        ("[, keep]", "[, function Child() {}, keep]"),
        ("[,,]", "[function First() {}, , class Last {}, ,]"),
        ("[`a, b`, /a,b/, <div>a,b</div>, ...rest]", "[class First {}, `a, b`, function Middle() {}, /a,b/, <div>a,b</div>, ...rest, function Last() {}]"),
    ] {
        let before = parse("a.tsx", &format!("class C {{ value = {before}; }}"));
        let after = parse("a.tsx", &format!("class C {{ value = {after}; }}"));
        let id = &named(&before, "value").node.data().id;
        for (before, after) in [(&before, &after), (&after, &before)] {
            let diff = graph(&[before]).diff(&graph(&[after])).unwrap();
            let parent = diff.nodes.iter().find(|d| &d.id == id).unwrap();
            assert!(parent.content.is_some(), "{} -> {}: {diff:?}", before.source, after.source);
            assert!(!parent.contains.is_empty());
        }
    }
}

/// 数组顺序是字段值的一部分；重排改变字段内容，但独立子对象本身保持不变。
#[test]
fn array_child_reordering_changes_only_initializer_content() {
    let before = parse(
        "a.ts",
        "class C { value = [function First() {}, , keep, class Last {}, ...rest, ,]; }",
    );
    let after = parse(
        "a.ts",
        "class C { value = [, class Last {}, keep, ...rest, , function First() {}]; }",
    );
    let diff = graph(&[&before]).diff(&graph(&[&after])).unwrap();
    assert_eq!(diff.nodes.len(), 1, "{diff:?}");
    assert_eq!(diff.nodes[0].id, named(&before, "value").node.data().id);
    assert!(diff.nodes[0].content.is_some());
    assert!(diff.nodes[0].contains.is_empty());
}

/// 不同真实空位的位置与数量仍产生内容差异，数组分隔符不能被整体删光。
#[test]
fn sparse_array_hole_positions_and_counts_remain_distinct() {
    let arrays = ["[keep]", "[, keep]", "[keep, ,]", "[,,]", "[]"];
    for (index, left) in arrays.iter().enumerate() {
        for right in arrays.iter().skip(index + 1) {
            let before = parse("a.ts", &format!("class C {{ value = {left}; }}"));
            let after = parse("a.ts", &format!("class C {{ value = {right}; }}"));
            assert!(
                !graph(&[&before])
                    .diff(&graph(&[&after]))
                    .unwrap()
                    .nodes
                    .is_empty(),
                "{left} versus {right}"
            );
        }
    }
}

/// 类型专用导入的非法值读取仍保留词法声明；局部同名值不可被外层导入吞掉。
#[test]
fn alias_metadata_keeps_lexical_identity_and_public_routes() {
    let document = parse("a.ts", "// @ts-nocheck\nimport type {Model} from './model'; export {Model as Public}; export default Model; function outer() { Model.run(); new Model(); } function shadow() { class Model { static run() {} } Model.run(); new Model(); }");
    let imported = document.aliases.iter().find(|a| a.import).unwrap();
    assert!(imported.type_only);
    for export in document.aliases.iter().filter(|a| !a.import) {
        assert_eq!(export.binding, imported.binding);
    }
    for name in ["outer", "shadow"] {
        let owner = &named(&document, name).node.data().id;
        let calls = document
            .occurrences
            .iter()
            .filter(|o| {
                &o.owner == owner
                    && matches!(o.kind, OccurrenceKind::Call | OccurrenceKind::Construct)
            })
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 2);
        assert!(calls.iter().all(|o| o.root_binding.is_some()));
        assert!(calls
            .iter()
            .all(|o| (o.root_binding == imported.binding) == (name == "outer")));
    }
}

/// 仓库外的测试目录名称不影响生产声明、身份和内容，两个入口都使用仓库内路径。
#[test]
fn repository_relative_test_classification_ignores_checkout_ancestors() {
    let source = "export function production() { test(() => 1); }";
    let expected = parse("src/main.ts", source);
    for root in [
        "/ordinary/repo",
        "/outer/__tests__/repo",
        "/outer/__tests__",
    ] {
        let actual = Document::parse(
            Path::new(root),
            Path::new(root).join("src/main.ts"),
            SourceText::new(source, LINE_BREAKS),
            AnalysisOptions::default(),
        )
        .unwrap();
        assert!(!actual.excluded, "{root}");
        assert_eq!(actual.source, expected.source);
        assert_eq!(actual.declarations.len(), expected.declarations.len());
        for (actual, expected) in actual.declarations.iter().zip(&expected.declarations) {
            assert_eq!(actual.node, expected.node);
        }
        assert!(
            !Document::parse(
                Path::new(root),
                Path::new(root).join("contest.ts"),
                SourceText::new(source, LINE_BREAKS),
                AnalysisOptions::default(),
            )
            .unwrap()
            .excluded
        );
    }
}
