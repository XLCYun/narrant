//! 构造函数参数属性与普通实例字段共用类别、类归属和稳定身份。
use super::*;

/// 可见性和 readonly 均可引入字段，组合修饰符不能产生重复节点。
#[test]
fn parameter_properties_are_direct_class_fields() {
    for modifier in [
        "public",
        "private",
        "protected",
        "readonly",
        "public readonly",
        "private readonly",
        "protected readonly",
    ] {
        let source = format!("class C {{ constructor({modifier} value: number) {{}} }}");
        let document = parse("a.ts", &source);
        let class = named(&document, "C").node.data();
        let constructor = named(&document, "constructor").node.data();
        let field = named(&document, "value");
        assert_eq!(field.node.category(), CodeCategory::Value, "{modifier}");
        assert_eq!(field.node.data().native_kind, "field");
        assert_eq!(field.parent.as_ref(), Some(&class.id));
        assert!(class.contains.contains(&field.node.data().id));
        assert!(!constructor.contains.contains(&field.node.data().id));
        assert_eq!(document.declarations.len(), 4);
        assert_eq!(
            field.node.data().source.as_deref(),
            Some(format!("{modifier} value: number").as_str())
        );
        assert_eq!(
            document
                .declared_at(source.find("value").unwrap() as u32)
                .unwrap()
                .node
                .data()
                .id,
            field.node.data().id
        );
        graph(&[&document]);
    }
}

/// override 及其合法组合仍是派生类自身字段，不与基类字段合并。
#[test]
fn overriding_parameter_properties_belong_to_the_derived_class() {
    for modifier in [
        "override",
        "public override",
        "override readonly",
        "protected override readonly",
    ] {
        let document = parse("a.ts", &format!(
            "class Base {{ protected value = 0; }} class Derived extends Base {{ constructor({modifier} value: number) {{ super(); }} }}"
        ));
        let derived = named(&document, "Derived").node.data();
        let fields = document
            .declarations
            .iter()
            .filter(|d| d.node.data().name.as_deref() == Some("value"))
            .collect::<Vec<_>>();
        assert_eq!(fields.len(), 2);
        assert_ne!(fields[0].node.data().id, fields[1].node.data().id);
        assert_eq!(fields[1].parent.as_ref(), Some(&derived.id));
        graph(&[&document]);
    }
}

/// 保留装饰器、可选标记与默认值；普通参数和 rest 不变成字段。
#[test]
fn parameter_properties_preserve_source_and_expression_ownership() {
    let document = parse(
        "a.ts",
        r#"
        class C {
            constructor(
                /** 服务依赖 */ @inject() private readonly service: Model = make(),
                public optional?: Model,
                ordinary: Model = makeOrdinary(),
                readonly callback: () => Model = () => makeCallback(),
                ...rest: Model[]
            ) {}
        }
    "#,
    );
    let service = named(&document, "service");
    assert_eq!(
        service.node.data().source.as_deref(),
        Some("/** 服务依赖 */ @inject() private readonly service: Model = make()")
    );
    assert_eq!(service.node.data().code.as_deref(), Some("make ( )"));
    assert!(named(&document, "optional")
        .node
        .data()
        .signature
        .as_deref()
        .unwrap()
        .contains("?"));
    for omitted in ["ordinary", "rest"] {
        assert!(document
            .declarations
            .iter()
            .all(|d| d.node.data().name.as_deref() != Some(omitted)));
    }
    for (owner, call) in [
        ("service", "make"),
        ("constructor", "makeOrdinary"),
        ("callback", "makeCallback"),
    ] {
        assert!(
            document
                .occurrences
                .iter()
                .any(|o| o.kind == OccurrenceKind::Call
                    && o.text == call
                    && o.owner == named(&document, owner).node.data().id),
            "{owner}: {call}"
        );
    }
    let callback = named(&document, "callback");
    assert_eq!(callback.node.category(), CodeCategory::Value);
    assert!(
        callback.callable_value.is_none(),
        "默认函数可被实参覆盖，不能当作确定的运行时函数值"
    );
    graph(&[&document]);
}

/// 参数次序、注释和声明写法不改变同一个类字段的身份。
#[test]
fn parameter_property_identity_matches_explicit_fields() {
    let before = parse(
        "a.ts",
        "class C { constructor(public first: number, private second: string) {} }",
    );
    let reordered = parse(
        "a.ts",
        "class C { constructor(private second: string, /* moved */ public first: number) {} }",
    );
    let explicit = parse("a.ts", "class C { public first: number; private second: string; constructor(first: number, second: string) { this.first = first; this.second = second; } }");
    for name in ["first", "second"] {
        let field = named(&before, name).node.data();
        assert_eq!(field.id, named(&reordered, name).node.data().id);
        assert_eq!(field.id, named(&explicit, name).node.data().id);
        assert_eq!(
            field.signature,
            named(&reordered, name).node.data().signature
        );
    }
}

/// 字段变化参与 diff，同时保留完整构造签名；函数体变化不会污染参数字段。
#[test]
fn parameter_property_changes_preserve_class_own_content() {
    let source = "class C { constructor(readonly value: number = 1) { run(); } }";
    let before = parse("a.ts", source);
    for after in [
        source.replace("= 1", "= 2"),
        source.replace("number", "number | string"),
    ] {
        let after = parse("a.ts", &after);
        let diff = graph(&[&before]).diff(&graph(&[&after])).unwrap();
        assert_eq!(diff.nodes.len(), 2, "{diff:?}");
        for name in ["value", "constructor"] {
            assert!(
                diff.nodes
                    .iter()
                    .any(|d| d.id == named(&before, name).node.data().id && d.content.is_some()),
                "{name}: {diff:?}"
            );
        }
    }
    let after = parse("a.ts", &source.replace("run()", "other()"));
    let diff = graph(&[&before]).diff(&graph(&[&after])).unwrap();
    assert_eq!(diff.nodes.len(), 1);
    assert_eq!(
        diff.nodes[0].id,
        named(&before, "constructor").node.data().id
    );
}

/// 重载不会重复字段；函数内的具名类仍拥有字段，匿名类不能泄漏给外层类。
#[test]
fn parameter_property_owners_survive_overloads_and_nested_classes() {
    let document = parse(
        "a.ts",
        r#"
        class Outer {
            constructor(value: number);
            constructor(public value: number) {}
            create() {
                class Inner { constructor(readonly nested: number) {} }
                return class { constructor(public hidden: number) {} };
            }
        }
        class Container {
            constructor(readonly factory = class { constructor(public hidden: HiddenModel) {} }) {}
        }
    "#,
    );
    assert_eq!(named(&document, "constructor").parts.len(), 2);
    assert_eq!(
        named(&document, "value").parent.as_ref(),
        Some(&named(&document, "Outer").node.data().id)
    );
    assert_eq!(
        named(&document, "nested").parent.as_ref(),
        Some(&named(&document, "Inner").node.data().id)
    );
    assert!(document
        .declarations
        .iter()
        .all(|d| d.node.data().name.as_deref() != Some("hidden")));
    assert!(!document
        .occurrences
        .iter()
        .any(|o| { o.kind == OccurrenceKind::TypeUse && o.text == "HiddenModel" }));
    graph(&[&document]);
}
