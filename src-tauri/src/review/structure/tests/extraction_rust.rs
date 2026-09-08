use super::*;

/// 通过真正的 parser 与提取接口检查 Rust 源码，测试不调用 adapter 的私有规则。
fn rust_file(source: &str) -> ExtractedFile {
    let parsed = parse_source(SourceLanguage::Rust, source).expect("Rust parser 应能加载");
    extract_source("src/fixture.rs", &parsed)
}

/// 具名声明的查找只辅助表达预期，重复声明需要在各自 impl 或父级中明确选择。
fn declaration_named<'a>(file: &'a ExtractedFile, name: &str) -> &'a ExtractedDeclaration {
    let mut declarations = file.declarations.iter().filter(|item| item.name == name);
    let declaration = declarations
        .next()
        .unwrap_or_else(|| panic!("缺少声明 {name}"));
    assert!(
        declarations.next().is_none(),
        "声明 {name} 应在此 fixture 中唯一"
    );
    declaration
}

/// 原生 Rust 类型、成员和关联声明不被压平成普通函数或字符串标签。
#[test]
fn preserves_native_kinds_and_real_tuple_fields() {
    let file = rust_file(
        r#"
pub struct Service<T> { pub value: T }
pub struct Pair(pub String, usize);
enum State { Idle, Count(u64), Record { name: String } }
trait Store {
    type Output;
    const LIMIT: usize;
    fn load(&self) -> Self::Output;
    fn build() -> Self;
    fn boxed(self: Box<Self>) {}
}
mod nested { pub const READY: bool = true; }
mod sibling;
type Alias = State;
static FLAG: bool = false;
extern "C" { fn foreign_fn(); }
"#,
    );
    assert!(file.complete, "{:#?}", file.diagnostics);
    for (name, native) in [
        ("Service", StructureNativeName::Struct),
        ("State", StructureNativeName::Enum),
        ("Idle", StructureNativeName::EnumVariant),
        ("Store", StructureNativeName::Trait),
        ("Output", StructureNativeName::AssociatedType),
        ("LIMIT", StructureNativeName::AssociatedConstant),
        ("load", StructureNativeName::Method),
        ("build", StructureNativeName::AssociatedFunction),
        ("boxed", StructureNativeName::Method),
        ("nested", StructureNativeName::Module),
        ("sibling", StructureNativeName::Module),
        ("READY", StructureNativeName::Constant),
        ("Alias", StructureNativeName::TypeAlias),
        ("FLAG", StructureNativeName::Static),
        ("foreign_fn", StructureNativeName::Function),
    ] {
        assert_eq!(
            declaration_named(&file, name).native_kind.name,
            native,
            "{name}"
        );
    }
    let pair = file
        .declarations
        .iter()
        .position(|item| item.name == "Pair")
        .unwrap();
    let fields: Vec<_> = file
        .declarations
        .iter()
        .filter(|item| item.parent == Some(pair))
        .collect();
    assert_eq!(
        fields
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>(),
        ["0", "1"]
    );
    assert!(fields
        .iter()
        .all(|item| item.native_kind.name == StructureNativeName::Field));
    assert!(fields[0].exported);
    assert!(!fields[1].exported);
    assert_eq!(
        declaration_named(&file, "value").native_kind.name,
        StructureNativeName::Field
    );
}

/// 同名 trait 方法保持各自 impl 身份；方法中的局部函数和类型不是 impl 的直接成员。
#[test]
fn keeps_trait_methods_distinct_and_local_declarations_under_the_method() {
    let file = rust_file(
        r#"
struct Service;
trait Reader { fn visit(&self); }
trait Writer { fn visit(&self); }
impl Reader for Service {
    fn visit(&self) { fn helper() {} struct Local; helper(); }
}
impl Writer for Service { fn visit(&self) { write(); } }
"#,
    );
    assert!(file.complete, "{:#?}", file.diagnostics);
    assert_eq!(
        file.declarations
            .iter()
            .filter(|item| item.name == "visit")
            .count(),
        4
    );
    assert_eq!(file.impl_groups.len(), 2);
    let reader = file
        .impl_groups
        .iter()
        .find(|group| group.trait_name.as_deref() == Some("Reader"))
        .unwrap();
    let writer = file
        .impl_groups
        .iter()
        .find(|group| group.trait_name.as_deref() == Some("Writer"))
        .unwrap();
    assert_eq!(reader.owner, "Service");
    assert_eq!(writer.owner, "Service");
    assert_eq!(reader.members.len(), 1);
    assert_eq!(writer.members.len(), 1);
    let reader_method = reader.members[0];
    let writer_method = writer.members[0];
    assert_ne!(reader_method, writer_method);
    assert_ne!(
        file.declarations[reader_method].impl_group,
        file.declarations[writer_method].impl_group
    );
    assert_eq!(file.declarations[reader_method].name, "visit");
    assert_eq!(file.declarations[writer_method].name, "visit");
    assert_eq!(
        declaration_named(&file, "helper").parent,
        Some(reader_method)
    );
    assert_eq!(
        declaration_named(&file, "helper").native_kind.name,
        StructureNativeName::Function
    );
    assert_eq!(
        declaration_named(&file, "Local").parent,
        Some(reader_method)
    );
}

