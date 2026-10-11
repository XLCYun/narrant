//! 迁移旧 graph.rs 的业务断言，经实际 helper 与公共 CodeGraph/diff 验证。
use crate::structure_review::{
    code_graph::{
        AnalysisInput, AnalysisOptions, ChangeKind, CodeGraph, CodeNode, LanguageAnalyzer,
    },
    native_analyzer::{HelperCommand, HelperIdentity, NativeLanguageAnalyzer, Platform},
};
use std::{collections::BTreeMap, fs, path::PathBuf};

/// 显式开发 executable 与内嵌标准声明来源，不注册生产工厂。
pub(super) fn analyzer() -> NativeLanguageAnalyzer {
    let executable =
        PathBuf::from(std::env::var_os("NARRANT_PYTHON_HELPER").expect("需指定实际 Python helper"))
            .canonicalize()
            .unwrap();
    let root = executable.parent().unwrap().to_owned();
    let environment = BTreeMap::new();
    #[cfg(windows)]
    let environment = {
        let mut environment = environment;
        for key in ["SystemRoot", "WINDIR", "SystemDrive"] {
            if let Some(value) = std::env::var_os(key) {
                environment.insert(key.into(), value);
            }
        }
        environment
    };
    NativeLanguageAnalyzer::new(
        HelperCommand {
            executable,
            args: vec![],
            environment,
            trusted_roots: vec![root.clone()],
        },
        HelperIdentity {
            helper_id: "narrant-python".into(),
            helper_version: "0.1.0".into(),
            projection_revision: "python-source-v1".into(),
            languages: vec!["python".into()],
            profile: "python-static-v1".into(),
        },
        vec![Platform {
            id: "python-stdlib".into(),
            kind: "python-embedded-stdlib".into(),
            version: "0.0.84".into(),
            root,
            metadata: serde_json::json!({"source_revision":"11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a"}),
        }],
    )
}

/// 无引用的源码也通过实际公共 Host 边界。
fn analyze(source: &str) -> CodeGraph {
    let repository = tempfile::tempdir().unwrap();
    fs::write(repository.path().join("main.py"), source).unwrap();
    analyzer()
        .analyze(&AnalysisInput {
            repository_root: repository.path().into(),
            files: vec!["main.py".into()],
            options: Default::default(),
        })
        .unwrap()
}
/// 仅通过公共包含图找到源码对象，不依赖适配器的身份编码和内部 AST。
pub(super) fn named<'a>(graph: &'a CodeGraph, name: &str) -> &'a CodeNode {
    let mut pending = graph.root().contains.clone();
    while let Some(id) = pending.pop() {
        let node = graph.node(&id).unwrap();
        if node.data().name.as_deref() == Some(name) {
            return node;
        }
        pending.extend(node.data().contains.iter().cloned());
    }
    panic!("缺少代码对象：{name}")
}

/// 函数体修改只修改函数，模块的声明锚点和文件容器保持不变。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn function_body_change_belongs_to_callable() {
    let before = analyze("def calculate():\n    return 1\n");
    let after = analyze("def calculate():\n    return 2\n");
    let function = named(&before, "calculate");
    assert!(matches!(function, CodeNode::Callable(_)));
    let difference = before.diff(&after).unwrap();
    assert_eq!(difference.nodes.len(), 1, "{difference:?}");
    assert_eq!(difference.nodes[0].id, function.data().id);
    assert_eq!(difference.nodes[0].content, Some(ChangeKind::Modified));
}

/// 类方法独立归属，局部类仍完整属于外层方法。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn class_method_body_and_local_class_do_not_change_parent_content() {
    let before = analyze("class Service:\n    token: str = 'a'\n    def run(this):\n        class Local:\n            value = 1\n        return Local\n");
    let after = analyze("class Service:\n    token: str = 'a'\n    def run(this):\n        class Local:\n            value = 2\n        return Local\n");
    assert!(matches!(named(&before, "Service"), CodeNode::Type(_)));
    assert!(matches!(named(&before, "token"), CodeNode::Value(_)));
    let difference = before.diff(&after).unwrap();
    assert_eq!(difference.nodes.len(), 1, "{difference:?}");
    assert_eq!(difference.nodes[0].id, named(&before, "run").data().id);
    assert!(named(&before, "run").data().contains.is_empty());
}

