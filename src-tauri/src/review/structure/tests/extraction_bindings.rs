use super::*;

/// 原生 JS 参数、解构及循环只引入绑定名称，不把键和默认值引用当作局部变量。
#[test]
fn javascript_patterns_preserve_only_binding_names() {
    let source = r#"
function outer({ key: renamed = fallback }, [first, ...tail], plain) {
  let { [computed()]: local = fallback, ...rest } = source;
  for (const loop of values) { loop(); }
}
"#;
    let file = extract_bindings_fixture(SourceLanguage::JavaScript, source);
    for name in ["renamed", "first", "tail", "plain"] {
        assert!(
            file.bindings.iter().any(|binding| {
                binding.name == name && binding.kind == ExtractedBindingKind::Parameter
            }),
            "缺少参数 {name}"
        );
    }
    for name in ["local", "rest", "loop"] {
        assert!(
            file.bindings.iter().any(|binding| {
                binding.name == name && binding.kind == ExtractedBindingKind::Local
            }),
            "缺少局部绑定 {name}"
        );
    }
    for name in ["key", "fallback", "computed", "values", "source"] {
        assert!(
            !file.bindings.iter().any(|binding| binding.name == name),
            "引用不应成为绑定 {name}"
        );
    }
    let call = file
        .occurrences
        .iter()
        .find(|item| item.written_target == "loop")
        .unwrap();
    let binding = file
        .bindings
        .iter()
        .find(|item| item.name == "loop")
        .unwrap();
    assert!(binding_scope_reaches(&file, binding.scope, call.scope));
}

/// var 穿过块级作用域，函数及 class 表达式的递归名称则仅在表达式内部可见。
#[test]
fn javascript_var_and_expression_self_names_have_real_visibility() {
    let source = r#"
function target() {}
function run(flag) {
  if (flag) { var target = other; }
  target();
}
const loop = function recurse() { recurse(); };
const Box = class Inside { member() { return new Inside(); } };
"#;
    let file = extract_bindings_fixture(SourceLanguage::JavaScript, source);
    let local = file
        .bindings
        .iter()
        .find(|binding| binding.name == "target" && binding.kind == ExtractedBindingKind::Local)
        .unwrap();
    let call = file
        .occurrences
        .iter()
        .find(|item| item.written_target == "target")
        .unwrap();
    assert!(binding_scope_reaches(&file, local.scope, call.scope));
    for (internal, visible) in [("recurse", "loop"), ("Inside", "Box")] {
        let binding = file
            .bindings
            .iter()
            .find(|binding| binding.name == internal)
            .unwrap();
        let declaration = binding
            .declaration
            .expect("内部名称对应变量命名的同一个声明");
        assert_eq!(file.declarations[declaration].name, visible);
        assert!(!binding_scope_reaches(&file, binding.scope, 0));
    }
}

/// 具名回调的递归绑定指向自身，回调外的同名调用不能看到这条内部绑定。
#[test]
fn named_callback_self_binding_stays_inside_its_own_function() {
    let source = "function outer() { queue(function retry() { retry(); }); retry(); }";
    for language in [SourceLanguage::TypeScript, SourceLanguage::JavaScript] {
        let file = extract_bindings_fixture(language, source);
        let retry = file
            .declarations
            .iter()
            .position(|item| item.name == "retry")
            .unwrap();
        let bindings: Vec<_> = file
            .bindings
            .iter()
            .filter(|item| item.name == "retry")
            .collect();
        assert_eq!(bindings.len(), 1);
        assert_eq!(bindings[0].declaration, Some(retry));
        let calls: Vec<_> = file
            .occurrences
            .iter()
            .filter(|item| item.written_target == "retry")
            .collect();
        assert_eq!(calls.len(), 2);
        assert!(binding_scope_reaches(
            &file,
            bindings[0].scope,
            calls[0].scope
        ));
        assert!(!binding_scope_reaches(
            &file,
            bindings[0].scope,
            calls[1].scope
        ));
    }
}

/// 原生成员可以通过 this/类型名访问，但不能以裸名称遮蔽词法环境中的全局函数。
#[test]
fn class_and_trait_members_do_not_become_lexical_bindings() {
    let typescript = extract_bindings_fixture(
        SourceLanguage::TypeScript,
        r#"
function work() {}
class Service {
  field = 1;
  constructor(public repo: Repository) {}
  work() {}
  run() { work(); }
}
enum State { Ready, Done }
"#,
    );
    assert_eq!(
        typescript
            .bindings
            .iter()
            .filter(|binding| binding.name == "work")
            .count(),
        1
    );
    for name in ["field", "Ready", "Done"] {
        assert!(!typescript
            .bindings
            .iter()
            .any(|binding| binding.name == name));
    }
    let repo = typescript
        .bindings
        .iter()
        .find(|binding| binding.name == "repo")
        .unwrap();
    assert_eq!(repo.kind, ExtractedBindingKind::Parameter);
    let rust = extract_bindings_fixture(
        SourceLanguage::Rust,
        r#"
struct Service { field: usize }
trait Work { const LIMIT: usize; type Item; fn make() -> Self; fn run(&self); }
impl Service { fn make() -> Self { todo!() } fn run(&self) { fn nested() {} nested(); } }
"#,
    );
    for name in ["field", "LIMIT", "Item", "make", "run"] {
        assert!(
            !rust.bindings.iter().any(|binding| binding.name == name),
            "成员 {name} 不应成为词法绑定"
        );
    }
    assert!(rust
        .bindings
        .iter()
        .any(|binding| binding.name == "nested" && binding.declaration.is_some()));
}

