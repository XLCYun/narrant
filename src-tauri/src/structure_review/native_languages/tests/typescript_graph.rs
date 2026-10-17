use super::*;
use crate::structure_review::code_graph::{AnalysisInput, CodeGraph, LanguageAnalyzer};
use crate::structure_review::code_graph::{CodeNode, ReferenceTarget};

/// 真实服务夹具完全独立于应用仓库和业务依赖。
fn fixture(files: &[(&str, &str)]) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    for (name, source) in files {
        let path = directory.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    directory
}

/// 固定工具缺失直接失败；真实语义验收通过 --ignored 显式执行。
fn analyze(directory: &std::path::Path, files: &[&str]) -> CodeGraph {
    analyzer()
        .analyze(&AnalysisInput {
            repository_root: directory.into(),
            files: files.iter().map(PathBuf::from).collect(),
            options: Default::default(),
        })
        .unwrap()
}

/// 遍历图的真实包含树，测试无需依赖身份编码细节。
fn nodes(graph: &CodeGraph) -> Vec<&CodeNode> {
    let mut pending = graph.root().contains.clone();
    let mut result = Vec::new();
    while let Some(id) = pending.pop() {
        let node = graph.node(&id).unwrap();
        pending.extend(node.data().contains.iter().cloned());
        result.push(node);
    }
    result
}

/// 按具名声明定位，再断言实际关系目标。
fn named<'a>(graph: &'a CodeGraph, name: &str) -> &'a CodeNode {
    nodes(graph)
        .into_iter()
        .find(|n| n.data().name.as_deref() == Some(name))
        .unwrap()
}

/// 按所属类定位同名成员，避免把不同类的方法混为同一目标。
fn member<'a>(graph: &'a CodeGraph, owner: &str, name: &str) -> &'a CodeNode {
    named(graph, owner)
        .data()
        .contains
        .iter()
        .map(|id| graph.node(id).unwrap())
        .find(|node| node.data().name.as_deref() == Some(name))
        .unwrap()
}

/// 参数属性进入真实图并承载类型与默认值关系，普通参数不被提升为字段。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_parameter_properties_keep_field_types_and_default_calls() {
    use crate::structure_review::code_graph::CodeCategory;
    let directory = fixture(&[
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"target":"ES2022","module":"Preserve","moduleResolution":"Bundler","types":[]},"include":["*.ts"]}"#,
        ),
        (
            "model.ts",
            "export class Model {} export function make(): Model { return new Model(); }",
        ),
        (
            "main.ts",
            r#"import {Model, make} from './model';
            export class Client {
                constructor(private readonly model: Model = make(), public optional?: Model,
                    ordinary?: Model, readonly callback: () => Model = () => make()) {}
            }
            export function run(client: Client) { client.callback(); }
        "#,
        ),
    ]);
    let graph = analyze(directory.path(), &["main.ts", "model.ts"]);
    let class = named(&graph, "Client").data();
    let constructor = named(&graph, "constructor").data();
    for name in ["model", "optional", "callback"] {
        let field = named(&graph, name);
        assert_eq!(field.category(), CodeCategory::Value);
        assert_eq!(field.data().native_kind, "field");
        assert!(class.contains.contains(&field.data().id));
        assert!(!constructor.contains.contains(&field.data().id));
        assert!(
            field.data().references.iter().any(|reference| {
                reference.semantic == "code.type_use"
                    && reference.target
                        == ReferenceTarget::Node(named(&graph, "Model").data().id.clone())
            }),
            "{name}: {:?}",
            field.data().references
        );
    }
    for name in ["model", "callback"] {
        assert!(named(&graph, name)
            .data()
            .references
            .iter()
            .any(|reference| {
                reference.semantic == "code.calls"
                    && reference.target
                        == ReferenceTarget::Node(named(&graph, "make").data().id.clone())
            }));
    }
    assert!(nodes(&graph)
        .iter()
        .all(|node| node.data().name.as_deref() != Some("ordinary")));
    assert!(constructor.references.is_empty());
    assert!(
        named(&graph, "run")
            .data()
            .references
            .iter()
            .all(|reference| {
                reference.target
                    != ReferenceTarget::Node(named(&graph, "callback").data().id.clone())
            }),
        "可被实参覆盖的默认函数不提供确定的调用目标"
    );
}

/// 资源与缺失模块的纯导入不生成图对象，也不阻断有效代码的调用图。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_unverified_relative_imports_do_not_abort_analysis() {
    let directory = fixture(&[
        ("tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","module":"Preserve","moduleResolution":"Bundler","types":[]},"include":["*.ts"]}"#),
        ("main.ts", "import './style.css'; import './missing.ts'; function target() {} export function caller() { target(); }"),
        ("style.css", "body { color: red; }"),
    ]);
    let graph = analyze(directory.path(), &["main.ts"]);
    let target = named(&graph, "target").data().id.clone();
    assert!(named(&graph, "caller")
        .data()
        .references
        .iter()
        .any(|reference| reference.semantic == "code.calls"
            && reference.target == ReferenceTarget::Node(target.clone())));
    assert_eq!(nodes(&graph).len(), 3);
    assert_eq!(
        graph.root().contains,
        [named(&graph, "main.ts").data().id.clone()]
    );
    assert!(nodes(&graph).iter().all(|node| matches!(
        node.category(),
        crate::structure_review::code_graph::CodeCategory::Callable
            | crate::structure_review::code_graph::CodeCategory::Container
    )));
}

