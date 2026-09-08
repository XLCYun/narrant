use super::*;

/// 格式变化忽略，字面量内容、稀疏数组槽和自动分号插入语义仍有差异。
#[test]
fn distinguishes_semantics_from_safe_typescript_formatting() {
    let read = |source| {
        extract_source(
            "a.ts",
            &parse_source(SourceLanguage::TypeScript, source).unwrap(),
        )
    };
    let compact = read("function f(){return ['a b'];}");
    let spaced = read("function f ( ) {\n return [ 'a b' ];\n}");
    assert_eq!(
        compact.declarations[0].body_tokens,
        spaced.declarations[0].body_tokens
    );
    assert_eq!(
        compact.declarations[0].signature_tokens,
        spaced.declarations[0].signature_tokens
    );
    assert_ne!(
        compact.declarations[0].body_tokens,
        read("function f(){return ['ab'];}").declarations[0].body_tokens
    );
    assert_ne!(
        compact.declarations[0].body_tokens,
        read("function f(){return\n['a b'];}").declarations[0].body_tokens
    );
    assert_ne!(
        read("const xs=[1,2]").declarations[0].body_tokens,
        read("const xs=[1,,2]").declarations[0].body_tokens
    );
    assert_eq!(
        read("const xs=[1,2]").declarations[0].body_tokens,
        read("const xs=[1,2,]").declarations[0].body_tokens
    );
}

/// 新增枚举/导出成员不改变容器自身，绑定修饰符改变属于具名箭头自身。
#[test]
fn separates_container_members_from_binding_modifier_changes() {
    let read = |source| {
        extract_source(
            "a.ts",
            &parse_source(SourceLanguage::TypeScript, source).unwrap(),
        )
    };
    let before = read("enum E { A } namespace N {} const f=()=>1;");
    let after = read("enum E { A, B } namespace N { export const x=1; } let f=()=>1;");
    for name in ["E", "N"] {
        let old = before
            .declarations
            .iter()
            .find(|item| item.name == name)
            .unwrap();
        let new = after
            .declarations
            .iter()
            .find(|item| item.name == name)
            .unwrap();
        assert_eq!(old.body_tokens, new.body_tokens, "{name}");
        assert_eq!(old.signature_tokens, new.signature_tokens, "{name}");
    }
    assert_ne!(
        before
            .declarations
            .iter()
            .find(|item| item.name == "f")
            .unwrap()
            .signature_tokens,
        after
            .declarations
            .iter()
            .find(|item| item.name == "f")
            .unwrap()
            .signature_tokens
    );
}

/// 类型参数只作为类型引用，不伪造成第二个父类型；限定继承只出现一次。
#[test]
fn retains_javascript_and_typescript_heritage_targets_without_fake_edges() {
    let js = extract_source(
        "a.js",
        &parse_source(SourceLanguage::JavaScript, "class A extends B {}").unwrap(),
    );
    assert!(js
        .occurrences
        .iter()
        .any(|item| item.kind == StructureRelationshipKind::Extends && item.written_target == "B"));
    let ts = extract_source(
        "a.ts",
        &parse_source(
            SourceLanguage::TypeScript,
            "class A extends B<T> implements ns.C {} interface D extends ns.E {}",
        )
        .unwrap(),
    );
    let heritage: Vec<_> = ts
        .occurrences
        .iter()
        .filter(|item| {
            matches!(
                item.kind,
                StructureRelationshipKind::Extends | StructureRelationshipKind::Implements
            )
        })
        .map(|item| (item.kind, item.written_target.as_str()))
        .collect();
    assert_eq!(
        heritage,
        vec![
            (StructureRelationshipKind::Extends, "B"),
            (StructureRelationshipKind::Implements, "ns.C"),
            (StructureRelationshipKind::Extends, "ns.E")
        ]
    );
    assert!(ts
        .occurrences
        .iter()
        .any(|item| item.kind == StructureRelationshipKind::TypeReference
            && item.written_target == "T"));
}

