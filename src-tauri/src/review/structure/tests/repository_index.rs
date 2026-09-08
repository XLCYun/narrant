use super::*;
use crate::review::structure::{extract_source, parse_source, SourceLanguage};

/// 测试从真实语法入口构造单侧文件，避免伪造词法作用域。
fn source(path: &str, text: &str) -> ExtractedFile {
    let language = if path.ends_with(".rs") {
        SourceLanguage::Rust
    } else {
        SourceLanguage::TypeScript
    };
    extract_source(path, &parse_source(language, text).unwrap())
}

/// 显式别名跨文件绑定，而同名参数会阻止外层导入被误用。
#[test]
fn explicit_alias_resolves_and_parameter_shadows_import() {
    let files = vec![
        source("src/service.ts", "export class Service {}"),
        source("src/main.ts", "import { Service as Alias } from './service'; function run(Alias: unknown) { Alias(); }"),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    assert_eq!(
        index.resolve_type("src/main.ts", 0, "Alias"),
        BindingResolution::Local(("src/service.ts".into(), 0))
    );
    let occurrence = files[1]
        .occurrences
        .iter()
        .find(|item| item.written_target == "Alias")
        .unwrap();
    assert!(matches!(
        index.resolve_type("src/main.ts", occurrence.scope, "Alias"),
        BindingResolution::Unknown(_)
    ));
}

/// 别名来自各侧固定配置，同样的导入可绑定到不同文件。
#[test]
fn per_side_tsconfig_alias_and_explicit_reexports() {
    let files = vec![
        source("src/old.ts", "export class Service {}"),
        source("src/new.ts", "export class Service {}"),
        source(
            "src/barrel.ts",
            "export { Service as Renamed } from '@/service';",
        ),
        source(
            "src/main.ts",
            "import { Renamed as Target } from './barrel';",
        ),
    ];
    let old = HashMap::from([(
        "tsconfig.json".into(),
        r#"{ // old snapshot
        "compilerOptions": { "baseUrl": ".", "paths": { "@/service": ["src/old.ts"], }, },
    }"#
        .into(),
    )]);
    let new = HashMap::from([(
        "tsconfig.json".into(),
        r#"{"compilerOptions":{"baseUrl":".","paths":{"@/service":["src/new.ts"]}}}"#.into(),
    )]);
    assert_eq!(
        RepositoryIndex::new(&files, &old).resolve_type("src/main.ts", 0, "Target"),
        BindingResolution::Local(("src/old.ts".into(), 0))
    );
    assert_eq!(
        RepositoryIndex::new(&files, &new).resolve_type("src/main.ts", 0, "Target"),
        BindingResolution::Local(("src/new.ts".into(), 0))
    );
}

/// 未导入的同名类型绝不从全仓库短名表补齐。
#[test]
fn same_basename_without_import_remains_unknown() {
    let files = vec![
        source("src/other.ts", "export class Service {}"),
        source("src/main.ts", "function run() {}"),
    ];
    assert!(matches!(
        RepositoryIndex::new(&files, &HashMap::new()).resolve_type("src/main.ts", 0, "Service"),
        BindingResolution::Unknown(_)
    ));
}