/// TS/JS 共用选集身份，导入和 barrel 别名最终落到声明而非导入节点。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_cross_language_alias_barrel_and_callbacks() {
    let directory = fixture(&[
        ("tsconfig.json", r#"{"compilerOptions":{"allowJs":true,"checkJs":true,"target":"ES2022","module":"Preserve","moduleResolution":"Bundler","types":[]},"include":["**/*"]}"#),
        ("target.js", "export function jsTarget() {}"),
        ("barrel.ts", "export {jsTarget as forwarded} from './target.js';"),
        ("main.ts", "import {forwarded as alias} from './barrel'; export const tsTarget = () => alias(); export function caller() { [1].map(() => { alias(); tsTarget(); }); }"),
        ("other.js", "import { tsTarget } from './main'; export function jsCaller() { tsTarget(); }"),
    ]);
    let graph = &analyze(
        directory.path(),
        &["main.ts", "target.js", "other.js", "barrel.ts"],
    );
    assert_eq!(nodes(graph).len(), 7);
    assert_eq!(graph.root().contains.len(), 3);
    assert!(nodes(graph)
        .iter()
        .flat_map(|node| &node.data().references)
        .all(|reference| !matches!(
            reference.semantic.as_str(),
            "code.imports" | "code.reexports"
        )));
    let target = named(graph, "jsTarget").data().id.clone();
    assert!(named(graph, "tsTarget")
        .data()
        .references
        .iter()
        .any(|r| r.semantic == "code.calls" && r.target == ReferenceTarget::Node(target.clone())));
    assert!(named(graph, "caller")
        .data()
        .references
        .iter()
        .any(|r| r.semantic == "code.calls" && r.target == ReferenceTarget::Node(target.clone())));
    let ts = named(graph, "tsTarget").data().id.clone();
    assert!(named(graph, "jsCaller")
        .data()
        .references
        .iter()
        .any(|r| r.target == ReferenceTarget::Node(ts.clone())));
    assert!(!nodes(graph)
        .iter()
        .any(|n| n.data().native_kind == "arrow" && n.data().name.is_none()));
}

/// 真实 LSP 解析的调用在合并后仍属于正确执行区域，文件容器只承载包含关系。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_file_modules_keep_merged_execution_calls() {
    use crate::structure_review::code_graph::CodeCategory;
    let directory = fixture(&[
        ("tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","module":"Preserve","moduleResolution":"Bundler","types":[]},"include":["*.ts"]}"#),
        ("target.ts", "export function initialize() {} export function refresh() {} export function fromCallback() {}"),
        ("main.ts", "import {initialize, refresh, fromCallback} from './target'; const a = initialize(); const b = initialize(); initialize(); if (a) refresh(); [1].forEach(() => fromCallback()); export function run() { refresh(); }"),
    ]);
    let graph = &analyze(directory.path(), &["main.ts", "target.ts"]);
    assert_eq!(graph.root().contains.len(), 2);
    let execution = named(graph, "顶层执行");
    assert_eq!(
        nodes(graph)
            .iter()
            .filter(|n| n.category() == CodeCategory::ExecutionRegion)
            .count(),
        1
    );
    assert!(named(graph, "main.ts")
        .data()
        .contains
        .contains(&execution.data().id));
    for name in ["initialize", "refresh", "fromCallback"] {
        let target = named(graph, name);
        assert!(
            execution
                .data()
                .references
                .iter()
                .any(|r| r.semantic == "code.calls"
                    && r.target == ReferenceTarget::Node(target.data().id.clone())),
            "{name}: {:?}",
            execution.data().references
        );
    }
    assert!(nodes(graph)
        .iter()
        .filter(|n| n.category() == CodeCategory::Container)
        .all(|n| n.data().references.is_empty()));
}

/// 缩减节点范围后，具名函数值、内部别名与被合并的初始化/回调调用仍由真实服务定位。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_named_function_values_and_omitted_initializers_keep_calls() {
    use crate::structure_review::code_graph::CodeCategory;
    let directory = fixture(&[
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"target":"ES2022","module":"Preserve","moduleResolution":"Bundler","types":[]},"include":["*.ts"]}"#,
        ),
        (
            "target.ts",
            "export const target = () => 1; export const expression = function() {};",
        ),
        (
            "main.ts",
            r#"import {target, expression} from './target';
            let mutable = () => target(); var legacy = function() { expression(); };
            class Holder { handler = () => target(); static action = function() { expression(); }; }
            const object = { handler: () => target(), objectExpression: function() { expression(); } };
            const first = target(); const second = expression(); const alias = target;
            function outer() { const localAlias = alias; localAlias(); mutable(); legacy();
                const holder = new Holder(); holder.handler(); Holder.action(); object.handler(); object.objectExpression();
                function nested() { target(); } const callback = () => expression();
                [1].forEach(() => { const inside = 1; target(); }); nested(); callback(); }
        "#,
        ),
    ]);
    let graph = &analyze(directory.path(), &["main.ts", "target.ts"]);
    for name in [
        "first",
        "second",
        "alias",
        "localAlias",
        "object",
        "nested",
        "callback",
        "inside",
        "holder",
    ] {
        assert!(
            nodes(graph)
                .iter()
                .all(|node| node.data().name.as_deref() != Some(name)),
            "{name}"
        );
    }
    assert!(nodes(graph)
        .iter()
        .all(|node| node.category() != CodeCategory::Value));
    for name in ["mutable", "legacy", "handler", "action", "expression"] {
        assert_eq!(named(graph, name).category(), CodeCategory::Callable);
    }
    let execution = named(graph, "顶层执行");
    assert_eq!(execution.category(), CodeCategory::ExecutionRegion);
    for owner in [execution, named(graph, "outer")] {
        for name in ["target", "expression"] {
            let target = named(graph, name);
            assert!(
                owner
                    .data()
                    .references
                    .iter()
                    .any(|reference| reference.semantic == "code.calls"
                        && reference.target == ReferenceTarget::Node(target.data().id.clone())),
                "{}: {:?}",
                owner.data().name.as_deref().unwrap(),
                owner.data().references
            );
        }
    }
    let outer = named(graph, "outer");
    for target in nodes(graph).into_iter().filter(|node| {
        matches!(
            node.data().name.as_deref(),
            Some("mutable" | "legacy" | "handler" | "action")
        )
    }) {
        assert!(
            outer
                .data()
                .references
                .iter()
                .any(|reference| reference.semantic == "code.calls"
                    && reference.target == ReferenceTarget::Node(target.data().id.clone())),
            "{:?}: {:?}",
            target.data().name,
            outer.data().references
        );
    }
}