/// impl 头部与自身文档不会因成员签名、正文或文档修改而重复变化。
#[test]
fn compares_impl_header_and_documentation_independently_of_members() {
    let before = rust_file("struct Service;\n/// implementation docs\nimpl Service {\n/// original method\nfn old(&self) -> u8 { 1 }\n}\n");
    let after = rust_file("struct Service;\n/// implementation docs\nimpl Service {\n/// updated method\nfn new(&self, value: u16) -> u16 { value + 2 }\n}\n");
    let own_docs_changed = rust_file("struct Service;\n/// revised implementation docs\nimpl Service {\n/// original method\nfn old(&self) -> u8 { 1 }\n}\n");
    let old_group = &before.impl_groups[0];
    let new_group = &after.impl_groups[0];
    assert_eq!(old_group.header_tokens, new_group.header_tokens);
    assert_eq!(old_group.documentation, new_group.documentation);
    assert!(old_group.documentation.contains("implementation docs"));
    assert!(!old_group.documentation.contains("original method"));
    assert_ne!(
        old_group.documentation,
        own_docs_changed.impl_groups[0].documentation
    );
    assert_eq!(
        old_group.header_tokens,
        own_docs_changed.impl_groups[0].header_tokens
    );
    assert_ne!(
        declaration_named(&before, "old").body_tokens,
        declaration_named(&after, "new").body_tokens
    );
    assert_ne!(
        declaration_named(&before, "old").documentation,
        declaration_named(&after, "new").documentation
    );
}

/// 字段、枚举成员和泛型参数留在类型作用域，不遮蔽同名文件级声明。
#[test]
fn gives_fields_variants_and_generic_parameters_their_type_scope() {
    let file = rust_file("struct T;\nfn run() {}\nstruct Wrap<T> { run: T }\nenum E { Variant }\nfn caller() { run(); }\n");
    assert!(file.complete, "{:#?}", file.diagnostics);
    let wrap = file
        .declarations
        .iter()
        .position(|item| item.name == "Wrap")
        .unwrap();
    let enum_index = file
        .declarations
        .iter()
        .position(|item| item.name == "E")
        .unwrap();
    let free_run = file
        .declarations
        .iter()
        .find(|item| item.name == "run" && item.native_kind.name == StructureNativeName::Function)
        .unwrap();
    let field = file
        .declarations
        .iter()
        .find(|item| item.name == "run" && item.native_kind.name == StructureNativeName::Field)
        .unwrap();
    assert_eq!(field.parent, Some(wrap));
    assert_ne!(field.scope, free_run.scope);
    assert_eq!(file.scopes[field.scope].declaration, Some(wrap));
    let variant = declaration_named(&file, "Variant");
    assert_eq!(file.scopes[variant.scope].declaration, Some(enum_index));
    let generic = file
        .bindings
        .iter()
        .find(|binding| binding.name == "T" && binding.kind != ExtractedBindingKind::Declaration)
        .expect("类型参数 T 应有遮蔽绑定");
    assert_eq!(file.scopes[generic.scope].declaration, Some(wrap));
    assert_ne!(generic.scope, declaration_named(&file, "T").scope);
    let references: Vec<_> = file
        .occurrences
        .iter()
        .filter(|occurrence| {
            occurrence.kind == StructureRelationshipKind::TypeReference
                && occurrence.written_target == "T"
        })
        .collect();
    assert_eq!(
        references.len(),
        1,
        "只应保留字段类型引用，类型参数声明不是引用"
    );
    assert_eq!(
        references[0].declaration,
        file.declarations.iter().position(
            |item| item.name == "run" && item.native_kind.name == StructureNativeName::Field
        )
    );
}