/// 条件中的声明保留控制结构与锚点；只改子体不修改模块，移动声明则修改模块。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn conditional_declarations_preserve_execution_order_without_copying_child_content() {
    let before = analyze("n = 1\nif n:\n    def run():\n        return 1\nn = 2\n");
    let body = analyze("n = 1\nif n:\n    def run():\n        return 2\nn = 2\n");
    let moved = analyze("n = 1\nn = 2\nif n:\n    def run():\n        return 1\n");
    let body_diff = before.diff(&body).unwrap();
    assert_eq!(body_diff.nodes.len(), 1);
    assert_eq!(body_diff.nodes[0].id, named(&before, "run").data().id);
    let moved_diff = before.diff(&moved).unwrap();
    assert_eq!(moved_diff.nodes.len(), 1);
    assert_eq!(
        moved_diff.nodes[0].id,
        named(&before, "module_body").data().id
    );
}

/// 实例字段以参数绑定确认，合并类声明和多处赋值，运行时 RHS 仍归方法。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn fields_deduplicate_receiver_bindings_without_stealing_method_content() {
    let source = "class Service:\n    value: int\n    def run(receiver):\n        receiver.value = 1\n        receiver.extra: str = 'x'\n        receiver.value = 2\n    def invalid(self):\n        self = 3\n        self.fake = 1\n    @staticmethod\n    def static(self):\n        self.fake_static = 1\n";
    let before = analyze(source);
    let after = analyze(&source.replace("receiver.value = 2", "receiver.value = 3"));
    let class = named(&before, "Service");
    let members = class
        .data()
        .contains
        .iter()
        .map(|id| before.node(id).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        members
            .iter()
            .filter(|node| matches!(node, CodeNode::Value(_)))
            .count(),
        2
    );
    assert!(matches!(named(&before, "extra"), CodeNode::Value(_)));
    let difference = before.diff(&after).unwrap();
    assert_eq!(difference.nodes.len(), 1, "{difference:?}");
    assert_eq!(difference.nodes[0].id, named(&before, "run").data().id);
}

/// import 与模式捕获会重绑定参数；内层作用域的同名局部变量不会覆盖外层实例。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn receiver_fields_follow_lexical_bindings() {
    let graph = analyze("class Service:\n    def imported(receiver):\n        import sys as receiver\n        receiver.fake_import = 1\n    def imported_from(receiver):\n        from sys import version as receiver\n        receiver.fake_from = 1\n    def captured(receiver, value):\n        match value:\n            case {'x': receiver}: pass\n        receiver.fake_capture = 1\n    def nested(receiver):\n        def inner():\n            receiver = None\n        class Local:\n            receiver = None\n        receiver.good = 1\n    def nonlocal_write(receiver):\n        def inner():\n            nonlocal receiver\n            receiver = None\n        receiver.fake_nonlocal = 1\n");
    let fields = named(&graph, "Service")
        .data()
        .contains
        .iter()
        .map(|id| graph.node(id).unwrap())
        .filter(|node| matches!(node, CodeNode::Value(_)))
        .map(|node| node.data().name.as_deref().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(fields, ["good"]);
}

/// 推导式迭代变量和 lambda 局部赋值独立作用域，推导式 walrus 则写入外层。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn receiver_fields_distinguish_expression_scopes_and_outer_walrus() {
    let graph = analyze("class Service:\n    def comprehension(receiver, items):\n        [receiver for receiver in items]\n        {receiver for receiver in items}\n        {receiver: 1 for receiver in items}\n        (receiver for receiver in items)\n        receiver.good_comprehension = 1\n    def callback(receiver):\n        action = lambda: (receiver := None)\n        receiver.good_lambda = 1\n    def outer_walrus(receiver, items):\n        [(receiver := value) for value in items]\n        receiver.fake_outer = 1\n");
    let fields = named(&graph, "Service")
        .data()
        .contains
        .iter()
        .map(|id| graph.node(id).unwrap())
        .filter(|node| matches!(node, CodeNode::Value(_)))
        .map(|node| node.data().name.as_deref().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(fields, ["good_comprehension", "good_lambda"]);
}

