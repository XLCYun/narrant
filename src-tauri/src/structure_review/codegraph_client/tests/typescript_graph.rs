use crate::structure_review::code_graph::{AnalysisInput, CodeGraph, LanguageAnalyzer};
use crate::structure_review::code_graph::{CodeNode, ReferenceTarget};
use crate::structure_review::codegraph_client::CodeGraphAnalyzer;
use std::{fs, path::PathBuf};

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

/// 显式真实验收要求隔离安装包存在，绝不跳过缺少工具的测试。
fn analyzer() -> CodeGraphAnalyzer {
    CodeGraphAnalyzer::from_pack(
        "typescript-javascript",
        &super::candidate::root("typescript-javascript"),
    )
    .unwrap()
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

/// 双平台真实 CLI 的分析取消必须返回取消类别，调用结束后输入目录可释放。
#[test]
#[ignore = "需要真实 TypeScript CodeGraph 包"]
fn real_typescript_cli_cancellation_reaps_before_return() {
    let directory = fixture(&[(
        "main.ts",
        "export function run() { return Promise.resolve(1); }",
    )]);
    for index in 0..300 {
        fs::write(directory.path().join(format!("context{index}.ts")), format!("export class Model{index} {{ value: Map<string, Promise<number>>; method() {{ return this.value; }} }}")).unwrap();
    }
    let started = std::time::Instant::now();
    let error = analyzer()
        .with_cancellation(std::sync::Arc::new(move || {
            started.elapsed() > std::time::Duration::from_millis(150)
        }))
        .analyze(&AnalysisInput {
            repository_root: directory.path().into(),
            files: vec!["main.ts".into()],
            options: Default::default(),
        })
        .unwrap_err();
    assert!(error.is_cancelled(), "{error}");
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    directory.close().unwrap();
}