/// use 的嵌套分组、self、别名和绝对路径展开成明确导入，而星号和下划线不制造绑定。
#[test]
fn flattens_rust_imports_without_inventing_wildcard_or_anonymous_bindings() {
    let file = rust_file(
        r#"
pub use crate::review::{self as review_api, Snapshot, nested::{Reader as LocalReader, *}};
use super::{service::Service, helpers::{self, run}};
use ::std::{self as standard, fmt::Debug};
use foo::Trait as _;
extern crate serde as serde_alias;
"#,
    );
    assert!(file.complete, "{:#?}", file.diagnostics);
    let mut actual: Vec<_> = file
        .imports
        .iter()
        .map(|item| {
            (
                item.module.as_str(),
                item.imported.as_str(),
                item.local.as_str(),
                item.reexport,
            )
        })
        .collect();
    let mut expected = vec![
        ("crate::review", "self", "review_api", true),
        ("crate::review", "Snapshot", "Snapshot", true),
        ("crate::review::nested", "Reader", "LocalReader", true),
        ("crate::review::nested", "*", "*", true),
        ("super::service", "Service", "Service", false),
        ("super::helpers", "self", "helpers", false),
        ("super::helpers", "run", "run", false),
        ("::std", "self", "standard", false),
        ("::std::fmt", "Debug", "Debug", false),
        ("foo", "Trait", "_", false),
        ("serde", "self", "serde_alias", false),
    ];
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(actual, expected);
    assert!(!file
        .bindings
        .iter()
        .any(|binding| matches!(binding.name.as_str(), "*" | "_")));
    for name in [
        "review_api",
        "Snapshot",
        "LocalReader",
        "Service",
        "helpers",
        "run",
        "standard",
        "Debug",
        "serde_alias",
    ] {
        assert!(file.bindings.iter().any(|binding| binding.kind == ExtractedBindingKind::Import && binding.name == name), "缺少导入绑定 {name}");
    }
}