/// 标准 Promise/Node 包装方法保持外部声明身份，JSDoc 原生类型推断可定位仓库方法。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_runtime_wrappers_node_modules_and_jsdoc() {
    let directory = fixture(&[
        ("package.json", r#"{"type":"module"}"#),
        ("tsconfig.json", r#"{"compilerOptions":{"allowJs":true,"checkJs":true,"target":"ES2022","module":"NodeNext","types":["node"],"lib":["ES2022"]},"include":["**/*"]}"#),
        ("model.ts", "export class Model { run() { return 1; } }"),
        ("main.js", "/** @param {import('./model.js').Model} model */\nexport function jsdocCaller(model) { return model.run(); }"),
        ("esm.mts", "import {readFile} from 'node:fs/promises'; export async function esmReader() { await readFile('x'); return Promise.resolve(1); }"),
        ("cjs.cts", "import fs = require('node:fs'); export function cjsReader() { return fs.readFileSync('x'); }"),
        ("common.cjs", "const fs = require('node:fs'); exports.read = function read() { return fs.readFileSync('x'); };"),
    ]);
    let graph = &analyze(
        directory.path(),
        &["model.ts", "main.js", "esm.mts", "cjs.cts", "common.cjs"],
    );
    let run = named(graph, "run").data().id.clone();
    assert!(named(graph, "jsdocCaller")
        .data()
        .references
        .iter()
        .any(|r| r.semantic == "code.calls" && r.target == ReferenceTarget::Node(run.clone())));
    for name in ["esmReader", "cjsReader", "read"] {
        let refs = &named(graph, name).data().references;
        assert!(refs.iter().any(|r| r.semantic == "code.calls" && matches!(&r.target, ReferenceTarget::External { symbol } if symbol.starts_with("typescript:runtime:"))), "{name}: {refs:?}");
    }
    let esm = &named(graph, "esmReader").data().references;
    assert!(esm.iter().any(|r| matches!(&r.target, ReferenceTarget::External {symbol} if symbol.contains("resolve"))), "{esm:?}");
    assert!(graph
        .external_symbols()
        .values()
        .any(|s| s.name == "readFile"));
}

/// JSX 组件调用单独分类；匿名属性回调内调用继续归属具名 render 函数。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_jsx_properties_heritage_and_function_values() {
    let directory = fixture(&[
        ("tsconfig.json", r#"{"compilerOptions":{"jsx":"preserve","allowJs":true,"target":"ES2022","module":"Preserve","moduleResolution":"Bundler","types":[]},"include":["**/*"]}"#),
        ("main.tsx", "interface Contract {} class Base {} class Child extends Base implements Contract { value: Contract; method() { this.value = {}; } handler = () => {}; } function Component() { return <div/>; } function hit() {} const alias = (hit); function render() { const saved = hit; const child = new Child(); child.handler(); alias(); return <div onClick={() => hit()}><Component/></div>; } const saved = hit; const data = 1; const copy = data;"),
    ]);
    let graph = &analyze(directory.path(), &["main.tsx"]);
    let render = named(graph, "render");
    let component = named(graph, "Component").data().id.clone();
    assert!(render
        .data()
        .references
        .iter()
        .any(|r| r.semantic == "jsx.uses_component"
            && r.target == ReferenceTarget::Node(component.clone())));
    assert!(!render.data().references.iter().any(
        |r| r.semantic == "code.calls" && r.target == ReferenceTarget::Node(component.clone())
    ));
    for (source, semantic, target) in [
        ("Child", "code.extends", "Base"),
        ("Child", "code.implements", "Contract"),
        ("value", "code.type_use", "Contract"),
        ("render", "code.calls", "hit"),
        ("render", "code.calls", "handler"),
        ("render", "code.references", "hit"),
    ] {
        let target = named(graph, target).data().id.clone();
        assert!(
            named(graph, source).data().references.iter().any(
                |r| r.semantic == semantic && r.target == ReferenceTarget::Node(target.clone())
            ),
            "{source}: {:?}",
            named(graph, source).data().references
        );
    }
    for name in ["saved", "alias", "data", "copy", "child"] {
        assert!(
            nodes(graph)
                .iter()
                .all(|node| node.data().name.as_deref() != Some(name)),
            "{name}"
        );
    }
    assert!(nodes(graph)
        .iter()
        .flat_map(|node| &node.data().references)
        .all(|reference| !matches!(reference.semantic.as_str(), "code.writes" | "code.reads")));
}

/// 真实 TS/JS 语义不输出写入边，也不把函数值字段左值降级成普通引用。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_assignments_omit_writes_and_keep_calls_and_function_values() {
    for path in ["main.ts", "main.js"] {
        let directory = fixture(&[
            ("tsconfig.json", r#"{"compilerOptions":{"allowJs":true,"checkJs":true,"target":"ES2022","types":[]},"include":["**/*"]}"#),
            (path, "function source() { return 1; } function replacement() {} class Model { value = 0; handler = () => {}; update() { this.value = source(); this.value += 1; this.value++; this.handler = replacement; ({ item: this.handler } = {}); } }"),
        ]);
        let graph = &analyze(directory.path(), &[path]);
        assert!(nodes(graph)
            .iter()
            .flat_map(|node| &node.data().references)
            .all(|reference| !matches!(reference.semantic.as_str(), "code.writes" | "code.reads")));
        let references = &named(graph, "update").data().references;
        assert_eq!(references.len(), 2, "{path}: {references:?}");
        for (semantic, target) in [("code.calls", "source"), ("code.references", "replacement")] {
            assert!(
                references.iter().any(|reference| {
                    reference.semantic == semantic
                        && reference.target
                            == ReferenceTarget::Node(named(graph, target).data().id.clone())
                }),
                "{path}: missing {semantic} {target}"
            );
        }
    }
}

/// 缺失业务依赖保留本地可靠事实；动态、结构调用签名和多个可能实现不猜测。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_missing_dynamic_ambiguous_and_type_only_are_unresolved() {
    let directory = fixture(&[
        ("tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","module":"Preserve","moduleResolution":"Bundler","types":[]},"include":["**/*"]}"#),
        ("main.ts", "import {missing} from 'missing-business'; function real() {} function other() {} interface Callable { (): void; method(): void; } declare const typed: Callable; function caller(flag: boolean) { real(); missing(); typed(); typed.method(); (flag ? real : other)(); const object = {action: real}; object['action'](); }"),
    ]);
    let graph = analyze(directory.path(), &["main.ts"]);
    let refs = &named(&graph, "caller").data().references;
    assert!(
        refs.iter()
            .any(|r| r.target == ReferenceTarget::Node(named(&graph, "real").data().id.clone())),
        "{refs:?}"
    );
    for name in [
        "missing",
        "typed",
        "method",
        "flag ? real : other",
        "\"action\"",
    ] {
        assert!(
            refs.iter().any(|r| r.semantic == "code.calls"
                && matches!(&r.target, ReferenceTarget::Unresolved {name: seen} if seen == name)),
            "{name}: {refs:?}"
        );
    }
}