/// 内部链接按选中的词法路径拥有节点，真实路径仅用于边界核验。
#[cfg(unix)]
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn internal_links_preserve_selected_paths_and_distinct_identities() {
    let server = tempfile::tempdir().unwrap();
    fs::write(server.path().join("impl.py"), "def service(): pass\n").unwrap();
    for alias in ["alias.py", "api.pyi"] {
        std::os::unix::fs::symlink("impl.py", server.path().join(alias)).unwrap();
    }
    let analyze = |files: &[&str]| {
        analyzer()
            .analyze(&AnalysisInput {
                repository_root: server.path().into(),
                files: files.iter().map(PathBuf::from).collect(),
                options: Default::default(),
            })
            .unwrap()
    };
    let single = analyze(&["alias.py"]);
    assert_eq!(single.root().contains.len(), 1);
    let alias_id = single.root().contains[0].clone();
    assert_eq!(
        single.node(&alias_id).unwrap().data().name.as_deref(),
        Some("alias.py")
    );
    let all = analyze(&["alias.py", "impl.py", "api.pyi"]);
    assert_eq!(all.root().contains.len(), 3);
    assert!(all.root().contains.contains(&alias_id));
    assert_eq!(
        named(&single, "service").data().id,
        all.node(&named(&single, "service").data().id)
            .unwrap()
            .data()
            .id
    );
}

/// 不支持的目标语法须按项目范围拒绝，不能默认为宿主或最新版本解析。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn project_requires_python_controls_native_syntax() {
    let server = tempfile::tempdir().unwrap();
    fs::write(
        server.path().join("main.py"),
        "match item:\n    case 1: pass\n",
    )
    .unwrap();
    fs::write(
        server.path().join("pyproject.toml"),
        "[project]\nrequires-python = '>=3.8'\n",
    )
    .unwrap();
    let result = analyzer().analyze(&AnalysisInput {
        repository_root: server.path().into(),
        files: vec!["main.py".into()],
        options: Default::default(),
    });
    let error = result.unwrap_err().message;
    assert!(
        error.contains("语法") && error.contains("main.py"),
        "{error}"
    );
}

/// 链接目标伪装成环境目录也不能跳过原生加载边界检查。
#[cfg(unix)]
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn escaping_context_links_fail_native_loading() {
    let server = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("pyvenv.cfg"), "home = /outside\n").unwrap();
    fs::write(outside.path().join("api.py"), "def external(): pass\n").unwrap();
    std::os::unix::fs::symlink(outside.path(), server.path().join("linked")).unwrap();
    fs::write(server.path().join("main.py"), "def local(): pass\n").unwrap();
    let result = analyzer().analyze(&AnalysisInput {
        repository_root: server.path().into(),
        files: vec!["main.py".into()],
        options: Default::default(),
    });
    assert!(result.unwrap_err().message.contains("逃逸"));
}

/// AST 规范化忽略排版和普通注释，但保存文档字符串与有序初始化语义。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn normalization_and_field_order_distinguish_semantic_content() {
    let before = analyze("class Record:\n    first = 1\n    second = 2\n    def read(self, default=1):\n        'doc'\n        return default\n");
    let formatting = analyze("# comment\nclass Record:\n  first=1 # comment\n  second=2\n  def read(self, default = 1):\n    \"doc\"\n    return default\n");
    assert!(before.diff(&formatting).unwrap().nodes.is_empty());
    let swapped = analyze("class Record:\n    second = 2\n    first = 1\n    def read(self, default=1):\n        'doc'\n        return default\n");
    let diff = before.diff(&swapped).unwrap();
    assert_eq!(diff.nodes.len(), 1);
    assert_eq!(diff.nodes[0].id, named(&before, "Record").data().id);
    let document = analyze("class Record:\n    first = 1\n    second = 2\n    def read(self, default=1):\n        'changed doc'\n        return default\n");
    assert_eq!(
        before.diff(&document).unwrap().nodes[0].id,
        named(&before, "read").data().id
    );
}

/// 多目标共享初始化保持在类内，重复字段只保留一个对象且不丢失初始化顺序。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn shared_initializers_and_repeated_fields_are_not_duplicated() {
    let before = analyze("class Pair:\n    left = right = 1\n    repeated = 1\n    repeated = 2\n");
    let after = analyze("class Pair:\n    left = right = 2\n    repeated = 1\n    repeated = 2\n");
    assert_eq!(named(&before, "Pair").data().contains.len(), 3);
    assert_eq!(named(&before, "left").data().code, None);
    let diff = before.diff(&after).unwrap();
    assert_eq!(diff.nodes.len(), 1);
    assert_eq!(diff.nodes[0].id, named(&before, "Pair").data().id);
    assert_eq!(
        named(&before, "repeated").data().code.as_deref(),
        Some("1\n2")
    );
}