/// 格式空白和具名子函数修改不改变父函数自身，raw string 的内部空格必须保留。
#[test]
fn keeps_raw_literal_changes_while_ignoring_safe_formatting_and_named_children() {
    let before =
        rust_file(r###"fn outer(value: &str) { fn helper() -> u8 { 1 } consume(r##"a b"##); }"###);
    let formatted = rust_file(
        r###"
fn outer ( value : &str )
{
    fn helper ( ) -> u8 { 1 }
    consume ( r##"a b"## ) ;
}
"###,
    );
    let child_changed = rust_file(
        r###"fn outer(value: &str) { fn renamed() -> u16 { 20 } consume(r##"a b"##); }"###,
    );
    let literal_changed =
        rust_file(r###"fn outer(value: &str) { fn helper() -> u8 { 1 } consume(r##"a  b"##); }"###);
    let original = declaration_named(&before, "outer");
    let reformatted = declaration_named(&formatted, "outer");
    assert_eq!(original.signature_tokens, reformatted.signature_tokens);
    assert_eq!(original.body_tokens, reformatted.body_tokens);
    assert_eq!(
        original.signature_tokens,
        declaration_named(&child_changed, "outer").signature_tokens
    );
    assert_eq!(
        original.body_tokens,
        declaration_named(&child_changed, "outer").body_tokens
    );
    assert_ne!(
        original.body_tokens,
        declaration_named(&literal_changed, "outer").body_tokens
    );
    assert_ne!(
        declaration_named(&before, "helper").signature_tokens,
        declaration_named(&child_changed, "renamed").signature_tokens
    );
}

/// 字段属性和可见性属于该字段；只改字段时不重复改动 tuple struct 的自身指纹。
#[test]
fn keeps_tuple_field_attributes_and_visibility_in_the_field_signature() {
    let before = rust_file("struct Pair(#[cfg(feature = \"x\")] pub String, usize);\n");
    let after = rust_file("struct Pair(#[cfg(feature = \"y\")] pub(crate) String, usize);\n");
    let old_field = declaration_named(&before, "0");
    let new_field = declaration_named(&after, "0");
    assert_ne!(old_field.signature_tokens, new_field.signature_tokens);
    assert!(old_field.signature.contains("#[cfg(feature = \"x\")]"));
    assert!(old_field.signature.contains("pub String"));
    assert!(new_field.signature.contains("pub(crate) String"));
    assert_eq!(
        declaration_named(&before, "Pair").signature_tokens,
        declaration_named(&after, "Pair").signature_tokens
    );
    assert_eq!(
        declaration_named(&before, "Pair").body_tokens,
        declaration_named(&after, "Pair").body_tokens
    );
}

/// Rust 内部文档属于所在模块，外部文档属于下一声明，两者修改不会互相污染。
#[test]
fn assigns_inner_and_outer_documentation_to_their_rust_owners() {
    let before = rust_file("/// outer module docs\nmod api {\n//! inner module docs\n/// function docs\nfn action() {}\n}\n");
    let after = rust_file("/// outer module docs\nmod api {\n//! revised inner module docs\n/// function docs\nfn action() {}\n}\n");
    let module = declaration_named(&before, "api");
    let action = declaration_named(&before, "action");
    assert!(module.documentation.contains("outer module docs"));
    assert!(module.documentation.contains("inner module docs"));
    assert!(!module.documentation.contains("function docs"));
    assert!(action.documentation.contains("function docs"));
    assert!(!action.documentation.contains("inner module docs"));
    assert_ne!(
        module.documentation,
        declaration_named(&after, "api").documentation
    );
    assert_eq!(
        action.documentation,
        declaration_named(&after, "action").documentation
    );
}

/// 一个函数的恢复错误不会抹掉完整的文件级兄弟声明，但该文件不能声称完整覆盖。
#[test]
fn preserves_a_trusted_sibling_when_another_function_has_parse_errors() {
    let file = rust_file("fn broken() { let = ; }\nfn healthy() { work(); }\n");
    assert!(!file.complete);
    assert!(!file.diagnostics.is_empty());
    assert!(!declaration_named(&file, "broken").reliable);
    assert!(declaration_named(&file, "healthy").reliable);
    let healthy = file
        .declarations
        .iter()
        .position(|item| item.name == "healthy")
        .unwrap();
    let work = file
        .occurrences
        .iter()
        .find(|occurrence| {
            occurrence.kind == StructureRelationshipKind::Calls
                && occurrence.written_target == "work"
        })
        .unwrap();
    assert_eq!(work.declaration, Some(healthy));
    assert!(work.reliable);
    assert_eq!(work.range.position.start_line, 2);
}

/// trait 的父约束与 impl trait 各归对应关系种类，普通参数类型仍是类型引用。
#[test]
fn distinguishes_trait_inheritance_implementation_and_ordinary_type_references() {
    let file = rust_file("trait Parent {}\ntrait Child: Parent {}\nstruct Service;\nimpl Child for Service {}\nfn accept(value: Service) {}\n");
    assert!(file.complete, "{:#?}", file.diagnostics);
    let child = file
        .declarations
        .iter()
        .position(|item| item.name == "Child")
        .unwrap();
    let extends: Vec<_> = file
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.kind == StructureRelationshipKind::Extends)
        .collect();
    assert_eq!(extends.len(), 1);
    assert_eq!(extends[0].written_target, "Parent");
    assert_eq!(extends[0].declaration, Some(child));
    let implementations: Vec<_> = file
        .occurrences
        .iter()
        .filter(|occurrence| occurrence.kind == StructureRelationshipKind::Implements)
        .collect();
    assert_eq!(implementations.len(), 1);
    assert_eq!(implementations[0].written_target, "Child");
    assert!(implementations[0].impl_group.is_some());
    assert!(!file.occurrences.iter().any(|occurrence| occurrence.kind
        == StructureRelationshipKind::TypeReference
        && matches!(occurrence.written_target.as_str(), "Parent" | "Child")));
    let accept = file
        .declarations
        .iter()
        .position(|item| item.name == "accept")
        .unwrap();
    assert!(file.occurrences.iter().any(|occurrence| occurrence.kind
        == StructureRelationshipKind::TypeReference
        && occurrence.written_target == "Service"
        && occurrence.declaration == Some(accept)));
}

/// async block 和闭包中的调用保留匿名执行边界，不冒充父函数直接执行的调用。
#[test]
fn records_deferred_async_and_closure_execution_ranges() {
    let source = "fn work() {}\nfn outer() {\nwork();\nlet future = async move { work(); };\nlet callback = || work();\n}\n";
    let file = rust_file(source);
    assert!(file.complete, "{:#?}", file.diagnostics);
    let outer = file
        .declarations
        .iter()
        .position(|item| item.name == "outer")
        .unwrap();
    let calls: Vec<_> = file
        .occurrences
        .iter()
        .filter(|occurrence| {
            occurrence.kind == StructureRelationshipKind::Calls
                && occurrence.written_target == "work"
        })
        .collect();
    assert_eq!(calls.len(), 3);
    assert!(calls.iter().all(|call| call.declaration == Some(outer)));
    let direct = calls
        .iter()
        .find(|call| call.range.position.start_line == 3)
        .unwrap();
    let deferred = calls
        .iter()
        .find(|call| call.range.position.start_line == 4)
        .unwrap();
    let closure = calls
        .iter()
        .find(|call| call.range.position.start_line == 5)
        .unwrap();
    assert!(direct.callback_range.is_none());
    assert_eq!(
        &source[deferred
            .callback_range
            .as_ref()
            .expect("async block 应保留延迟执行范围")
            .bytes
            .clone()],
        "async move { work(); }"
    );
    assert_eq!(
        &source[closure
            .callback_range
            .as_ref()
            .expect("闭包应保留匿名执行范围")
            .bytes
            .clone()],
        "|| work()"
    );
}

/// 模块路径属性按 Rust 字符串语义解码，raw string 的反斜杠不会被再次转义。
#[test]
fn decodes_explicit_module_paths_from_normal_and_raw_attributes() {
    for (source, expected) in [
        (
            r##"#[path = "tests/child.rs"] mod child;"##,
            "tests/child.rs",
        ),
        (
            r##"#[path = "tests/\x66\u{006F}o.rs"] mod child;"##,
            "tests/foo.rs",
        ),
        (
            r##"#[path = "tests/\u{1F680}.rs"] mod child;"##,
            "tests/🚀.rs",
        ),
        (
            r##"#[path = "tests/quoted\"name.rs"] mod child;"##,
            "tests/quoted\"name.rs",
        ),
        (
            r##"#[path = "tests\\child.rs"] mod child;"##,
            "tests\\child.rs",
        ),
        (
            r##"#[path = "tests/line\n\r\t.rs"] mod child;"##,
            "tests/line\n\r\t.rs",
        ),
        (
            r###"#[path = r#"tests/child.rs"#] mod child;"###,
            "tests/child.rs",
        ),
        (
            r###"#[path = r#"tests\child.rs"#] mod child;"###,
            "tests\\child.rs",
        ),
        (
            r##"#[r#path = "tests/raw_identifier.rs"] mod child;"##,
            "tests/raw_identifier.rs",
        ),
        (
            "#[path = \"tests/spaced.rs\"]\n\nmod child;",
            "tests/spaced.rs",
        ),
        (
            "#[path = \"tests/commented.rs\"]\n// explanation\n\nmod child;",
            "tests/commented.rs",
        ),
    ] {
        let file = rust_file(source);
        assert!(file.complete, "{source}: {:#?}", file.diagnostics);
        assert_eq!(
            declaration_named(&file, "child").module_path,
            Some(ExtractedModulePath::Explicit(expected.to_owned())),
            "{source}"
        );
    }
}

/// 无路径属性的模块使用默认规则，普通文档及不涉及 path 的条件属性不改变它。
#[test]
fn keeps_default_module_paths_when_attributes_do_not_choose_a_source() {
    let file = rust_file(
        r#"
mod plain;
#[doc = "path is only documentation"]
#[cfg_attr(feature = "x", allow(dead_code))]
mod documented;
#[cfg_attr(feature = "x", doc = "path")]
mod conditionally_documented;
fn ordinary() {}
"#,
    );
    assert!(file.complete, "{:#?}", file.diagnostics);
    for name in ["plain", "documented", "conditionally_documented"] {
        assert_eq!(
            declaration_named(&file, name).module_path,
            Some(ExtractedModulePath::Default),
            "{name}"
        );
    }
    assert_eq!(declaration_named(&file, "ordinary").module_path, None);
}

/// 条件路径、冲突路径和无法可靠解码的属性必须保持未知，不能退回猜测文件名。
#[test]
fn leaves_conditional_conflicting_and_undecodable_module_paths_unknown() {
    for source in [
        r#"#[cfg_attr(feature = "x", path = "one.rs")] mod child;"#,
        r#"#[cfg_attr(feature = "x", cfg_attr(feature = "y", path = "two.rs"))] mod child;"#,
        r#"#[path = "one.rs"] #[path = "two.rs"] mod child;"#,
        r#"#[path = "one.rs"] #[cfg_attr(feature = "x", path = "two.rs")] mod child;"#,
        r#"#[path = concat!("tests/", "child.rs")] mod child;"#,
        r#"#[path = b"child.rs"] mod child;"#,
        r#"#[path = "tests/\xFF.rs"] mod child;"#,
        r#"#[path = "tests/\u{D800}.rs"] mod child;"#,
        r#"#[path = "tests/\u{4F60_597D}.rs"] mod child;"#,
        "#[path = \"tests/\\\nchild.rs\"] mod child;",
    ] {
        let file = rust_file(source);
        assert_eq!(
            declaration_named(&file, "child").module_path,
            Some(ExtractedModulePath::Unknown),
            "{source}"
        );
    }
}