/// 同所有者重载合并到唯一 Callable；跨文件合并接口保留各自声明及全部类型关系。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_overloads_and_merged_fragments() {
    let directory = fixture(&[
        ("tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","types":[]},"include":["*.ts"]}"#),
        ("a.ts", "interface Shared { a: string; } function pick(x: string): string; function pick(x: number): number; function pick(x: any) { return x; } function caller() { pick(1); }"),
        ("b.ts", "interface Shared { b: string; } class Holder { value: Shared; }"),
    ]);
    let graph = &analyze(directory.path(), &["a.ts", "b.ts"]);
    assert_eq!(
        nodes(graph)
            .iter()
            .filter(|n| n.data().name.as_deref() == Some("pick"))
            .count(),
        1
    );
    let shared = nodes(graph)
        .iter()
        .filter(|n| n.data().name.as_deref() == Some("Shared"))
        .map(|n| n.data().id.clone())
        .collect::<Vec<_>>();
    assert_eq!(shared.len(), 2);
    let refs = &named(graph, "value").data().references;
    for id in shared {
        assert!(
            refs.iter()
                .any(|r| r.semantic == "code.type_use"
                    && r.target == ReferenceTarget::Node(id.clone())),
            "{refs:?}"
        );
    }
    assert!(named(graph, "caller")
        .data()
        .references
        .iter()
        .any(|r| r.target == ReferenceTarget::Node(named(graph, "pick").data().id.clone())));
}

/// 原生错误位置阻止 type-only 别名穿透；同名的内层运行时绑定仍能被正确调用。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_type_only_alias_chains_do_not_become_runtime_calls() {
    let directory = fixture(&[
        ("tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","module":"Preserve","moduleResolution":"Bundler","types":[]},"include":["*.ts"]}"#),
        ("target.ts", "export function action() {} export class Model { static run() {} }"),
        ("barrel.ts", "export type {action} from './target'; export {action as runtimeAction} from './target'; import type {Model as ImportedModel} from './target'; export {ImportedModel as BarrelModel};"),
        ("main.ts", "import type {Model, action as direct} from './target'; import {action, runtimeAction, BarrelModel} from './barrel'; function caller() { action(); direct(); Model.run(); new Model(); BarrelModel.run(); new BarrelModel(); } namespace Shadow { function action() {} class Model { static run() {} } export function shadow() { action(); Model.run(); new Model(); } } function allowed() { runtimeAction(); }"),
    ]);
    let main = directory.path().join("main.ts");
    fs::write(
        &main,
        format!(
            "{} function many() {{ {} }}",
            fs::read_to_string(&main).unwrap(),
            "action();".repeat(45)
        ),
    )
    .unwrap();
    let graph = analyze(directory.path(), &["main.ts", "target.ts", "barrel.ts"]);
    fs::write(
        &main,
        format!("// @ts-nocheck\n{}", fs::read_to_string(&main).unwrap()),
    )
    .unwrap();
    let unchecked = analyze(directory.path(), &["main.ts", "target.ts", "barrel.ts"]);
    for graph in [&graph, &unchecked] {
        let calls = named(graph, "caller")
            .data()
            .references
            .iter()
            .filter(|r| r.semantic == "code.calls")
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 5, "{calls:?}");
        assert!(
            calls
                .iter()
                .all(|r| matches!(r.target, ReferenceTarget::Unresolved { .. })),
            "{calls:?}"
        );
        for name in ["action", "direct", "run", "Model", "BarrelModel"] {
            assert!(
                calls
                    .iter()
                    .any(|r| r.target == ReferenceTarget::Unresolved { name: name.into() }),
                "{name}: {calls:?}"
            );
        }
        // 相同调用会合并为一个关系，重复次数不能使类型专用导入变成运行时目标。
        let repeated = &named(graph, "many").data().references;
        assert_eq!(repeated.len(), 1, "{repeated:?}");
        assert!(repeated.iter().all(|r| r.semantic == "code.calls"
            && r.target
                == ReferenceTarget::Unresolved {
                    name: "action".into()
                }));
        for name in ["shadow", "allowed"] {
            let calls = named(graph, name)
                .data()
                .references
                .iter()
                .filter(|r| r.semantic == "code.calls")
                .collect::<Vec<_>>();
            assert!(!calls.is_empty());
            assert!(
                calls
                    .iter()
                    .all(|r| matches!(r.target, ReferenceTarget::Node(_))),
                "{name}: {calls:?}"
            );
        }
    }
}

/// 两个真实配置对同一调用目标不一致时保留未知关系，本地确定调用仍可解析。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_project_conflicts_keep_unknown_calls_and_local_targets() {
    let directory = fixture(&[
        ("a/tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","module":"CommonJS","moduleResolution":"Node","baseUrl":"..","paths":{"@impl":["a/impl.ts"]},"types":[]},"files":["../shared.ts","impl.ts"]}"#),
        ("b/tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","module":"CommonJS","moduleResolution":"Node","baseUrl":"..","paths":{"@impl":["b/impl.ts"]},"types":[]},"files":["../shared.ts","impl.ts"]}"#),
        ("a/impl.ts", "export function work() { return 1; }"),
        ("b/impl.ts", "export function work() { return 2; }"),
        ("shared.ts", "import {work} from '@impl'; function reliable() {} function caller() { work(); reliable(); }"),
    ]);
    let graph = analyze(directory.path(), &["shared.ts", "a/impl.ts", "b/impl.ts"]);
    let refs = &named(&graph, "caller").data().references;
    assert_eq!(refs.len(), 2, "{refs:?}");
    for target in [
        ReferenceTarget::Unresolved {
            name: "work".into(),
        },
        ReferenceTarget::Node(named(&graph, "reliable").data().id.clone()),
    ] {
        assert!(
            refs.iter()
                .any(|r| r.semantic == "code.calls" && r.target == target),
            "{refs:?}"
        );
    }
    assert_eq!(
        nodes(&graph)
            .iter()
            .filter(|node| node.data().name.as_deref() == Some("work"))
            .count(),
        2
    );
}