/// 输入验证先于测试过滤；排除的测试明确覆盖，启用后恢复实际节点。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn test_filter_and_input_failures_never_hide_invalid_source_paths() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("tests")).unwrap();
    fs::write(
        root.path().join("tests/test_api.py"),
        "def test_it(): pass\n",
    )
    .unwrap();
    fs::write(root.path().join("notes.txt"), "text").unwrap();
    let analyzer = analyzer();
    let mut input = AnalysisInput {
        repository_root: root.path().into(),
        files: vec!["tests/test_api.py".into()],
        options: Default::default(),
    };
    assert!(analyzer.analyze(&input).unwrap().root().contains.is_empty());
    for path in ["../escape.py", "notes.txt", "tests/../notes.txt"] {
        input.files = vec![path.into()];
        assert!(analyzer.analyze(&input).is_err(), "{path}");
    }
    input.files = vec!["tests/test_api.py".into()];
    input.options.include_tests = true;
    assert_eq!(analyzer.analyze(&input).unwrap().root().contains.len(), 1);
}

/// 不支持的目标、无交集约束和非 UTF-8 声明均报告明确分析失败。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn invalid_versions_and_encodings_fail_native_analysis() {
    for (config, source) in [
        ("[environment]\npython-version='3.6'", "def run(): pass"),
        ("[environment]\npython-version='3.15'", "def run(): pass"),
        ("", "# coding: latin-1\ndef run(): pass"),
        ("", "# coding note; coding: latin-1\ntext = 'é'"),
        ("", "#!/usr/bin/python\r# coding: latin-1\rdef run(): pass"),
    ] {
        let server = tempfile::tempdir().unwrap();
        fs::write(server.path().join("ty.toml"), config).unwrap();
        fs::write(server.path().join("main.py"), source).unwrap();
        assert!(analyzer()
            .analyze(&AnalysisInput {
                repository_root: server.path().into(),
                files: vec!["main.py".into()],
                options: Default::default()
            })
            .is_err());
    }
}

/// Python 只采用首个有效 cookie，后续行上的编码文字不覆盖已选编码。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn first_valid_utf8_cookie_controls_source_decoding() {
    let graph =
        analyze("# coding note; coding: utf-8\n# coding: latin-1\ndef text(): return '中文'\n");
    assert!(named(&graph, "text")
        .data()
        .code
        .as_ref()
        .unwrap()
        .contains("中文"));
}

