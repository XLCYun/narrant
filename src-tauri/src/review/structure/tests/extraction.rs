use super::*;

/// 原生类成员、具名箭头函数和嵌套函数保持独立归属及原文件位置。
#[test]
fn extracts_native_members_nested_functions_and_utf16_locations() {
    let parsed = parse_source(
        SourceLanguage::TypeScript,
        "/** 服务说明 */\r\nexport class Service {\r\n  label: string = '值';\r\n  submit(value: string) { function encode() { return value; } return encode(); }\r\n}\r\nconst render = (值: string) => 值;\r\n",
    )
    .unwrap();
    let file = extract_source("src/service.ts", &parsed);
    let class = file
        .declarations
        .iter()
        .position(|item| item.name == "Service")
        .unwrap();
    let submit = file
        .declarations
        .iter()
        .position(|item| item.name == "submit")
        .unwrap();
    assert_eq!(file.declarations[submit].parent, Some(class));
    assert_eq!(
        file.declarations
            .iter()
            .find(|item| item.name == "encode")
            .unwrap()
            .parent,
        Some(submit)
    );
    assert_eq!(
        file.declarations
            .iter()
            .find(|item| item.name == "label")
            .unwrap()
            .native_kind
            .name,
        StructureNativeName::Field
    );
    let render = file
        .declarations
        .iter()
        .find(|item| item.name == "render")
        .unwrap();
    assert_eq!(render.native_kind.name, StructureNativeName::ArrowFunction);
    assert_eq!(render.name_range.position.start_line, 6);
    assert_eq!(render.name_range.position.start_column, 7);
    assert!(file.declarations[class].documentation.contains("服务说明"));
    assert!(file.complete);
}

/// 子声明及其文档变化不重复计入父级，匿名代码仍归外层函数。
#[test]
fn owned_fingerprints_exclude_named_children_and_their_documentation() {
    let before = extract_source("a.ts", &parse_source(SourceLanguage::TypeScript, "function outer() {\n/** child */\nfunction inner(x: string) { return 'a'; }\nreturn () => consume('b');\n}").unwrap());
    let after = extract_source("a.ts", &parse_source(SourceLanguage::TypeScript, "function outer() {\n/** updated child */\nfunction renamed(x: number) { return 'c'; }\nreturn () => consume('b');\n}").unwrap());
    let old_parent = &before.declarations[0];
    let new_parent = &after.declarations[0];
    assert_eq!(old_parent.signature_tokens, new_parent.signature_tokens);
    assert_eq!(old_parent.body_tokens, new_parent.body_tokens);
    assert_eq!(old_parent.documentation, new_parent.documentation);
    assert_ne!(
        before.declarations[1].body_tokens,
        after.declarations[1].body_tokens
    );
    let callback_call = before
        .occurrences
        .iter()
        .find(|item| item.written_target == "consume")
        .unwrap();
    assert_eq!(callback_call.declaration, Some(0));
    assert!(callback_call.callback_range.is_some());
}

/// 声明外层的导出、默认导出和 ambient 修饰保留到原生签名比较与展示中。
#[test]
fn keeps_export_default_and_declare_wrappers_in_the_own_signature() {
    for (before, after, expected_signature) in [
        (
            "function f() {}",
            "export function f() {}",
            "export function f()",
        ),
        (
            "export function f() {}",
            "export default function f() {}",
            "export default function f()",
        ),
        ("class C {}", "declare class C {}", "declare class C"),
    ] {
        let old = extract_source(
            "a.ts",
            &parse_source(SourceLanguage::TypeScript, before).unwrap(),
        );
        let new = extract_source(
            "a.ts",
            &parse_source(SourceLanguage::TypeScript, after).unwrap(),
        );
        assert!(old.complete && new.complete);
        assert_eq!(old.declarations.len(), 1);
        assert_eq!(new.declarations.len(), 1);
        assert_eq!(old.declarations[0].name, new.declarations[0].name);
        assert_ne!(
            old.declarations[0].signature_tokens,
            new.declarations[0].signature_tokens
        );
        assert_eq!(
            old.declarations[0].body_tokens,
            new.declarations[0].body_tokens
        );
        assert_eq!(new.declarations[0].signature, expected_signature);
    }
}

/// 重新赋值只记录实际写入名称；对象键及右侧引用不污染词法绑定证据。
#[test]
fn retains_assignment_update_destructuring_and_receiver_mutation_evidence() {
    let file = extract_source("a.js", &parse_source(SourceLanguage::JavaScript, "function a() {} function b() {} function run() { a = b; a += b; a++; ({a, c: d} = obj); holder.method = b; a(); }").unwrap());
    assert!(file.complete);
    assert_eq!(
        file.mutations
            .iter()
            .map(|mutation| mutation.name.as_str())
            .collect::<Vec<_>>(),
        ["a", "a", "a", "a", "d", "holder"]
    );
    let call = file
        .occurrences
        .iter()
        .find(|occurrence| occurrence.written_target == "a")
        .unwrap();
    assert!(file
        .mutations
        .iter()
        .all(|mutation| mutation.scope == call.scope
            && mutation.range.bytes.start < call.range.bytes.start));
    assert!(!file
        .mutations
        .iter()
        .any(|mutation| matches!(mutation.name.as_str(), "b" | "c" | "obj" | "method")));
}

/// this 的具体成员、计算属性与普通函数重绑定分别保留，供关系层精准判断污染范围。
#[test]
fn distinguishes_named_computed_and_dynamic_this_member_writes() {
    let file = extract_source("a.ts", &parse_source(SourceLanguage::TypeScript, "class C { run() { this.m = other; this.state.x = value; this[key] = value; (() => { this.m = other; })(); function replace() { this.m = other; } } }").unwrap());
    assert!(file.complete);
    let actual: Vec<_> = file
        .mutations
        .iter()
        .map(|mutation| {
            assert_eq!(mutation.name, "this");
            (mutation.member.clone(), mutation.dynamic_this)
        })
        .collect();
    assert_eq!(
        actual,
        vec![
            (Some(ExtractedMutationMember::Named("m".to_owned())), false),
            (
                Some(ExtractedMutationMember::Named("state".to_owned())),
                false
            ),
            (Some(ExtractedMutationMember::Computed), false),
            (Some(ExtractedMutationMember::Named("m".to_owned())), false),
            (Some(ExtractedMutationMember::Named("m".to_owned())), true),
        ]
    );
}

/// 无声明循环左值会改写已有绑定，新声明的循环变量则只形成独立局部遮蔽。
#[test]
fn records_for_assignment_targets_without_tainting_new_loop_bindings() {
    let file = extract_source("a.js", &parse_source(SourceLanguage::JavaScript, "function a() {} function run() { for (a of values) a(); for (a in object) a(); for (let a of values) a(); for (const a in object) a(); for (var a of values) a(); }").unwrap());
    assert!(file.complete);
    assert_eq!(file.mutations.len(), 2);
    assert!(file
        .mutations
        .iter()
        .all(|mutation| mutation.name == "a" && mutation.member.is_none()));
    assert_eq!(
        file.bindings
            .iter()
            .filter(|binding| binding.name == "a" && binding.kind == ExtractedBindingKind::Local)
            .count(),
        3
    );
}