/// workspace exports 保持 dist 声明入口，服务通过 references 原生重定向源码。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_workspace_references_without_built_dist() {
    let directory = fixture(&[
        (
            "package.json",
            r#"{"private":true,"workspaces":["packages/*"]}"#,
        ),
        (
            "tsconfig.json",
            r#"{"files":[],"references":[{"path":"packages/lib"},{"path":"packages/app"}]}"#,
        ),
        (
            "packages/lib/package.json",
            r#"{"name":"@sample/lib","version":"1.0.0","type":"module","exports":{".":{"types":"./dist/index.d.ts","default":"./dist/index.js"}}}"#,
        ),
        (
            "packages/lib/tsconfig.json",
            r#"{"compilerOptions":{"composite":true,"module":"NodeNext","target":"ES2022","rootDir":"src","outDir":"dist","types":[]},"include":["src"]}"#,
        ),
        ("packages/lib/src/index.ts", "export function service() {}"),
        (
            "packages/app/package.json",
            r#"{"name":"@sample/app","version":"1.0.0","type":"module","dependencies":{"@sample/lib":"workspace:*"}}"#,
        ),
        (
            "packages/app/tsconfig.json",
            r#"{"compilerOptions":{"composite":true,"module":"NodeNext","target":"ES2022","rootDir":"src","types":[]},"references":[{"path":"../lib"}],"include":["src"]}"#,
        ),
        (
            "packages/app/src/main.ts",
            "import {service} from '@sample/lib'; export function caller() { service(); }",
        ),
    ]);
    let graph = analyze(
        directory.path(),
        &["packages/app/src/main.ts", "packages/lib/src/index.ts"],
    );
    assert!(!directory.path().join("packages/lib/dist").exists());
    let service = named(&graph, "service").data().id.clone();
    assert!(named(&graph, "caller")
        .data()
        .references
        .iter()
        .any(|r| r.semantic == "code.calls" && r.target == ReferenceTarget::Node(service.clone())));
}

/// 测试过滤只影响图成员；选集变化和仓库绝对路径变化不会改变外部源码身份。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_test_membership_lazy_context_and_stable_external_symbols() {
    let files = [
        ("tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","module":"Preserve","moduleResolution":"Bundler","types":[]},"include":["*.ts"]}"#),
        ("hidden.test.ts", "export function helper() {}"),
        ("main.ts", "import {helper} from './hidden.test'; function caller() { helper(); Promise.resolve(1); }"),
        // 未被语言服务定位的上下文不应被 Oxc 全仓库解析。
        ("unused.ts", "const invalid = ;"),
    ];
    let a = fixture(&files);
    let b = fixture(&files);
    // 未命中的上下文连文本都不读取，准备和结束阶段不能因其编码错误失败。
    for directory in [&a, &b] {
        fs::write(directory.path().join("unread.ts"), [0xff]).unwrap();
    }
    let one = analyze(a.path(), &["main.ts", "hidden.test.ts"]);
    let two = analyze(b.path(), &["main.ts"]);
    assert!(one.diff(&two).unwrap().nodes.is_empty());
    assert!(!nodes(&one)
        .iter()
        .any(|n| n.data().name.as_deref() == Some("helper")));
    let with_tests = analyzer()
        .analyze(&AnalysisInput {
            repository_root: a.path().into(),
            files: vec!["main.ts".into(), "hidden.test.ts".into()],
            options: crate::structure_review::code_graph::AnalysisOptions {
                include_tests: true,
            },
        })
        .unwrap();
    let helper = named(&with_tests, "helper").data().id.clone();
    assert!(named(&one, "caller")
        .data()
        .references
        .iter()
        .any(|r| r.target
            == ReferenceTarget::External {
                symbol: helper.0.clone()
            }));
    assert!(named(&with_tests, "caller")
        .data()
        .references
        .iter()
        .any(|r| r.target == ReferenceTarget::Node(helper.clone())));
    assert!(one.external_symbols().values().any(|s| s.name == "helper"
        && s.scope == crate::structure_review::code_graph::SymbolScope::Repository));
}

/// 格式修改不产生图差异；子方法修改不污染父类，自身回调代码修改仍可见。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_diff_format_child_parent_and_inline_callback() {
    let before = fixture(&[(
        "main.ts",
        "class Parent { child() { return 1; } } function caller() { [1].map(() => 1); }",
    )]);
    let formatted = fixture(&[("main.ts", "// ordinary\nclass Parent {\n child ( ) { return 1 ; }\n}\nfunction caller() { [ 1 ].map( () => 1 ); }")]);
    let changed = fixture(&[(
        "main.ts",
        "class Parent { child() { return 2; } } function caller() { [1].map(() => 2); }",
    )]);
    let a = analyze(before.path(), &["main.ts"]);
    let b = analyze(formatted.path(), &["main.ts"]);
    let c = analyze(changed.path(), &["main.ts"]);
    assert!(a.diff(&b).unwrap().nodes.is_empty());
    let diff = a.diff(&c).unwrap();
    assert_eq!(diff.nodes.len(), 2);
    assert!(diff
        .nodes
        .iter()
        .all(|d| d.content == Some(crate::structure_review::code_graph::ChangeKind::Modified)));
    assert!(!diff
        .nodes
        .iter()
        .any(|d| d.id == named(&a, "Parent").data().id));
}

