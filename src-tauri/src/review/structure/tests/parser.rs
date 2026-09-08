use super::{parse_source, ParsedSource, SourceLanguage, SyntaxDiagnosticKind};

/// 通过模块入口验证真实 TS 声明及可回到原文的范围。
#[test]
fn parses_typescript_declarations_from_memory() {
    let source = "class Box { value: number; read() { return this.value; } }\nfunction helper() {}\nconst arrow = () => 1;";
    let parsed = parse_source(SourceLanguage::TypeScript, source).unwrap();
    let names: Vec<_> = parsed
        .outline
        .iter()
        .map(|entry| entry.name.as_str())
        .collect();
    assert_eq!(names, ["Box", "value", "read", "helper", "arrow"]);
    assert_eq!(parsed.outline[0].native_kind, "class_declaration");
    assert_eq!(parsed.outline[1].parent, Some(0));
    assert_eq!(parsed.outline[4].native_kind, "variable_declarator");
    assert_eq!(
        &source[parsed.outline[4].range.bytes.clone()],
        "arrow = () => 1"
    );
    assert!(parsed.diagnostics.is_empty());
}

/// 错误恢复必须同时保留 ERROR、零宽 MISSING、可信声明和未经补写的原文。
#[test]
fn damaged_source_keeps_both_diagnostic_kinds_and_readable_declarations() {
    let source = "fn good() {}\nfn broken() { let value = ; }\nfn missing() { let value = 1 }";
    let parsed = parse_source(SourceLanguage::Rust, source).unwrap();
    assert_eq!(parsed.source, source);
    assert!(parsed.outline.iter().any(|entry| entry.name == "good"));
    assert!(parsed
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.kind == SyntaxDiagnosticKind::Error));
    let missing = parsed
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.kind == SyntaxDiagnosticKind::Missing)
        .expect("missing semicolon must remain visible");
    assert!(missing.range.bytes.is_empty());
    assert_eq!(missing.range.start.line, 2);
    assert_eq!(parsed.syntax[missing.syntax_node].native_kind, ";");
    assert!(parsed.syntax[missing.syntax_node].is_missing);
    assert!(parsed.syntax[0].contains_error);
}

/// TSX 组件沿用原生函数与具名箭头语法，不依赖 React 或语言服务器。
#[test]
fn parses_tsx_function_and_named_arrow_components() {
    let source = "function Card() { return <div>中文</div>; }\nexport const Label = () => <span/>;";
    let parsed = parse_source(SourceLanguage::Tsx, source).unwrap();
    assert!(parsed.diagnostics.is_empty());
    assert!(parsed
        .outline
        .iter()
        .any(|entry| entry.name == "Card" && entry.native_kind == "function_declaration"));
    assert!(parsed.outline.iter().any(|entry| entry.name == "Label"));
    assert!(parsed
        .syntax
        .iter()
        .any(|node| node.native_kind == "jsx_element"));
    assert!(parsed
        .syntax
        .iter()
        .any(|node| node.native_kind == "arrow_function"));
    trace_outline(&parsed);
}

/// JavaScript grammar 同时解析 JSX，并保存现成 import 与成员证据。
#[test]
fn parses_javascript_jsx_and_import_evidence() {
    let source = "import { run as execute } from './runner';\nexport class Box { read() { return execute(); } }\nconst arrow = () => <div/>;";
    let parsed = parse_source(SourceLanguage::JavaScript, source).unwrap();
    assert!(parsed.diagnostics.is_empty());
    let import = parsed
        .outline
        .iter()
        .find(|entry| entry.is_import == Some(true))
        .unwrap();
    assert_eq!(import.name, "'./runner'");
    let class = parsed
        .outline
        .iter()
        .position(|entry| entry.name == "Box")
        .unwrap();
    assert_eq!(parsed.outline[class].is_exported, Some(true));
    let method = parsed
        .outline
        .iter()
        .find(|entry| entry.name == "read")
        .unwrap();
    assert_eq!(method.parent, Some(class));
    assert_eq!(
        &source[method.range.bytes.clone()],
        "read() { return execute(); }"
    );
    assert!(parsed.outline.iter().any(|entry| entry.name == "arrow"));
    trace_outline(&parsed);
}