/// 定义来源和默认/具名/命名空间导入各自保留显式别名。
#[test]
fn preserves_import_aliases_reexports_and_native_component_functions() {
    let file = extract_source("view.tsx", &parse_source(SourceLanguage::Tsx, "import Default, { fn as local, type Shape } from '@/api'; import * as all from './all'; export { local as shared }; export default function View() { return <div>{local()}</div>; }").unwrap());
    assert!(file.imports.iter().any(|item| item.module == "@/api"
        && item.imported == "default"
        && item.local == "Default"));
    assert!(file
        .imports
        .iter()
        .any(|item| item.module == "@/api" && item.imported == "fn" && item.local == "local"));
    assert!(file
        .imports
        .iter()
        .any(|item| item.module == "@/api" && item.imported == "Shape" && item.type_only));
    assert!(file
        .imports
        .iter()
        .any(|item| item.module == "./all" && item.imported == "*" && item.local == "all"));
    assert!(file.imports.iter().any(|item| item.module.is_empty()
        && item.imported == "View"
        && item.local == "default"
        && item.reexport));
    assert_eq!(
        file.declarations
            .iter()
            .find(|item| item.name == "View")
            .unwrap()
            .native_kind
            .name,
        StructureNativeName::Function
    );
    assert!(file.occurrences.iter().any(
        |item| item.kind == StructureRelationshipKind::Calls && item.written_target == "local"
    ));
    assert!(!file
        .occurrences
        .iter()
        .any(|item| item.kind == StructureRelationshipKind::Calls && item.written_target == "div"));
}

/// 括号包装的具名箭头仍是自身执行，内部匿名回调保留额外边界。
#[test]
fn keeps_named_wrapped_arrow_execution_and_anonymous_callbacks_distinct() {
    let file = extract_source(
        "a.ts",
        &parse_source(
            SourceLanguage::TypeScript,
            "const run = (() => { direct(); items.map(() => deferred()); });",
        )
        .unwrap(),
    );
    let direct = file
        .occurrences
        .iter()
        .find(|item| item.written_target == "direct")
        .unwrap();
    let deferred = file
        .occurrences
        .iter()
        .find(|item| item.written_target == "deferred")
        .unwrap();
    assert_eq!(direct.declaration, deferred.declaration);
    assert!(direct.callback_range.is_none());
    assert!(deferred.callback_range.is_some());
}

/// 直接作为参数传递的具名函数拥有独立声明；匿名回调仍归外层执行来源。
#[test]
fn retains_named_callback_functions_without_duplicate_variable_bound_declarations() {
    let source = "function outer() { queue(function retry() { retry(); }); queue(function* steps() { yield next(); }); queue(function () { anonymous(); }); const run = function internal() { internal(); }; }";
    for language in [SourceLanguage::TypeScript, SourceLanguage::JavaScript] {
        let file = extract_source("callback.ts", &parse_source(language, source).unwrap());
        let outer = file
            .declarations
            .iter()
            .position(|item| item.name == "outer")
            .unwrap();
        for name in ["retry", "steps"] {
            let (index, declaration) = file
                .declarations
                .iter()
                .enumerate()
                .find(|(_, item)| item.name == name)
                .unwrap();
            assert_eq!(declaration.parent, Some(outer));
            assert_eq!(declaration.native_kind.name, StructureNativeName::Function);
            let target = if name == "retry" { "retry" } else { "next" };
            let call = file
                .occurrences
                .iter()
                .find(|item| item.written_target == target)
                .unwrap();
            assert_eq!(call.declaration, Some(index));
            assert!(call.callback_range.is_none());
        }
        assert_eq!(
            file.declarations
                .iter()
                .filter(|item| item.name == "run")
                .count(),
            1
        );
        assert!(!file.declarations.iter().any(|item| item.name == "internal"));
        let anonymous = file
            .occurrences
            .iter()
            .find(|item| item.written_target == "anonymous")
            .unwrap();
        assert_eq!(anonymous.declaration, Some(outer));
        assert!(anonymous.callback_range.is_some());
    }
}

/// UTF-16 列反映原始非 BMP 字符，不把 UTF-8 字节列直接交给编辑器。
#[test]
fn positions_use_original_utf16_columns_after_non_bmp_text() {
    let file = extract_source(
        "a.ts",
        &parse_source(
            SourceLanguage::TypeScript,
            "const marker = '😀'; const value = 1;\r\nfunction f(){}\r\n",
        )
        .unwrap(),
    );
    let value = file
        .declarations
        .iter()
        .find(|item| item.name == "value")
        .unwrap();
    assert_eq!(value.name_range.position.start_line, 1);
    assert_eq!(value.name_range.position.start_column, 28);
    assert_eq!(
        file.declarations
            .iter()
            .find(|item| item.name == "f")
            .unwrap()
            .name_range
            .position
            .start_line,
        2
    );
}