/// 动态名字来自 AST 规范表示，格式和引号变化无差异，字符串值变化仍可辨别。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_dynamic_unknown_names_ignore_format_but_preserve_literal_values() {
    let a = fixture(&[("main.ts", "import 'missing-business'; function factory() { return () => 1; } function run() { factory()(); const obj: any = {}; obj['a b'](); }")]);
    let b = fixture(&[("main.ts", "import \"missing-business\"; function factory() { return () => 1; } function run() { factory /* ordinary comment */ ( ) ( ); const obj: any = {}; obj[\"a b\"] ( ); }")]);
    let c = fixture(&[("main.ts", "import 'missing-business'; function factory() { return () => 1; } function run() { factory()(); const obj: any = {}; obj['a  b'](); }")]);
    let one = analyze(a.path(), &["main.ts"]);
    let two = analyze(b.path(), &["main.ts"]);
    let three = analyze(c.path(), &["main.ts"]);
    assert!(
        one.diff(&two).unwrap().nodes.is_empty(),
        "{:?}",
        one.diff(&two).unwrap()
    );
    assert!(one
        .diff(&three)
        .unwrap()
        .nodes
        .iter()
        .any(|n| !n.references.is_empty()));
}

/// Unicode/CRLF 坐标、默认/namespace 别名、const 链和联合接收者均走真实定义证据。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_unicode_default_namespace_alias_and_union_evidence() {
    let directory = fixture(&[
        ("tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","module":"Preserve","moduleResolution":"Bundler","types":[]},"include":["**/*.ts"]}"#),
        ("中文 path/实现.ts", "export default function action() {}\r\nexport function namedAction() {}"),
        ("main.ts", "import defaultAction, * as scope from './中文 path/实现';\r\nconst emoji = '😀'; const first = defaultAction; const second = (first as typeof first); class A { method() {} } class B { method() {} } function caller(value: A | B) { const saved = value.method; second(); scope.namedAction(); value.method(); }"),
    ]);
    let graph = &analyze(directory.path(), &["main.ts", "中文 path/实现.ts"]);
    let refs = &named(graph, "caller").data().references;
    assert!(
        refs.iter().any(|r| r.semantic == "code.references"
            && r.target
                == ReferenceTarget::Unresolved {
                    name: "method".into()
                }),
        "{:?}",
        refs
    );
    for target in ["action", "namedAction"] {
        assert!(
            refs.iter().any(|r| r.semantic == "code.calls"
                && r.target == ReferenceTarget::Node(named(graph, target).data().id.clone())),
            "{refs:?}"
        );
    }
    assert!(
        refs.iter().any(|r| r.semantic == "code.calls"
            && r.target == ReferenceTarget::Node(member(graph, "A", "method").data().id.clone())),
        "{refs:?}"
    );
    assert!(!nodes(graph)
        .iter()
        .flat_map(|n| &n.data().references)
        .any(|r| matches!(r.semantic.as_str(), "code.imports" | "code.reexports")));
}

/// 仓库声明式函数是值绑定；接口调用契约和无实现的普通孤立签名仍不能确认运行时目标。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_repository_ambient_functions_and_nonambient_signature_boundary() {
    let directory = fixture(&[
        ("tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","module":"NodeNext","types":[]},"include":["**/*"]}"#),
        ("api.d.mts", "export function api(): void;"),
        ("globals.d.ts", "declare function globalApi(): void;"),
        ("main.mts", "import {api} from './api.mjs'; declare function explicit(): void; declare namespace Context { function ambient(): void; } function isolated(): void; interface Callable { (): void; method(): void; } declare const typed: Callable; export function caller() { api(); globalApi(); explicit(); Context.ambient(); isolated(); typed(); typed.method(); }"),
    ]);
    let selected = analyze(directory.path(), &["main.mts", "api.d.mts", "globals.d.ts"]);
    let unselected = analyze(directory.path(), &["main.mts"]);
    for target in ["api", "globalApi", "explicit", "ambient"] {
        let id = named(&selected, target).data().id.clone();
        assert!(
            named(&selected, "caller").data().references.iter().any(
                |r| r.semantic == "code.calls" && r.target == ReferenceTarget::Node(id.clone())
            ),
            "{target}: {:?}",
            named(&selected, "caller").data().references
        );
        if ["api", "globalApi"].contains(&target) {
            assert!(named(&unselected, "caller")
                .data()
                .references
                .iter()
                .any(|r| r.target
                    == ReferenceTarget::External {
                        symbol: id.0.clone()
                    }));
            assert_eq!(
                unselected.external_symbols()[&id.0].scope,
                crate::structure_review::code_graph::SymbolScope::Repository
            );
        }
    }
    for target in ["isolated", "typed", "method"] {
        assert!(named(&selected, "caller")
            .data()
            .references
            .iter()
            .any(|r| r.semantic == "code.calls"
                && r.target
                    == ReferenceTarget::Unresolved {
                        name: target.into()
                    }));
    }
}

/// 类内异步方法和回调内的成员调用都应解析到实际方法声明。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_async_member_calls_keep_concrete_targets() {
    let directory = fixture(&[
        (
            "tsconfig.json",
            r#"{"compilerOptions":{"target":"ES2022","types":[]},"include":["*.ts"]}"#,
        ),
        (
            "main.ts",
            r#"
            class TickHttpClient {
                public async request() { return this.sendOnce(); }
                public requestTick() { return this.request(); }
                public async retry() { return [1].map(() => this.sendOnce()); }
                private async sendOnce(): Promise<number> { return this.parseResult(); }
                private parseResult() { return 1; }
            }
            class Other { public async request() { return 2; } }
            function unionRequest(client: TickHttpClient | Other) { return client.request(); }
        "#,
        ),
    ]);
    let graph = analyze(directory.path(), &["main.ts"]);
    let method = |name| member(&graph, "TickHttpClient", name);
    for (caller, callee) in [
        ("request", "sendOnce"),
        ("requestTick", "request"),
        ("retry", "sendOnce"),
        ("sendOnce", "parseResult"),
    ] {
        let calls = &method(caller).data().references;
        assert!(
            calls
                .iter()
                .any(|reference| reference.semantic == "code.calls"
                    && reference.target == ReferenceTarget::Node(method(callee).data().id.clone())),
            "{caller} -> {callee}: {calls:?}"
        );
    }
    let calls = &named(&graph, "unionRequest").data().references;
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert_eq!(
        calls[0].target,
        ReferenceTarget::Node(method("request").data().id.clone())
    );
}