/// 声明签名与泛型各有自己的名称范围，类型中的参数不泄漏到模块。
#[test]
fn generic_and_type_signature_parameters_do_not_leak() {
    let file = extract_bindings_fixture(
        SourceLanguage::TypeScript,
        r#"
class T {}
function work() {}
declare function API(work: () => void): void;
function run<T>(value: T): T { return value; }
type Callable = <U>(argument: U) => U;
work();
"#,
    );
    let call = file
        .occurrences
        .iter()
        .find(|item| item.written_target == "work")
        .unwrap();
    let parameter = file
        .bindings
        .iter()
        .find(|binding| binding.name == "work" && binding.kind == ExtractedBindingKind::Parameter)
        .unwrap();
    assert!(!binding_scope_reaches(&file, parameter.scope, call.scope));
    for name in ["T", "U", "argument"] {
        let binding = file
            .bindings
            .iter()
            .find(|binding| binding.name == name && binding.kind == ExtractedBindingKind::Parameter)
            .unwrap();
        assert!(!binding_scope_reaches(&file, binding.scope, 0));
    }
}

/// Rust 模式提取叶子绑定，循环变量和 if-let 变量只在实际成功的正文里可见。
#[test]
fn rust_patterns_exclude_constructor_names_and_respect_branch_scope() {
    let file = extract_bindings_fixture(
        SourceLanguage::Rust,
        r#"
fn source() {}
fn run(value: Option<fn()>, values: Vec<fn()>) {
  for source in source() { source(); }
  if let Some(branch) = value { branch(); } else { branch(); }
  branch();
  while let Some(repeat) = value { repeat(); }
  match value { Some(target) if guard() => target(), _ => fallback() }
  let Pair(first, ref second) = pair;
  let Data { renamed: local, shorthand } = data;
  let callback = |plain, (left, right): (i32, i32)| plain();
}
"#,
    );
    for name in [
        "source",
        "branch",
        "repeat",
        "target",
        "first",
        "second",
        "local",
        "shorthand",
        "plain",
        "left",
        "right",
    ] {
        assert!(
            file.bindings.iter().any(|binding| binding.name == name),
            "缺少模式绑定 {name}"
        );
    }
    for name in [
        "Some", "Pair", "Data", "Option", "Vec", "guard", "fallback", "renamed",
    ] {
        assert!(
            !file.bindings.iter().any(|binding| binding.name == name),
            "引用不应成为绑定 {name}"
        );
    }
    let iterator = file
        .bindings
        .iter()
        .find(|binding| binding.name == "source" && binding.kind == ExtractedBindingKind::Local)
        .unwrap();
    let source_calls: Vec<_> = file
        .occurrences
        .iter()
        .filter(|item| item.written_target == "source")
        .collect();
    assert_eq!(source_calls.len(), 2);
    assert!(!binding_scope_reaches(
        &file,
        iterator.scope,
        source_calls[0].scope
    ));
    assert!(binding_scope_reaches(
        &file,
        iterator.scope,
        source_calls[1].scope
    ));
    let branch = file
        .bindings
        .iter()
        .find(|binding| binding.name == "branch")
        .unwrap();
    let branch_calls: Vec<_> = file
        .occurrences
        .iter()
        .filter(|item| item.written_target == "branch")
        .collect();
    assert_eq!(branch_calls.len(), 3);
    assert!(binding_scope_reaches(
        &file,
        branch.scope,
        branch_calls[0].scope
    ));
    assert!(!binding_scope_reaches(
        &file,
        branch.scope,
        branch_calls[1].scope
    ));
    assert!(!binding_scope_reaches(
        &file,
        branch.scope,
        branch_calls[2].scope
    ));
}

/// 测试只调用结构分析公开提取入口，避免与 adapter 的私有遍历耦合。
fn extract_bindings_fixture(language: SourceLanguage, source: &str) -> ExtractedFile {
    let parsed = parse_source(language, source).expect("fixture 可以解析");
    assert!(parsed.diagnostics.is_empty(), "fixture 不应依赖语法恢复");
    extract_source(
        if language == SourceLanguage::Rust {
            "fixture.rs"
        } else {
            "fixture.ts"
        },
        &parsed,
    )
}

/// 从发生点沿公开作用域链向上确认绑定可见范围。
fn binding_scope_reaches(file: &ExtractedFile, binding: usize, mut occurrence: usize) -> bool {
    loop {
        if binding == occurrence {
            return true;
        }
        match file.scopes[occurrence].parent {
            Some(parent) => occurrence = parent,
            None => return false,
        }
    }
}