/// Rust 原生 struct、trait 和多个 impl 保持独立，方法直接指向各自实现。
#[test]
fn preserves_rust_native_kinds_and_separate_implementation_groups() {
    let source = "pub struct Box { pub value: i32 }\ntrait Read { fn read(&self) -> i32; }\nimpl Read for Box { fn read(&self) -> i32 { self.value } }\nimpl Box { pub fn new() -> Self { todo!() } }";
    let parsed = parse_source(SourceLanguage::Rust, source).unwrap();
    assert!(parsed.diagnostics.is_empty());
    let kinds: Vec<_> = parsed
        .outline
        .iter()
        .map(|entry| entry.native_kind.as_str())
        .collect();
    assert_eq!(
        kinds,
        [
            "struct_item",
            "field_declaration",
            "trait_item",
            "function_signature_item",
            "impl_item",
            "function_item",
            "impl_item",
            "function_item"
        ]
    );
    assert_eq!(parsed.outline[3].parent, Some(2));
    assert_eq!(parsed.outline[5].parent, Some(4));
    assert_eq!(parsed.outline[7].parent, Some(6));
    assert_ne!(parsed.outline[4].range, parsed.outline[6].range);
    let implementation = &parsed.syntax[parsed.outline[4].syntax_node];
    assert!(implementation
        .children
        .iter()
        .any(|&id| parsed.syntax[id].field.as_deref() == Some("trait")
            && &source[parsed.syntax[id].range.bytes.clone()] == "Read"));
    trace_outline(&parsed);
}

/// 嵌套声明、类型别名和字段即使没有 outline 条目，也保留完整语法供后续补齐。
#[test]
fn retains_native_syntax_beyond_provider_outline_coverage() {
    for (language, source, required_kinds) in [
        (SourceLanguage::TypeScript, "export const arrow = () => 1;\ntype Alias = { value: number };\nfunction outer() { function inner() {} }\nnamespace N { export function nested() {} }", vec!["arrow_function", "type_alias_declaration", "function_declaration", "internal_module"]),
        (SourceLanguage::Rust, "mod nested { struct Item { value: i32 } impl Item { fn method(&self) {} } fn outer() { fn inner() {} } type Alias = Item; const FLAG: bool = true; }", vec!["mod_item", "field_declaration", "impl_item", "function_item", "type_item", "const_item"]),
    ] {
        let parsed = parse_source(language, source).unwrap();
        assert!(parsed.diagnostics.is_empty());
        for kind in required_kinds {
            assert!(parsed.syntax.iter().any(|node| node.native_kind == kind), "missing native syntax {kind}");
        }
        let inner_name = parsed.syntax.iter().find(|node| node.field.as_deref() == Some("name") && &source[node.range.bytes.clone()] == "inner").unwrap();
        let inner_function = &parsed.syntax[inner_name.parent.unwrap()];
        assert_eq!(&source[inner_function.range.bytes.clone()], if language == SourceLanguage::Rust { "fn inner() {}" } else { "function inner() {}" });
        trace_outline(&parsed);
    }
}

/// 中文、emoji 和 CRLF 的原始偏移不得被字符列或换行归一化破坏。
#[test]
fn preserves_utf8_byte_columns_crlf_and_original_text() {
    let source = "// 标题\r\nconst 汉 = 1; const arrow = () => '😀';\r\n";
    let parsed = parse_source(SourceLanguage::TypeScript, source).unwrap();
    assert_eq!(parsed.source, source);
    let arrow = parsed
        .outline
        .iter()
        .find(|entry| entry.name == "arrow")
        .unwrap();
    assert_eq!(arrow.range.start.line, 1);
    assert_eq!(arrow.range.start.byte_column, 21);
    assert_eq!(&source[arrow.range.bytes.clone()], "arrow = () => '😀'");
    assert_eq!(parsed.syntax[0].range.bytes, 0..source.len());
    assert_eq!(parsed.syntax[0].range.end.line, 2);
}

/// 空内容仍有可读取原文和完整语法根，不会伪造声明或缺失诊断。
#[test]
fn empty_input_is_readable_and_repeated_parses_do_not_reuse_source() {
    for language in [
        SourceLanguage::TypeScript,
        SourceLanguage::Tsx,
        SourceLanguage::JavaScript,
        SourceLanguage::Rust,
    ] {
        let empty = parse_source(language, "").unwrap();
        assert_eq!(empty.source, "");
        assert!(empty.outline.is_empty());
        assert!(empty.diagnostics.is_empty());
        assert_eq!(empty.syntax[0].range.bytes, 0..0);
    }
    let first = parse_source(SourceLanguage::Rust, "fn first() {}").unwrap();
    let second = parse_source(SourceLanguage::Rust, "fn second() {}").unwrap();
    assert_eq!(first.outline[0].name, "first");
    assert_eq!(second.outline[0].name, "second");
    assert_eq!(first.source, "fn first() {}");
}

/// 输出可复查的真实 provider 证据供调查报告引用；断言仍针对模块行为。
fn trace_outline(parsed: &ParsedSource) {
    eprintln!(
        "{:?} {}: {} syntax nodes",
        parsed.language,
        parsed.provider,
        parsed.syntax.len()
    );
    for entry in &parsed.outline {
        eprintln!(
            "  {} {:?} {} {:?} parent={:?} signature={:?}",
            entry.native_kind,
            entry.name,
            entry.provider_category,
            entry.range.bytes,
            entry.parent,
            entry.signature
        );
    }
}