/// 联合成员调用保留 definition 导航到的方法或箭头字段，不补齐其他运行时实现。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_heterogeneous_union_members_follow_definition_navigation() {
    let directory = fixture(&[
        ("tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","types":[]},"include":["*.ts"]}"#),
        ("main.ts", "class A { method() {} } class B { method = () => {}; } function forward(value: A | B) { value.method(); } function reversed(value: B | A) { value.method(); } function field(value: B) { value.method(); } class method {} class Producer { method(): method { return new method(); } } function single(value: Producer) { value.method(); }"),
    ]);
    let graph = analyze(directory.path(), &["main.ts"]);
    // 固定 TypeScript 服务将两种联合写法都导航到 A；直接访问 B 时仍定位其箭头字段。
    for (name, owner) in [("forward", "A"), ("reversed", "A"), ("field", "B")] {
        let calls = named(&graph, name)
            .data()
            .references
            .iter()
            .filter(|r| r.semantic == "code.calls")
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 1, "{name}: {calls:?}");
        assert_eq!(
            calls[0].target,
            ReferenceTarget::Node(member(&graph, owner, "method").data().id.clone()),
            "{name}: {calls:?}"
        );
    }
    let calls = named(&graph, "single")
        .data()
        .references
        .iter()
        .filter(|r| r.semantic == "code.calls")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 1);
    assert!(
        matches!(calls[0].target, ReferenceTarget::Node(_)),
        "{calls:?}"
    );
}

/// 同一仓库内路径在普通根、外部测试目录和名为 __tests__ 的仓库下产生完全相同的图。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_repository_relative_test_classification_ignores_checkout_ancestors() {
    let temporary = tempfile::tempdir().unwrap();
    let mut expected: Option<CodeGraph> = None;
    for relative_root in ["ordinary/repo", "__tests__/repo", "named/__tests__"] {
        let root = temporary.path().join(relative_root);
        for (name, source) in [
            ("tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","types":[]},"include":["**/*.ts"]}"#),
            ("src/main.ts", "export function production() { test(() => 1); } function test(callback: () => number) { return callback(); }"),
            ("contest.ts", "export const contest = () => 1;"),
            ("__tests__/main.ts", "export const directoryTest = () => 1;"),
            ("src/main.test.ts", "export const suffixTest = () => 1;"),
            ("src/main.spec.ts", "export const suffixSpec = () => 1;"),
        ] {
            let path = root.join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }
        let files = [
            "src/main.ts",
            "contest.ts",
            "__tests__/main.ts",
            "src/main.test.ts",
            "src/main.spec.ts",
        ];
        let actual = analyze(&root, &files);
        assert_eq!(
            named(&actual, "production").data().source.as_ref().unwrap(),
            "export function production() { test(() => 1); }"
        );
        assert!(nodes(&actual)
            .iter()
            .any(|node| node.data().name.as_deref() == Some("contest")));
        for name in ["directoryTest", "suffixTest", "suffixSpec"] {
            assert!(!nodes(&actual)
                .iter()
                .any(|node| node.data().name.as_deref() == Some(name)));
        }
        if let Some(expected) = &expected {
            assert_eq!(actual.root(), expected.root());
            assert_eq!(nodes(&actual), nodes(expected));
            assert_eq!(actual.external_symbols(), expected.external_symbols());
        } else {
            expected = Some(actual);
        }
        let enabled = analyzer()
            .analyze(&AnalysisInput {
                repository_root: root,
                files: files.iter().map(PathBuf::from).collect(),
                options: crate::structure_review::code_graph::AnalysisOptions {
                    include_tests: true,
                },
            })
            .unwrap();
        for name in ["directoryTest", "suffixTest", "suffixSpec"] {
            assert_eq!(named(&enabled, name).data().name.as_deref(), Some(name));
        }
    }
}

/// 直接和继承的 configDir 别名保持确定本地调用，不得命中共享配置目录的竞争声明。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_config_dir_templates_retain_local_call() {
    for inherited in [false, true] {
        let fixture = crate::structure_review::config_dir_fixture::fixture(inherited);
        let graph = analyze(
            fixture.path(),
            &["app/src/main.ts", "app/src/dep.ts", "config/src/dep.ts"],
        );
        let target = named(&graph, "target").data().id.clone();
        let wrong = named(&graph, "wrongTarget").data().id.clone();
        let references = &named(&graph, "caller").data().references;
        assert!(
            references
                .iter()
                .any(|reference| reference.semantic == "code.calls"
                    && reference.target == ReferenceTarget::Node(target.clone())),
            "{inherited}: {references:?}"
        );
        assert!(!references
            .iter()
            .any(|reference| reference.target == ReferenceTarget::Node(wrong.clone())));
    }
}

/// 显式真实验收要求隔离安装包存在，绝不跳过缺少工具的测试。
fn analyzer() -> NativeLanguageAnalyzer {
    let root =
        std::env::var_os("NARRANT_TYPESCRIPT_PACK").expect("NARRANT_TYPESCRIPT_PACK required");
    from_pack(PackId::TypeScriptJavascript, Path::new(&root)).unwrap()
}

/// 真实 TypeScript CPU 分析期间取消，复用公共 ProcessTree 退出屏障，不能返回成功空图。
#[test]
#[ignore = "需要真实 TypeScript 原生支持包"]
fn typescript_real_helper_cancellation_reaps_before_return() {
    let directory = fixture(&[(
        "main.ts",
        "export function run() { return Promise.resolve(1); }",
    )]);
    for index in 0..300 {
        fs::write(directory.path().join(format!("context{index}.ts")), format!("export class Model{index} {{ value: Map<string, Promise<number>>; method() {{ return this.value; }} }}")).unwrap();
    }
    let started = std::time::Instant::now();
    let analyzer = analyzer().with_cancellation(std::sync::Arc::new(move || {
        started.elapsed() > std::time::Duration::from_millis(150)
    }));
    let error = analyzer
        .analyze(&AnalysisInput {
            repository_root: directory.path().into(),
            files: vec!["main.ts".into()],
            options: Default::default(),
        })
        .unwrap_err();
    assert!(error.is_cancelled(), "{error:?}");
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    // Windows 上成功删除实际输入目录还验证公共 Host 已完成进程资源回收。
    directory.close().unwrap();
}