/// 退役前旧完整图快照继续校验原生图的每个身份、分类、内容、原文、关系与外部边界。
#[test]
#[ignore = "需要已构建 Python helper"]
fn python_helper_preserves_historical_complete_graphs() {
    /// 一份完整图对照包含场景名、真实仓库源码和明确选集。
    type GraphFixture<'a> = (&'a str, &'a [(&'a str, &'a str)], &'a [&'a str]);
    let fixtures: &[GraphFixture<'_>] = &[
        ("relationships", &[
            ("main.py", "from api import service\nimport absent_package\nfrom typing import overload\nclass Payload: pass\nclass Worker:\n    value: Payload\n    @property\n    def title(self): return '中文'\n    @title.setter\n    def title(self, value): self._title = value\n    @overload\n    def convert(self, value: int) -> int: ...\n    @overload\n    def convert(self, value: str) -> str: ...\n    def convert(self, value): return value\n    def run(receiver):\n        receiver.extra = 1\n        service()\n        len([])\n        'abc'.upper()\n        absent_package.fetch()\n"),
            ("api.py", "def service(): return 1\n"),
        ], &["main.py", "api.py"]),
        ("bindings", &[
            ("main.py", "from api import service as clean\ndef first(): pass\ndef second(): pass\ngood = first\nrebound = first\nrebound = second\nforward = rebound\nif unknown:\n    conditional = first\nelse:\n    conditional = second\ndef run():\n    clean()\n    good()\n    rebound()\n    forward()\n    conditional()\n"),
            ("api.py", "def service(): pass\n"),
        ], &["main.py"]),
        ("projects", &[
            ("alpha/ty.toml", "[environment]\nroot=['src']\npython-version='3.8'\n"),
            ("alpha/src/api.py", "def alpha(): pass\n"),
            ("alpha/src/main.py", "from api import alpha\ndef run_alpha(): alpha()\n"),
            ("beta/ty.toml", "[environment]\nroot=['src']\npython-version='3.14'\n"),
            ("beta/src/api.py", "def beta(): pass\n"),
            ("beta/src/main.py", "from api import beta\ndef run_beta(): beta()\n"),
        ], &["alpha/src/main.py", "alpha/src/api.py", "beta/src/main.py", "beta/src/api.py"]),
    ];
    for (name, files, selected) in fixtures {
        let repository = tempfile::tempdir().unwrap();
        for (path, source) in *files {
            let path = repository.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }
        let input = AnalysisInput {
            repository_root: repository.path().into(),
            files: selected.iter().map(PathBuf::from).collect(),
            options: AnalysisOptions::default(),
        };
        let native = analyzer().analyze(&input).unwrap();
        let historical = match *name {
            "relationships" => include_str!("fixtures/python-historical/relationships.json"),
            "bindings" => include_str!("fixtures/python-historical/bindings.json"),
            "projects" => include_str!("fixtures/python-historical/projects.json"),
            _ => unreachable!(),
        };
        let historical: serde_json::Value = serde_json::from_str(historical).unwrap();
        assert_eq!(graph_contract(&native), historical, "{name}");
    }
}

/// 迁移旧取消矩阵中的超时和调用方 panic；真实 helper 退出后公共 Host 才允许返回。
#[cfg(unix)]
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn python_helper_timeout_and_caller_panic_reap_actual_process() {
    use std::{os::unix::fs::PermissionsExt, sync::Arc, time::Duration};
    for panic in [false, true] {
        let repository = tempfile::tempdir().unwrap();
        let tools = tempfile::tempdir().unwrap();
        let source: String = (0..15000)
            .map(|index| format!("def value_{index}(): return len([])\n"))
            .collect();
        fs::write(repository.path().join("main.py"), source).unwrap();
        let marker = tools.path().join("pid");
        let launcher = tools.path().join("start-helper");
        let executable = PathBuf::from(std::env::var_os("NARRANT_PYTHON_HELPER").unwrap());
        // 测试只记录即将 exec 的真实 PID，不加入假分析器或任何控制/所有权实现。
        let quote = |value: &std::path::Path| {
            format!("'{}'", value.to_str().unwrap().replace('\'', "'\\''"))
        };
        fs::write(
            &launcher,
            format!(
                "#!/bin/sh\nprintf '%s' \"$$\" > {}\nexec {}\n",
                quote(&marker),
                quote(&executable)
            ),
        )
        .unwrap();
        fs::set_permissions(&launcher, fs::Permissions::from_mode(0o755)).unwrap();
        let mut native = analyzer();
        native.command.executable = launcher;
        native.command.trusted_roots.push(tools.path().into());
        let observed = marker.clone();
        native = native
            .with_limits(crate::structure_review::native_analyzer::AnalysisLimits {
                analysis_timeout: Duration::from_millis(150),
                ..Default::default()
            })
            .with_cancellation(Arc::new(move || {
                assert!(!(panic && observed.exists()), "模拟调用方观察器 panic");
                false
            }));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            native.analyze(&AnalysisInput {
                repository_root: repository.path().into(),
                files: vec!["main.py".into()],
                options: Default::default(),
            })
        }));
        if panic {
            assert!(result.is_err());
        } else {
            let error = result.unwrap().unwrap_err();
            assert!(error.message.contains("超时"), "{error}");
            assert!(!error.is_cancelled());
        }
        let pid: i32 = fs::read_to_string(&marker).unwrap().parse().unwrap();
        assert_eq!(
            unsafe { libc::kill(pid, 0) },
            -1,
            "返回后真实 helper {pid} 未退出"
        );
    }
}

/// 历史完整业务快照按集合排序关系和成员，保持身份、分类、内容、原文及外部范围断言。
fn graph_contract(graph: &CodeGraph) -> serde_json::Value {
    let mut pending = graph.root().contains.clone();
    let mut nodes = BTreeMap::new();
    while let Some(id) = pending.pop() {
        let node = graph.node(&id).unwrap();
        let data = node.data();
        pending.extend(data.contains.clone());
        let mut contains = data
            .contains
            .iter()
            .map(|id| id.0.clone())
            .collect::<Vec<_>>();
        contains.sort();
        let mut references = data
            .references
            .iter()
            .map(|reference| format!("{reference:?}"))
            .collect::<Vec<_>>();
        references.sort();
        nodes.insert(id.0.clone(),serde_json::json!({"category":format!("{:?}",node.category()),"language":data.language.0,"kind":data.native_kind,"name":data.name,"signature":data.signature,"code":data.code,"source":data.source,"contains":contains,"references":references,"extensions":data.extensions}));
    }
    serde_json::json!({"root":format!("{:?}",graph.root()),"nodes":nodes,"externals":format!("{:?}",graph.external_symbols())})
}