/// 命名默认导出与 namespace import 都沿显式导出事实解析。
#[test]
fn namespace_and_named_default_exports() {
    let files = vec![
        source(
            "src/lib.ts",
            "export default class Service {} export class Named {}",
        ),
        source(
            "src/main.ts",
            "import Default from './lib'; import * as lib from './lib';",
        ),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    assert_eq!(
        index.resolve_type("src/main.ts", 0, "Default"),
        BindingResolution::Local(("src/lib.ts".into(), 0))
    );
    assert_eq!(
        index.resolve_type("src/main.ts", 0, "lib.Named"),
        BindingResolution::Local(("src/lib.ts".into(), 1))
    );
}

/// import 类型重载不能依赖遍历顺序选某个定义。
#[test]
fn overloads_are_candidates_and_wildcards_do_not_prove_exports() {
    let files = vec![
        source("src/lib.ts", "export function work(a: string): string; export function work(a: number): number; export function work(a: unknown) { return a; }"),
        source("src/barrel.ts", "export * from './lib';"),
        source("src/main.ts", "import { work } from './lib'; import { work as indirect } from './barrel';"),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    assert!(
        matches!(index.resolve_type("src/main.ts", 0, "work"), BindingResolution::Candidates(keys) if keys.len() == 3)
    );
    assert!(matches!(
        index.resolve_type("src/main.ts", 0, "indirect"),
        BindingResolution::Unknown(_)
    ));
}

/// 配置缺失及别名循环不会伪装成 npm 外部包。
#[test]
fn missing_config_and_cyclic_reexports_are_unknown() {
    let files = vec![
        source("src/main.ts", "import { Thing } from '@/thing';"),
        source("src/a.ts", "export { Thing } from './b';"),
        source("src/b.ts", "export { Thing } from './a';"),
        source("src/use.ts", "import { Thing } from './a';"),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    assert_eq!(
        index.resolve_type("src/main.ts", 0, "Thing"),
        BindingResolution::Unknown(StructureDiagnosticReason::ConfigurationUnavailable)
    );
    assert!(matches!(
        index.resolve_type("src/use.ts", 0, "Thing"),
        BindingResolution::Unknown(_)
    ));
}

/// Rust use 别名、crate/self/super 与跨文件 impl 均绑定到原类型身份。
#[test]
fn rust_module_paths_aliases_and_cross_file_members() {
    let files = vec![
        source("src/lib.rs", "mod model; mod worker;"),
        source("src/model.rs", "pub struct Service; pub trait Run { fn run(&self); }"),
        source("src/worker.rs", "use crate::model::{Service as Alias, Run}; impl Alias { pub fn create() -> Self { Self } pub fn save(&self) {} } impl Run for Alias { fn run(&self) {} }"),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    assert_eq!(
        index.resolve_type("src/worker.rs", 0, "Alias"),
        BindingResolution::Local(("src/model.rs".into(), 0))
    );
    assert_eq!(
        index.resolve_type("src/worker.rs", 0, "super::model::Service"),
        BindingResolution::Local(("src/model.rs".into(), 0))
    );
    let create = files[2]
        .declarations
        .iter()
        .position(|item| item.name == "create")
        .unwrap();
    assert_eq!(
        index.resolve_type("src/worker.rs", 0, "Alias::create"),
        BindingResolution::Local(("src/worker.rs".into(), create))
    );
}

/// 内联模块的同名类型和其他 crate 的同名文件不能串线。
#[test]
fn rust_inline_modules_and_crates_stay_separate() {
    let files = vec![
        source(
            "one/src/lib.rs",
            "mod a { pub struct Thing; } mod b { pub struct Thing; }",
        ),
        source("two/src/lib.rs", "pub struct Thing;"),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    let a = files[0]
        .declarations
        .iter()
        .position(|item| item.qualified_name.ends_with("a::Thing"))
        .unwrap();
    let b = files[0]
        .declarations
        .iter()
        .position(|item| item.qualified_name.ends_with("b::Thing"))
        .unwrap();
    assert_eq!(
        index.resolve_type("one/src/lib.rs", 0, "crate::a::Thing"),
        BindingResolution::Local(("one/src/lib.rs".into(), a))
    );
    assert_eq!(
        index.resolve_type("one/src/lib.rs", 0, "crate::b::Thing"),
        BindingResolution::Local(("one/src/lib.rs".into(), b))
    );
    assert!(matches!(
        index.resolve_type("one/src/lib.rs", 0, "Thing"),
        BindingResolution::Unknown(_)
    ));
}

/// 已声明的具体接收者类型可绑定，未知对象与 dyn trait 不能制造确定方法调用。
#[test]
fn typed_receiver_resolves_but_dynamic_receiver_does_not() {
    let files = vec![source("src/main.ts", "class Service { save() {} } function run(s: Service, dynamic: unknown) { s.save(); dynamic.save(); }")];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    let save = files[0]
        .declarations
        .iter()
        .position(|item| item.name == "save")
        .unwrap();
    for occurrence in files[0]
        .occurrences
        .iter()
        .filter(|item| item.kind == StructureRelationshipKind::Calls)
    {
        let result = index.resolve_occurrence(
            &files[0].path,
            occurrence.scope,
            &occurrence.written_target,
            occurrence.range.bytes.start,
            occurrence.kind,
        );
        if occurrence.written_target == "s.save" {
            assert_eq!(
                result,
                BindingResolution::Local(("src/main.ts".into(), save))
            );
        }
        if occurrence.written_target == "dynamic.save" {
            assert!(matches!(result, BindingResolution::Unknown(_)));
        }
    }
}

/// Rust let 只遮蔽声明之后的名称；同作用域重复 let 使用最近绑定。
#[test]
fn rust_sequential_local_shadowing_is_respected() {
    let files = vec![source(
        "src/lib.rs",
        "fn work() {} fn run() { work(); let work = 1; work(); }",
    )];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    let calls: Vec<_> = files[0]
        .occurrences
        .iter()
        .filter(|item| {
            item.kind == StructureRelationshipKind::Calls && item.written_target == "work"
        })
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(matches!(
        index.resolve_occurrence(
            "src/lib.rs",
            calls[0].scope,
            "work",
            calls[0].range.bytes.start,
            calls[0].kind
        ),
        BindingResolution::Local(_)
    ));
    assert!(matches!(
        index.resolve_occurrence(
            "src/lib.rs",
            calls[1].scope,
            "work",
            calls[1].range.bytes.start,
            calls[1].kind
        ),
        BindingResolution::Unknown(_)
    ));
}

/// 没有依赖证据的 Rust 首段不是外部包，当前 Cargo 的显式依赖可以形成外部身份。
#[test]
fn rust_external_targets_require_current_side_dependency_evidence() {
    let files = vec![source(
        "src/lib.rs",
        "use serde::Serialize as Encode; fn run() {}",
    )];
    let config = HashMap::from([(
        "Cargo.toml".into(),
        "[package]\nname = \"app\"\n[dependencies]\nserde = \"1\"\n".into(),
    )]);
    let index = RepositoryIndex::new(&files, &config);
    assert_eq!(
        index.resolve_type("src/lib.rs", 0, "Encode"),
        BindingResolution::External {
            package_name: "serde".into(),
            name: "serde::Serialize".into()
        }
    );
    assert!(matches!(
        index.resolve_type("src/lib.rs", 0, "Missing::method"),
        BindingResolution::Unknown(_)
    ));
    assert!(matches!(
        RepositoryIndex::new(&files, &HashMap::new()).resolve_type("src/lib.rs", 0, "Encode"),
        BindingResolution::Unknown(_)
    ));
}

/// type-only import 不提供运行时值绑定，类型限定调用也不能伪造实例方法。
#[test]
fn type_only_import_and_instance_method_as_static_are_not_resolved_calls() {
    let files = vec![source("src/lib.ts", "export class Service { save() {} static create() {} }"), source("src/main.ts", "import type { Service as TypeOnly } from './lib'; import { Service } from './lib'; function run() { TypeOnly(); Service.save(); }")];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    let calls: Vec<_> = files[1]
        .occurrences
        .iter()
        .filter(|item| item.kind == StructureRelationshipKind::Calls)
        .collect();
    assert_eq!(calls.len(), 2);
    for call in calls {
        assert!(matches!(
            index.resolve_occurrence(
                "src/main.ts",
                call.scope,
                &call.written_target,
                call.range.bytes.start,
                call.kind
            ),
            BindingResolution::Unknown(_)
        ));
    }
}

/// lib 与 binary 的根声明即使同名也不共用 crate 命名空间。
#[test]
fn rust_library_and_binary_roots_do_not_create_false_candidates() {
    let files = vec![
        source("src/lib.rs", "pub struct Thing; mod worker;"),
        source("src/main.rs", "struct Thing; fn main() {}"),
        source("src/worker.rs", "use crate::Thing;"),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    assert_eq!(
        index.resolve_type("src/worker.rs", 0, "Thing"),
        BindingResolution::Local(("src/lib.rs".into(), 0))
    );
    assert_eq!(
        index.resolve_type("src/main.rs", 0, "crate::Thing"),
        BindingResolution::Local(("src/main.rs".into(), 0))
    );
}

/// #[path] 的物理目录不改变逻辑模块层级，super 回到声明该模块的真实父模块。
#[test]
fn rust_path_attribute_uses_logical_parent_for_super() {
    let files = vec![
        source(
            "src/lib.rs",
            "pub fn calls() {} #[path = \"tests/helpers.rs\"] mod helpers;",
        ),
        source(
            "src/tests/helpers.rs",
            "use super::calls; pub fn run() { calls(); }",
        ),
        source("src/tests.rs", "pub fn calls() {}"),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    assert_eq!(
        index.resolve_type("src/tests/helpers.rs", 0, "calls"),
        BindingResolution::Local(("src/lib.rs".into(), 0))
    );
    assert_eq!(
        index.resolve_type("src/lib.rs", 0, "crate::helpers::run"),
        BindingResolution::Local(("src/tests/helpers.rs".into(), 0))
    );
}

/// cfg_attr 路径和未声明的物理文件不猜默认模块归属，即便旁边存在同名 tests.rs。
#[test]
fn conditional_path_and_unlinked_files_keep_module_resolution_unknown() {
    let files = vec![
        source("src/lib.rs", "pub fn calls() {} #[cfg_attr(test, path = \"tests/helpers.rs\")] mod helpers; mod tests;"),
        source("src/helpers.rs", "pub struct Default;"),
        source("src/tests.rs", "pub fn calls() {}"),
        source("src/tests/helpers.rs", "use super::calls; pub fn run() { calls(); }"),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    assert!(matches!(
        index.resolve_type("src/tests/helpers.rs", 0, "calls"),
        BindingResolution::Unknown(_)
    ));
    assert!(matches!(
        index.resolve_type("src/lib.rs", 0, "crate::helpers::Default"),
        BindingResolution::Unknown(_)
    ));
}

/// 自定义 Cargo lib 路径同样可提供 crate 根，而未声明的默认 lib 不会混入。
#[test]
fn custom_cargo_library_path_is_the_actual_module_root() {
    let files = vec![
        source("core/entry.rs", "pub struct Thing; mod worker;"),
        source("core/worker.rs", "use crate::Thing;"),
        source("src/lib.rs", "pub struct Thing;"),
    ];
    let config = HashMap::from([(
        "Cargo.toml".into(),
        "[package]\nname = \"app\"\n[lib]\npath = \"core/entry.rs\"\n".into(),
    )]);
    let index = RepositoryIndex::new(&files, &config);
    assert_eq!(
        index.resolve_type("core/worker.rs", 0, "Thing"),
        BindingResolution::Local(("core/entry.rs".into(), 0))
    );
    assert!(matches!(
        index.resolve_type("src/lib.rs", 0, "crate::Thing"),
        BindingResolution::Unknown(_)
    ));
}

/// 显式重导出循环与循环 path include 都只会留下未知，不递归无限展开。
#[test]
fn rust_reexport_and_module_include_cycles_are_bounded() {
    let files = vec![
        source("src/lib.rs", "mod a; mod b; use crate::a::Thing;"),
        source("src/a.rs", "pub use crate::b::Thing;"),
        source("src/b.rs", "pub use crate::a::Thing;"),
    ];
    assert!(matches!(
        RepositoryIndex::new(&files, &HashMap::new()).resolve_type("src/lib.rs", 0, "Thing"),
        BindingResolution::Unknown(_)
    ));
    let cyclic = vec![source("src/lib.rs", "#[path = \"lib.rs\"] mod nested;")];
    assert!(matches!(
        RepositoryIndex::new(&cyclic, &HashMap::new()).resolve_type(
            "src/lib.rs",
            0,
            "crate::nested::Missing"
        ),
        BindingResolution::Unknown(_)
    ));
}

/// Rust 默认 prelude 的 trait 可绑定到真实 core 名称，no_std 仍保留这些核心 trait。
#[test]
fn rust_core_prelude_resolves_default_clone_and_copy() {
    let files = vec![source(
        "src/lib.rs",
        "#![no_std]\nstruct Item; impl Default for Item { fn default() -> Self { Self } }",
    )];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    for (name, target) in [
        ("Default", "core::default::Default"),
        ("Clone", "core::clone::Clone"),
        ("Copy", "core::marker::Copy"),
    ] {
        assert_eq!(
            index.resolve_type("src/lib.rs", 0, name),
            BindingResolution::External {
                package_name: "core".into(),
                name: target.into()
            }
        );
    }
}

/// 显式同名 trait 优先于 prelude；禁用隐式prelude及有可能遮蔽的glob不猜核心目标。
#[test]
fn custom_trait_and_disabled_or_shadowed_prelude_are_respected() {
    let local = vec![source(
        "src/lib.rs",
        "trait Default {} struct Item; impl Default for Item {}",
    )];
    assert_eq!(
        RepositoryIndex::new(&local, &HashMap::new()).resolve_type("src/lib.rs", 0, "Default"),
        BindingResolution::Local(("src/lib.rs".into(), 0))
    );
    let disabled = vec![
        source("src/lib.rs", "#![no_implicit_prelude]\nmod child;"),
        source("src/child.rs", "struct Item; impl Default for Item {}"),
    ];
    assert!(matches!(
        RepositoryIndex::new(&disabled, &HashMap::new()).resolve_type("src/child.rs", 0, "Default"),
        BindingResolution::Unknown(_)
    ));
    let glob = vec![source(
        "src/lib.rs",
        "mod custom { pub trait Default {} } use custom::*; struct Item; impl Default for Item {}",
    )];
    assert_eq!(
        RepositoryIndex::new(&glob, &HashMap::new()).resolve_type("src/lib.rs", 0, "Default"),
        BindingResolution::Local((
            "src/lib.rs".into(),
            glob[0]
                .declarations
                .iter()
                .position(|item| item.name == "Default")
                .unwrap()
        ))
    );
}

/// 未证明trait可见性时保留成员候选；明确的固有方法优先于同名trait成员。
#[test]
fn rust_trait_members_are_candidates_without_dispatch_evidence() {
    let files = vec![source("src/lib.rs", "struct Item; trait Run { fn run(&self); } impl Run for Item { fn run(&self) {} } impl Item { fn save(&self) {} }")];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    assert!(
        matches!(index.resolve_member(BindingResolution::Local(("src/lib.rs".into(), 0)), "run"), BindingResolution::Candidates(keys) if keys.len() == 1)
    );
    assert!(matches!(
        index.resolve_member(BindingResolution::Local(("src/lib.rs".into(), 0)), "save"),
        BindingResolution::Local(_)
    ));
}

/// 原函数绑定被赋值后，静态分析不能继续把调用确定地连接到原声明或猜测右侧函数。
#[test]
fn reassigned_function_binding_is_unknown_instead_of_original_declaration() {
    let files = vec![source(
        "src/main.ts",
        "function a() {} function b() {} function run() { a = b; a(); }",
    )];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    let call = files[0]
        .occurrences
        .iter()
        .find(|item| item.kind == StructureRelationshipKind::Calls && item.written_target == "a")
        .unwrap();
    assert!(matches!(
        index.resolve_occurrence(
            "src/main.ts",
            call.scope,
            "a",
            call.range.bytes.start,
            call.kind
        ),
        BindingResolution::Unknown(_)
    ));
}

/// 闭包可能在调用前执行，源码顺序不证明外层函数绑定未被重写。
#[test]
fn closure_mutation_invalidates_the_same_binding_without_source_order_guess() {
    let files = vec![source(
        "src/main.ts",
        "function a() {} function b() {} function run() { a(); return () => { a = b; }; }",
    )];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    let call = files[0]
        .occurrences
        .iter()
        .find(|item| item.kind == StructureRelationshipKind::Calls && item.written_target == "a")
        .unwrap();
    assert!(matches!(
        index.resolve_occurrence(
            "src/main.ts",
            call.scope,
            "a",
            call.range.bytes.start,
            call.kind
        ),
        BindingResolution::Unknown(_)
    ));
}

/// 同名局部函数的写入只污染该词法绑定，外层函数仍能明确解析。
#[test]
fn assignment_to_shadowing_local_binding_does_not_taint_outer_function() {
    let files = vec![source(
        "src/main.ts",
        "function a() {} function b() {} function run() { { function a() {} a = b; a(); } a(); }",
    )];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    let calls: Vec<_> = files[0]
        .occurrences
        .iter()
        .filter(|item| item.kind == StructureRelationshipKind::Calls && item.written_target == "a")
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(matches!(
        index.resolve_occurrence(
            "src/main.ts",
            calls[0].scope,
            "a",
            calls[0].range.bytes.start,
            calls[0].kind
        ),
        BindingResolution::Unknown(_)
    ));
    assert_eq!(
        index.resolve_occurrence(
            "src/main.ts",
            calls[1].scope,
            "a",
            calls[1].range.bytes.start,
            calls[1].kind
        ),
        BindingResolution::Local(("src/main.ts".into(), 0))
    );
}

/// 导入是源模块的实时绑定，源函数被重写后导入别名也不能继续确定地指向原函数。
#[test]
fn exported_rebinding_invalidates_an_explicit_import_alias() {
    let files = vec![
        source(
            "src/lib.ts",
            "export function a() {} function b() {} export function replace() { a = b; }",
        ),
        source(
            "src/main.ts",
            "import { a as imported } from './lib'; function run() { imported(); }",
        ),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    let call = files[1]
        .occurrences
        .iter()
        .find(|item| {
            item.kind == StructureRelationshipKind::Calls && item.written_target == "imported"
        })
        .unwrap();
    assert!(matches!(
        index.resolve_occurrence(
            "src/main.ts",
            call.scope,
            "imported",
            call.range.bytes.start,
            call.kind
        ),
        BindingResolution::Unknown(_)
    ));
}

/// 函数表达式的内部自名是另一条绑定，重写外部变量不能污染仍存在的递归函数对象。
#[test]
fn rebinding_variable_does_not_poison_function_expression_self_name() {
    let files = vec![source(
        "src/main.ts",
        "function other() {} let run = function retry() { retry(); }; run = other; run();",
    )];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    let recursive = files[0]
        .occurrences
        .iter()
        .find(|item| {
            item.kind == StructureRelationshipKind::Calls && item.written_target == "retry"
        })
        .unwrap();
    let reassigned = files[0]
        .occurrences
        .iter()
        .find(|item| item.kind == StructureRelationshipKind::Calls && item.written_target == "run")
        .unwrap();
    assert!(matches!(
        index.resolve_occurrence(
            "src/main.ts",
            recursive.scope,
            "retry",
            recursive.range.bytes.start,
            recursive.kind
        ),
        BindingResolution::Local(_)
    ));
    assert!(matches!(
        index.resolve_occurrence(
            "src/main.ts",
            reassigned.scope,
            "run",
            reassigned.range.bytes.start,
            reassigned.kind
        ),
        BindingResolution::Unknown(_)
    ));
}

/// 无声明for-of左值会重绑定原函数，循环里的调用不能冒充原函数调用。
#[test]
fn assignment_loop_target_invalidates_original_function_binding() {
    for text in [
        "function a() {} function b() {} function run() { for (a of [b]) { a(); } a(); }",
        "function a() {} function b() {} function run() { for (a in { key: 1 }) { a(); } a(); }",
    ] {
        let files = vec![source("src/main.ts", text)];
        let index = RepositoryIndex::new(&files, &HashMap::new());
        let calls: Vec<_> = files[0]
            .occurrences
            .iter()
            .filter(|item| {
                item.kind == StructureRelationshipKind::Calls && item.written_target == "a"
            })
            .collect();
        assert_eq!(calls.len(), 2);
        for call in calls {
            assert!(matches!(
                index.resolve_occurrence(
                    "src/main.ts",
                    call.scope,
                    "a",
                    call.range.bytes.start,
                    call.kind
                ),
                BindingResolution::Unknown(_)
            ));
        }
    }
}

/// 带let的for-of只引入局部绑定，不能污染循环之后的外层同名函数。
#[test]
fn declared_loop_target_does_not_rebind_outer_function() {
    let files = vec![source(
        "src/main.ts",
        "function a() {} function b() {} function run() { for (let a of [b]) { a = b; a(); } a(); }",
    )];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    let calls: Vec<_> = files[0]
        .occurrences
        .iter()
        .filter(|item| item.kind == StructureRelationshipKind::Calls && item.written_target == "a")
        .collect();
    assert_eq!(calls.len(), 2);
    assert!(matches!(
        index.resolve_occurrence(
            "src/main.ts",
            calls[0].scope,
            "a",
            calls[0].range.bytes.start,
            calls[0].kind
        ),
        BindingResolution::Unknown(_)
    ));
    assert_eq!(
        index.resolve_occurrence(
            "src/main.ts",
            calls[1].scope,
            "a",
            calls[1].range.bytes.start,
            calls[1].kind
        ),
        BindingResolution::Local(("src/main.ts".into(), 0))
    );
}

/// 真实父模块glob为分散在子文件的impl提供同一类型与trait身份。
#[test]
fn rust_parent_glob_resolves_cross_file_implementation_owners() {
    let files = vec![
        source("src/lib.rs", "mod lifecycle; mod other { pub trait Known {} }"),
        source("src/lifecycle/mod.rs", "struct ReviewLifecycle; trait Known {} mod presentation; mod refresh; mod workspace; mod structure;"),
        source("src/lifecycle/presentation.rs", "use super::*; impl ReviewLifecycle { fn present(&self) {} } impl Known for ReviewLifecycle {}"),
        source("src/lifecycle/refresh.rs", "use super::*; impl ReviewLifecycle { fn refresh(&self) {} }"),
        source("src/lifecycle/workspace.rs", "use super::*; impl ReviewLifecycle { fn workspace(&self) {} }"),
        source("src/lifecycle/structure.rs", "use super::*; impl ReviewLifecycle { fn structure(&self) {} }"),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    let owner = BindingResolution::Local(("src/lifecycle/mod.rs".into(), 0));
    let known = BindingResolution::Local(("src/lifecycle/mod.rs".into(), 1));
    for file in &files[2..] {
        assert_eq!(index.resolve_type(&file.path, 0, "ReviewLifecycle"), owner);
        assert_eq!(index.resolve_type(&file.path, 0, "Known"), known);
        let other_trait = index.resolve_type(&file.path, 0, "crate::other::Known");
        assert!(matches!(other_trait, BindingResolution::Local(_)));
        assert_ne!(other_trait, known);
        let member = &file.declarations[0];
        assert_eq!(
            index.resolve_member(owner.clone(), &member.name),
            BindingResolution::Local((file.path.clone(), 0))
        );
    }
}

/// 两个glob提供同名声明时必须保留冲突，不能按扫描顺序选第一个。
#[test]
fn rust_conflicting_globs_do_not_prove_unique_binding() {
    let files = vec![source(
        "src/lib.rs",
        "mod a { pub struct Item; } mod b { pub struct Item; } use a::*; use b::*;",
    )];
    assert!(matches!(
        RepositoryIndex::new(&files, &HashMap::new()).resolve_type("src/lib.rs", 0, "Item"),
        BindingResolution::Candidates(_) | BindingResolution::Unknown(_)
    ));
}

/// 显式导入与本地声明均优先于glob提供的同名声明。
#[test]
fn rust_explicit_bindings_shadow_glob_candidates() {
    let files = vec![
        source(
            "src/lib.rs",
            "mod a { pub struct Item; } mod b { pub struct Item; } mod explicit; mod local;",
        ),
        source("src/explicit.rs", "use crate::a::*; use crate::b::Item;"),
        source("src/local.rs", "use crate::a::*; struct Item;"),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    assert_eq!(
        index.resolve_type("src/explicit.rs", 0, "Item"),
        index.resolve_type("src/lib.rs", 0, "b::Item")
    );
    assert!(matches!(
        index.resolve_type("src/explicit.rs", 0, "Item"),
        BindingResolution::Local(_)
    ));
    assert_eq!(
        index.resolve_type("src/local.rs", 0, "Item"),
        BindingResolution::Local(("src/local.rs".into(), 0))
    );
}

/// 未连入真实模块图、循环或部分未知glob都不能提供唯一性证明。
#[test]
fn rust_unlinked_cyclic_and_incomplete_globs_remain_unknown() {
    let files = vec![
        source(
            "src/lib.rs",
            "struct Item; mod a; mod b; mod known { pub struct Item; } mod partial;",
        ),
        source("src/a.rs", "pub use super::b::*;"),
        source("src/b.rs", "pub use super::a::*;"),
        source("src/orphan.rs", "use super::*;"),
        source("src/partial.rs", "use super::known::*; use missing::*;"),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    for path in ["src/a.rs", "src/b.rs", "src/orphan.rs", "src/partial.rs"] {
        assert!(
            matches!(
                index.resolve_type(path, 0, "Item"),
                BindingResolution::Unknown(_) | BindingResolution::Candidates(_)
            ),
            "{path}"
        );
    }
}

/// 同级模块的私有声明不属于glob可见集合，不能误连同名类型。
#[test]
fn rust_sibling_private_declarations_are_not_glob_exports() {
    let files = vec![
        source("src/lib.rs", "mod secret; mod consumer;"),
        source("src/secret.rs", "struct Private; pub struct Public;"),
        source("src/consumer.rs", "use super::secret::*;"),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    assert!(matches!(
        index.resolve_type("src/consumer.rs", 0, "Private"),
        BindingResolution::Unknown(_)
    ));
    assert_eq!(
        index.resolve_type("src/consumer.rs", 0, "Public"),
        BindingResolution::Local(("src/secret.rs".into(), 1))
    );
}

/// 多条glob路径指向同一原始声明时去重，已知不含该名称的模块不会制造未知。
#[test]
fn rust_globs_deduplicate_same_target_and_proven_absence() {
    let files = vec![
        source(
            "src/lib.rs",
            "mod model; mod left; mod right; mod empty; mod consumer;",
        ),
        source("src/model.rs", "pub struct Item;"),
        source("src/left.rs", "pub use super::model::Item;"),
        source("src/right.rs", "pub use super::model::Item;"),
        source("src/empty.rs", "pub struct Other;"),
        source(
            "src/consumer.rs",
            "use super::model::*; use crate::model::*; use super::empty::*;",
        ),
    ];
    assert_eq!(
        RepositoryIndex::new(&files, &HashMap::new()).resolve_type("src/consumer.rs", 0, "Item"),
        BindingResolution::Local(("src/model.rs".into(), 0))
    );
}

/// 内层glob先于外层显式导入，但内层顺序let生效后仍遮蔽它。
#[test]
fn rust_inner_glob_precedes_outer_import_until_local_binding() {
    let files = vec![source("src/lib.rs", "mod outer { pub fn work() {} } mod inner { pub fn work() {} } use outer::work; fn run() { use crate::inner::*; work(); let work = 1; work(); }")];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    let inner_work = files[0]
        .declarations
        .iter()
        .position(|item| item.qualified_name == "inner::work")
        .unwrap();
    let calls: Vec<_> = files[0]
        .occurrences
        .iter()
        .filter(|item| {
            item.kind == StructureRelationshipKind::Calls && item.written_target == "work"
        })
        .collect();
    assert_eq!(calls.len(), 2);
    assert_eq!(
        index.resolve_occurrence(
            "src/lib.rs",
            calls[0].scope,
            "work",
            calls[0].range.bytes.start,
            calls[0].kind
        ),
        BindingResolution::Local(("src/lib.rs".into(), inner_work))
    );
    assert!(matches!(
        index.resolve_occurrence(
            "src/lib.rs",
            calls[1].scope,
            "work",
            calls[1].range.bytes.start,
            calls[1].kind
        ),
        BindingResolution::Unknown(_)
    ));
}

/// glob沿path属性的逻辑父模块，不把物理目录或另一个crate根混入绑定。
#[test]
fn rust_parent_glob_uses_path_attribute_and_separate_crate_roots() {
    let files = vec![
        source(
            "src/lib.rs",
            "struct Item; #[path=\"tests/helpers.rs\"] mod helpers;",
        ),
        source("src/tests/helpers.rs", "use super::*;"),
        source(
            "src/main.rs",
            "struct Item; #[path=\"bin_child.rs\"] mod child;",
        ),
        source("src/bin_child.rs", "use super::*;"),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    assert_eq!(
        index.resolve_type("src/tests/helpers.rs", 0, "Item"),
        BindingResolution::Local(("src/lib.rs".into(), 0))
    );
    assert_eq!(
        index.resolve_type("src/bin_child.rs", 0, "Item"),
        BindingResolution::Local(("src/main.rs".into(), 0))
    );
}

/// 外部glob或条件路径未知时不能凭名称猜导出，也不能回退成prelude。
#[test]
fn rust_external_and_conditional_globs_do_not_invent_prelude_targets() {
    let files = vec![
        source(
            "src/lib.rs",
            "#[cfg_attr(feature=\"alternate\", path=\"alternate.rs\")] mod child; mod external;",
        ),
        source("src/child.rs", "use super::*;"),
        source("src/external.rs", "use std::anything::*;"),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    for path in ["src/child.rs", "src/external.rs"] {
        assert!(matches!(
            index.resolve_type(path, 0, "Default"),
            BindingResolution::Unknown(_)
        ));
    }
}

/// pub与括号之间的空白不改变crate/super/self可见性，也不扩大私有域。
#[test]
fn rust_glob_respects_spaced_restricted_visibility() {
    let files = vec![
        source("src/lib.rs", "mod owner { pub mod secret { pub (super) struct ParentOnly; pub (crate) struct Shared; pub (self) struct Hidden; } } mod consumer;"),
        source("src/consumer.rs", "use crate::owner::secret::*;"),
    ];
    let index = RepositoryIndex::new(&files, &HashMap::new());
    for name in ["ParentOnly", "Hidden"] {
        assert!(matches!(
            index.resolve_type("src/consumer.rs", 0, name),
            BindingResolution::Unknown(_)
        ));
    }
    let shared = files[0]
        .declarations
        .iter()
        .position(|item| item.name == "Shared")
        .unwrap();
    assert_eq!(
        index.resolve_type("src/consumer.rs", 0, "Shared"),
        BindingResolution::Local(("src/lib.rs".into(), shared))
    );
}

/// 隐藏同名声明会遮蔽公开glob导出，不能跳过它选中被遮蔽目标。
#[test]
fn rust_private_direct_name_blocks_public_glob_fallback() {
    let files = vec![
        source("src/lib.rs", "mod original { pub struct Item; } mod hidden { struct Item; pub use crate::original::*; } mod consumer;"),
        source("src/consumer.rs", "use crate::hidden::*;"),
    ];
    assert!(matches!(
        RepositoryIndex::new(&files, &HashMap::new()).resolve_type("src/consumer.rs", 0, "Item"),
        BindingResolution::Unknown(_)
    ));
}

/// 重导出的pub范围尚未成为owned证据，非后代不能把受限重导出当公开导出。
#[test]
fn rust_restricted_reexport_is_unknown_across_glob_boundary() {
    for declaration in [
        "pub (self) use crate::original::Item;",
        "pub (self) use crate::original::*;",
    ] {
        let files = vec![
            source("src/lib.rs", &format!("mod original {{ pub struct Item; }} mod hidden {{ {declaration} }} mod consumer;")),
            source("src/consumer.rs", "use crate::hidden::*;"),
        ];
        assert!(matches!(
            RepositoryIndex::new(&files, &HashMap::new()).resolve_type(
                "src/consumer.rs",
                0,
                "Item"
            ),
            BindingResolution::Unknown(_)
        ));
    }
}

/// 公开类型不能跨越不可见的中间私有模块，inline及外部mod路径都检查声明边。
#[test]
fn rust_glob_rejects_invisible_intermediate_module_paths() {
    let inline = vec![source("src/lib.rs", "mod owner { mod hidden { pub struct Item; } } mod consumer { use crate::owner::hidden::*; impl Item {} }")];
    let index = RepositoryIndex::new(&inline, &HashMap::new());
    let group = &inline[0].impl_groups[0];
    assert!(matches!(
        index.resolve_type("src/lib.rs", group.scope, "Item"),
        BindingResolution::Unknown(_)
    ));
    let external = vec![
        source("src/lib.rs", "mod owner; mod consumer;"),
        source("src/owner.rs", "mod hidden;"),
        source("src/owner/hidden.rs", "pub struct Item;"),
        source("src/consumer.rs", "use crate::owner::hidden::*;"),
    ];
    assert!(matches!(
        RepositoryIndex::new(&external, &HashMap::new()).resolve_type("src/consumer.rs", 0, "Item"),
        BindingResolution::Unknown(_)
    ));
}

/// 中间模块公开或对crate可见时仍可解析，私有路径对其所属模块后代仍有效。
#[test]
fn rust_glob_allows_proven_module_visibility_and_private_descendants() {
    for visibility in ["pub", "pub (crate)"] {
        let files = vec![
            source(
                "src/lib.rs",
                &format!(
                    "mod owner {{ {visibility} mod shared {{ pub struct Item; }} }} mod consumer;"
                ),
            ),
            source("src/consumer.rs", "use crate::owner::shared::*;"),
        ];
        assert!(matches!(
            RepositoryIndex::new(&files, &HashMap::new()).resolve_type(
                "src/consumer.rs",
                0,
                "Item"
            ),
            BindingResolution::Local(_)
        ));
    }
    let files = vec![source("src/lib.rs", "mod owner { mod hidden { pub struct Item; } mod consumer { use super::hidden::*; impl Item {} } }")];
    let group = &files[0].impl_groups[0];
    assert!(matches!(
        RepositoryIndex::new(&files, &HashMap::new()).resolve_type(
            "src/lib.rs",
            group.scope,
            "Item"
        ),
        BindingResolution::Local(_)
    ));
}

/// pub和受限括号之间含注释时，未解码可见性应为未知而不能误当公开。
#[test]
fn rust_commented_visibility_prefix_does_not_become_public() {
    let files = vec![
        source(
            "src/lib.rs",
            "mod owner { pub mod secret { pub /* scope */ (super) struct Item; } } mod consumer;",
        ),
        source("src/consumer.rs", "use crate::owner::secret::*;"),
    ];
    assert!(matches!(
        RepositoryIndex::new(&files, &HashMap::new()).resolve_type("src/consumer.rs", 0, "Item"),
        BindingResolution::Unknown(_)
    ));
}
