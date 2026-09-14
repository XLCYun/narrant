use super::*;
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
fn analyze(directory: &std::path::Path, files: &[&str]) -> AnalysisReport {
    TypeScriptLanguageAnalyzer::default()
        .analyze_with_report(&AnalysisInput {
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
    let report = analyze(
        directory.path(),
        &["main.ts", "target.js", "other.js", "barrel.ts"],
    );
    let graph = &report.graph;
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
    let report = analyze(
        directory.path(),
        &["model.ts", "main.js", "esm.mts", "cjs.cts", "common.cjs"],
    );
    let graph = &report.graph;
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
fn graph_jsx_properties_heritage_writes_and_function_values() {
    let directory = fixture(&[
        ("tsconfig.json", r#"{"compilerOptions":{"jsx":"preserve","allowJs":true,"target":"ES2022","module":"Preserve","moduleResolution":"Bundler","types":[]},"include":["**/*"]}"#),
        ("main.tsx", "interface Contract {} class Base {} class Child extends Base implements Contract { value: Contract; method() { this.value = {}; } handler = () => {}; } function Component() { return <div/>; } function hit() {} const alias = (hit); function render() { const child = new Child(); child.handler(); alias(); return <div onClick={() => hit()}><Component/></div>; } const saved = hit; const data = 1; const copy = data;"),
    ]);
    let report = analyze(directory.path(), &["main.tsx"]);
    let graph = &report.graph;
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
        ("method", "code.writes", "value"),
        ("render", "code.calls", "hit"),
        ("render", "code.calls", "handler"),
        ("saved", "code.references", "hit"),
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
    assert!(named(graph, "copy").data().references.is_empty());
}

/// 缺失业务依赖保留本地可靠事实；动态、结构调用签名和多个可能实现不猜测。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_missing_dynamic_ambiguous_and_type_only_are_unresolved() {
    let directory = fixture(&[
        ("tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","module":"Preserve","moduleResolution":"Bundler","types":[]},"include":["**/*"]}"#),
        ("main.ts", "import {missing} from 'missing-business'; function real() {} function other() {} interface Callable { (): void; method(): void; } declare const typed: Callable; function caller(flag: boolean) { real(); missing(); typed(); typed.method(); (flag ? real : other)(); const object = {action: real}; object['action'](); }"),
    ]);
    let report = analyze(directory.path(), &["main.ts"]);
    let refs = &named(&report.graph, "caller").data().references;
    assert!(
        refs.iter()
            .any(|r| r.target
                == ReferenceTarget::Node(named(&report.graph, "real").data().id.clone())),
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
    assert!(report
        .notes
        .iter()
        .any(|n| n.message.contains("missing-business")));
    assert!(report
        .notes
        .iter()
        .all(|n| n.file.as_ref().is_none_or(|p| p.is_relative())
            && n.project.as_ref().is_none_or(|p| p.is_relative())));
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
    let report = analyze(directory.path(), &["a.ts", "b.ts"]);
    let graph = &report.graph;
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
        ("main.ts", "import type {Model, action as direct} from './target'; import {action, runtimeAction, BarrelModel} from './barrel'; function caller() { action(); direct(); Model.run(); new Model(); BarrelModel.run(); new BarrelModel(); } function shadow() { function action() {} action(); class Model { static run() {} } Model.run(); new Model(); } function allowed() { runtimeAction(); }"),
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
    let report = analyze(directory.path(), &["main.ts", "target.ts", "barrel.ts"]);
    let refs = &named(&report.graph, "caller").data().references;
    assert!(
        refs.iter()
            .filter(|r| r.semantic == "code.calls")
            .all(|r| matches!(r.target, ReferenceTarget::Unresolved { .. })),
        "{refs:?}"
    );
    assert!(named(&report.graph, "shadow")
        .data()
        .references
        .iter()
        .any(|r| r.semantic == "code.calls" && matches!(r.target, ReferenceTarget::Node(_))));
    assert!(named(&report.graph, "many")
        .data()
        .references
        .iter()
        .all(|r| matches!(r.target, ReferenceTarget::Unresolved { .. })));
    assert!(report.notes.len() <= 200);
    assert!(report.notes.iter().any(|n| n.code == "typescript.TS1362"));
    fs::write(
        &main,
        format!("// @ts-nocheck\n{}", fs::read_to_string(&main).unwrap()),
    )
    .unwrap();
    let unchecked = analyze(directory.path(), &["main.ts", "target.ts", "barrel.ts"]);
    for graph in [&report.graph, &unchecked.graph] {
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
    assert!(
        named(&unchecked.graph, "caller")
            .data()
            .references
            .iter()
            .filter(|r| r.semantic == "code.calls")
            .all(|r| matches!(r.target, ReferenceTarget::Unresolved { .. })),
        "{:?}",
        named(&unchecked.graph, "caller").data().references
    );
}

/// 两个真实配置对同一调用目标不一致时保留未知关系和独立冲突说明。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_project_conflicts_and_ts5_compatibility_notes() {
    let directory = fixture(&[
        ("a/tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","module":"CommonJS","moduleResolution":"Node","baseUrl":"..","paths":{"@impl":["a/impl.ts"]},"types":[]},"files":["../shared.ts","impl.ts"]}"#),
        ("b/tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","module":"CommonJS","moduleResolution":"Node","baseUrl":"..","paths":{"@impl":["b/impl.ts"]},"types":[]},"files":["../shared.ts","impl.ts"]}"#),
        ("a/impl.ts", "export function work() { return 1; }"),
        ("b/impl.ts", "export function work() { return 2; }"),
        ("shared.ts", "import {work} from '@impl'; function reliable() {} function caller() { work(); reliable(); }"),
    ]);
    let report = analyze(directory.path(), &["shared.ts", "a/impl.ts", "b/impl.ts"]);
    let refs = &named(&report.graph, "caller").data().references;
    assert!(
        refs.iter().any(|r| r.target
            == ReferenceTarget::Unresolved {
                name: "work".into()
            }),
        "{refs:?}"
    );
    assert!(refs
        .iter()
        .any(|r| r.target
            == ReferenceTarget::Node(named(&report.graph, "reliable").data().id.clone())));
    assert!(report
        .notes
        .iter()
        .any(|n| n.code == "typescript.conflicting_projects"));
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.message.contains("deprecated")),
        "{:?}",
        report.notes
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
    let report = analyze(
        directory.path(),
        &["packages/app/src/main.ts", "packages/lib/src/index.ts"],
    );
    assert!(!directory.path().join("packages/lib/dist").exists());
    let service = named(&report.graph, "service").data().id.clone();
    assert!(named(&report.graph, "caller")
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
    let one = analyze(a.path(), &["main.ts", "hidden.test.ts"]);
    let two = analyze(b.path(), &["main.ts"]);
    assert!(one.graph.diff(&two.graph).unwrap().nodes.is_empty());
    assert!(!nodes(&one.graph)
        .iter()
        .any(|n| n.data().name.as_deref() == Some("helper")));
    let with_tests = TypeScriptLanguageAnalyzer::default()
        .analyze_with_report(&AnalysisInput {
            repository_root: a.path().into(),
            files: vec!["main.ts".into(), "hidden.test.ts".into()],
            options: crate::structure_review::code_graph::AnalysisOptions {
                include_tests: true,
            },
        })
        .unwrap();
    let helper = named(&with_tests.graph, "helper").data().id.clone();
    assert!(named(&one.graph, "caller")
        .data()
        .references
        .iter()
        .any(|r| r.target
            == ReferenceTarget::External {
                symbol: helper.0.clone()
            }));
    assert!(named(&with_tests.graph, "caller")
        .data()
        .references
        .iter()
        .any(|r| r.target == ReferenceTarget::Node(helper.clone())));
    assert!(one
        .graph
        .external_symbols()
        .values()
        .any(|s| s.name == "helper"
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
    assert!(a.graph.diff(&b.graph).unwrap().nodes.is_empty());
    let diff = a.graph.diff(&c.graph).unwrap();
    assert_eq!(diff.nodes.len(), 2);
    assert!(diff
        .nodes
        .iter()
        .all(|d| d.content == Some(crate::structure_review::code_graph::ChangeKind::Modified)));
    assert!(!diff
        .nodes
        .iter()
        .any(|d| d.id == named(&a.graph, "Parent").data().id));
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
        one.graph.diff(&two.graph).unwrap().nodes.is_empty(),
        "{:?}",
        one.graph.diff(&two.graph).unwrap()
    );
    assert!(one
        .graph
        .diff(&three.graph)
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
    let report = analyze(directory.path(), &["main.ts", "中文 path/实现.ts"]);
    let graph = &report.graph;
    let refs = &named(graph, "caller").data().references;
    assert!(
        named(graph, "saved")
            .data()
            .references
            .iter()
            .any(|r| r.semantic == "code.references"
                && r.target
                    == ReferenceTarget::Unresolved {
                        name: "method".into()
                    }),
        "{:?}",
        named(graph, "saved").data().references
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
            && r.target
                == ReferenceTarget::Unresolved {
                    name: "method".into()
                }),
        "{refs:?}"
    );
    assert!(!nodes(graph)
        .iter()
        .flat_map(|n| &n.data().references)
        .any(|r| r.semantic == "code.imports"
            && matches!(&r.target, ReferenceTarget::Unresolved {name} if name == "scope")));
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
        let id = named(&selected.graph, target).data().id.clone();
        assert!(
            named(&selected.graph, "caller")
                .data()
                .references
                .iter()
                .any(
                    |r| r.semantic == "code.calls" && r.target == ReferenceTarget::Node(id.clone())
                ),
            "{target}: {:?}",
            named(&selected.graph, "caller").data().references
        );
        if ["api", "globalApi"].contains(&target) {
            assert!(named(&unselected.graph, "caller")
                .data()
                .references
                .iter()
                .any(|r| r.target
                    == ReferenceTarget::External {
                        symbol: id.0.clone()
                    }));
            assert_eq!(
                unselected.graph.external_symbols()[&id.0].scope,
                crate::structure_review::code_graph::SymbolScope::Repository
            );
        }
    }
    for target in ["isolated", "typed", "method"] {
        assert!(named(&selected.graph, "caller")
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

/// 同名方法和箭头字段是不同运行时候选，联合顺序不能制造确定调用。
#[test]
#[ignore = "需要固定 TypeScript 工具"]
fn graph_heterogeneous_union_members_preserve_ambiguity() {
    let directory = fixture(&[
        ("tsconfig.json", r#"{"compilerOptions":{"target":"ES2022","types":[]},"include":["*.ts"]}"#),
        ("main.ts", "class A { method() {} } class B { method = () => {}; } function forward(value: A | B) { value.method(); } function reversed(value: B | A) { value.method(); } class method {} class Producer { method(): method { return new method(); } } function single(value: Producer) { value.method(); }"),
    ]);
    let report = analyze(directory.path(), &["main.ts"]);
    for name in ["forward", "reversed"] {
        let calls = named(&report.graph, name)
            .data()
            .references
            .iter()
            .filter(|r| r.semantic == "code.calls")
            .collect::<Vec<_>>();
        assert_eq!(calls.len(), 1, "{name}: {calls:?}");
        assert_eq!(
            calls[0].target,
            ReferenceTarget::Unresolved {
                name: "method".into()
            },
            "{name}: {calls:?}"
        );
    }
    let calls = named(&report.graph, "single")
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