/// 固定退役前已验证的身份、类别、原文和包含树，覆盖 Unicode、标点与两平台路径分隔符。
#[test]
#[ignore = "需要真实 TypeScript 原生支持包"]
fn typescript_historical_identity_and_source_contract() {
    /// 历史夹具只保存公共语言契约，不依赖退役后的 AST 或传输实现。
    #[derive(serde::Deserialize)]
    struct HistoricalNode {
        id: String,
        category: String,
        native_kind: String,
        name: Option<String>,
        source: Option<String>,
        contains: Vec<String>,
    }
    let sources = [
        ("中文 path/声明!~'().ts", "/** docs */\nexport class Model { field = 1; constructor(public prop: string) {} method(){} static { run(); } } namespace N { export const arrow = () => 1; } const handlers = { nested: { save(){} } }; function overloaded(x:string):string; function overloaded(x:any){return x;} interface I { property: string; method(): void; } interface I { another: number; }"),
        ("main.js", "export function caller() {} const expression = function hidden(){}; class C { #field = 1; get value(){return 1} set value(v){} };"),
        ("default.ts", "export default function() {}"),
    ];
    let directory = fixture(&sources);
    let graph = analyze(
        directory.path(),
        &sources.iter().map(|(file, _)| *file).collect::<Vec<_>>(),
    );
    let expected: Vec<HistoricalNode> =
        serde_json::from_str(include_str!("fixtures/typescript-historical-identity.json")).unwrap();
    assert_eq!(nodes(&graph).len(), expected.len());
    for node in expected {
        // 历史基线冻结为 POSIX；Windows 仅改变身份路径内的目录分隔符编码。
        let platform_id = |id: String| {
            if cfg!(windows) {
                id.replace("%2F", "%5C")
            } else {
                id
            }
        };
        let id = crate::structure_review::code_graph::NodeId(platform_id(node.id));
        let actual = graph
            .node(&id)
            .unwrap_or_else(|| panic!("原生身份缺少 {id:?}"));
        assert_eq!(format!("{:?}", actual.category()), node.category);
        assert_eq!(actual.data().native_kind, node.native_kind);
        assert_eq!(actual.data().name, node.name);
        assert_eq!(actual.data().source, node.source);
        let mut actual_children: Vec<_> = actual
            .data()
            .contains
            .iter()
            .map(|id| id.0.clone())
            .collect();
        actual_children.sort();
        let mut expected_children: Vec<_> = node.contains.into_iter().map(platform_id).collect();
        expected_children.sort();
        assert_eq!(actual_children, expected_children);
    }
}

/// 目录链接在 Unix/Windows 均可验证；TS 合并物理文件，测试选项和越界检查不能被别名绕过。
#[test]
#[ignore = "需要真实 TypeScript 原生支持包"]
fn typescript_aliases_merge_canonical_nodes_and_keep_boundary_and_test_policy() {
    let directory = fixture(&[
        ("impl/main.ts", "export function run() { return 1; }"),
        ("impl/hidden.test.ts", "export function testOnly() {}"),
    ]);
    for alias in ["alias", "__tests__"] {
        link_directory(
            &directory.path().join("impl"),
            &directory.path().join(alias),
        );
    }
    let mut input = AnalysisInput {
        repository_root: directory.path().into(),
        files: [
            "impl/main.ts",
            "alias/main.ts",
            "__tests__/main.ts",
            "alias/hidden.test.ts",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect(),
        options: Default::default(),
    };
    let analyzer = analyzer();
    let graph = analyzer.analyze(&input).unwrap();
    assert_eq!(graph.root().contains.len(), 1);
    assert_eq!(nodes(&graph).len(), 2);
    assert_eq!(
        graph
            .node(&graph.root().contains[0])
            .unwrap()
            .data()
            .name
            .as_deref(),
        Some("impl/main.ts")
    );
    input.options.include_tests = true;
    let graph = analyzer.analyze(&input).unwrap();
    assert_eq!(graph.root().contains.len(), 2);
    assert_eq!(nodes(&graph).len(), 4);
    input.files.push("impl/main.ts".into());
    assert!(analyzer
        .analyze(&input)
        .unwrap_err()
        .to_string()
        .contains("重复路径"));
    let outside = fixture(&[("escape.ts", "export function escape() {}")]);
    link_directory(outside.path(), &directory.path().join("outside"));
    input.files = vec!["outside/escape.ts".into()];
    assert!(analyzer.analyze(&input).is_err());
    fs::write(directory.path().join("tsconfig.json"), "{ invalid").unwrap();
    input.files = vec!["__tests__/main.ts".into()];
    input.options.include_tests = false;
    assert!(analyzer.analyze(&input).unwrap().root().contains.is_empty());
    input.options.include_tests = true;
    assert!(analyzer.analyze(&input).is_err());
    input.options.include_tests = false;
    input.files.push("impl/main.ts".into());
    assert!(analyzer.analyze(&input).is_err());
    input.files.clear();
    assert!(analyzer.analyze(&input).unwrap().root().contains.is_empty());
}

/// 沿用公共输入夹具的目录链接方式，Windows junction 不需要文件 symlink 特权。
fn link_directory(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    assert!(std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(link)
        .arg(target)
        .status()
        .unwrap()
        .success());
}

/// JSX 前导空格和纯空白文本会改变渲染结果，真实图差异不能只保留 source 而漏掉内容修改。
#[test]
#[ignore = "需要真实 TypeScript 原生支持包"]
fn typescript_jsx_leading_and_space_only_text_changes_graph_content() {
    for (before, after) in [
        ("<div>a</div>", "<div> a</div>"),
        ("<div></div>", "<div> </div>"),
    ] {
        let one = fixture(&[(
            "main.tsx",
            &format!("export function render() {{ return {before}; }}"),
        )]);
        let two = fixture(&[(
            "main.tsx",
            &format!("export function render() {{ return {after}; }}"),
        )]);
        let a = analyze(one.path(), &["main.tsx"]);
        let b = analyze(two.path(), &["main.tsx"]);
        let diff = a.diff(&b).unwrap();
        assert_eq!(diff.nodes.len(), 1, "{before} -> {after}");
        assert_eq!(diff.nodes[0].id, named(&a, "render").data().id);
        assert_eq!(
            diff.nodes[0].content,
            Some(crate::structure_review::code_graph::ChangeKind::Modified)
        );
    }
}
