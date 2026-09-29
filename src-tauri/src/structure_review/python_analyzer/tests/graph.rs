use super::*;
use crate::{
    lsp::test_support::FakeServer,
    structure_review::code_graph::{ChangeKind, CodeNode},
};
use std::fs;

/// 无引用的源码仍经过真实 stdio 边界；测试不依赖安装 ty。
fn analyze(source: &str) -> CodeGraph {
    let server = graph_server();
    fs::write(server.workspace().join("main.py"), source).unwrap();
    PythonLanguageAnalyzer::new(server.workspace().join("server"))
        .analyze(&AnalysisInput {
            repository_root: server.workspace().into(),
            files: vec!["main.py".into()],
            options: Default::default(),
        })
        .unwrap()
}

/// 仅通过公共包含图找到源码对象，不依赖适配器的身份编码和内部 AST。
fn named<'a>(graph: &'a CodeGraph, name: &str) -> &'a CodeNode {
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

/// 不支持的目标语法须按项目范围拒绝，不能默认为宿主或最新版本解析。
#[test]
fn project_requires_python_controls_syntax_before_starting_the_tool() {
    let server = FakeServer::new(vec![]);
    fs::write(
        server.workspace().join("main.py"),
        "match item:\n    case 1: pass\n",
    )
    .unwrap();
    fs::write(
        server.workspace().join("pyproject.toml"),
        "[project]\nrequires-python = '>=3.8'\n",
    )
    .unwrap();
    let result = PythonLanguageAnalyzer::new(server.workspace().join("missing-tool")).analyze(
        &AnalysisInput {
            repository_root: server.workspace().into(),
            files: vec!["main.py".into()],
            options: Default::default(),
        },
    );
    let error = result.unwrap_err().message;
    assert!(
        error.contains("语法") && error.contains("main.py"),
        "{error}"
    );
    assert!(!server.workspace().join("pid").exists());
}

/// 测试替身只实现标准协议，未知定义返回 null；不依赖待测 AST 的结构。
fn graph_server() -> FakeServer {
    let server = FakeServer::new(vec![]);
    fs::write(server.workspace().join("server"), r#"#!/bin/sh
set -eu
printf '%s' "$$" > pid
while IFS= read -r header; do
    IFS= read -r blank
    length=$(printf '%s' "${header#Content-Length: }" | /usr/bin/tr -d '\r')
    body=$(/bin/dd bs=1 count="$length" 2>/dev/null)
    printf '%s\n' "$body" >> requests
    case "$body" in
        *'"method":"exit"'*) exit 0 ;;
        *'"method":"initialize"'*) result='{"capabilities":{"definitionProvider":true,"positionEncoding":"utf-16"}}' ;;
        *'"method":"textDocument/definition"'*|*'"method":"shutdown"'*) result=null ;;
        *) continue ;;
    esac
    id=$(printf '%s' "$body" | /usr/bin/sed -E 's/.*"id":([0-9]+).*/\1/')
    response="{\"jsonrpc\":\"2.0\",\"id\":$id,\"result\":$result}"
    printf 'Content-Length: %s\r\n\r\n%s' "${#response}" "$response"
done
"#).unwrap();
    server
}

/// 链接目标伪装成环境目录也不能跳过启动前检查。
#[test]
fn escaping_links_fail_before_starting_the_language_server() {
    let server = graph_server();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("pyvenv.cfg"), "home = /outside\n").unwrap();
    fs::write(outside.path().join("api.py"), "def external(): pass\n").unwrap();
    std::os::unix::fs::symlink(outside.path(), server.workspace().join("linked")).unwrap();
    fs::write(server.workspace().join("main.py"), "def local(): pass\n").unwrap();
    let result =
        PythonLanguageAnalyzer::new(server.workspace().join("server")).analyze(&AnalysisInput {
            repository_root: server.workspace().into(),
            files: vec!["main.py".into()],
            options: Default::default(),
        });
    assert!(result.unwrap_err().message.contains("逃逸"));
    assert!(!server.workspace().join("pid").exists());
}

/// AST 规范化忽略排版和普通注释，但保存文档字符串与有序初始化语义。
#[test]
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

/// 输入验证先于测试过滤；缺少工具不影响被明确排除的测试文件。
#[test]
fn test_filter_and_input_failures_never_hide_invalid_source_paths() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("tests")).unwrap();
    fs::write(
        root.path().join("tests/test_api.py"),
        "def test_it(): pass\n",
    )
    .unwrap();
    fs::write(root.path().join("notes.txt"), "text").unwrap();
    let analyzer = PythonLanguageAnalyzer::new(root.path().join("missing"));
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
    assert!(analyzer.analyze(&input).is_err());
}

/// 范围选择按可用 patch 判断，脚本约束与显式项目版本遵循已确认优先级。
#[test]
fn target_selection_matches_the_controlled_protocol_configuration() {
    let cases = [
        (
            "[project]\nrequires-python = '>=3.8,!=3.8.*'",
            "",
            "",
            "3.9",
        ),
        ("[project]\nrequires-python = '>=3.10.5'", "", "", "3.10"),
        (
            "[project]\nrequires-python = '>=3.8'\n[tool.ty.environment]\npython-version = '3.9'",
            "[environment]\npython-version = '3.12'",
            "",
            "3.12",
        ),
        (
            "[project]\nrequires-python = '>=3.8'",
            "",
            "# /// script\n# requires-python = '>=3.11'\n# ///\n",
            "3.11",
        ),
        ("", "", "", "3.14"),
        ("[project]\nrequires-python = ''", "", "", "3.14"),
        (
            "",
            "",
            "# /// script\r# requires-python = '>=3.12'\r# ///\r",
            "3.12",
        ),
    ];
    for (pyproject, ty, prefix, expected) in cases {
        let server = graph_server();
        if !pyproject.is_empty() {
            fs::write(server.workspace().join("pyproject.toml"), pyproject).unwrap();
        }
        if !ty.is_empty() {
            fs::write(server.workspace().join("ty.toml"), ty).unwrap();
        }
        fs::write(
            server.workspace().join("main.py"),
            format!("{prefix}def service(): pass\n"),
        )
        .unwrap();
        PythonLanguageAnalyzer::new(server.workspace().join("server"))
            .analyze(&AnalysisInput {
                repository_root: server.workspace().into(),
                files: vec!["main.py".into()],
                options: Default::default(),
            })
            .unwrap();
        let requests = server.requests();
        let options = &requests[0]["params"]["initializationOptions"];
        assert_eq!(
            options["configuration"]["environment"]["python-version"],
            expected
        );
        assert_eq!(
            options["configuration"]["environment"]["python-platform"],
            "all"
        );
        assert_eq!(options["untrustedWorkspace"], true);
        assert_eq!(options["experimental"]["useUv"], "off");
        assert!(!Path::new(options["configurationFile"].as_str().unwrap())
            .starts_with(server.workspace()));
    }
}

/// 不支持的目标、无交集约束和非 UTF-8 声明均在启动前报告明确失败。
#[test]
fn invalid_versions_and_encodings_fail_before_startup() {
    for (config, source) in [
        ("[environment]\npython-version='3.6'", "def run(): pass"),
        ("[environment]\npython-version='3.15'", "def run(): pass"),
        ("", "# coding: latin-1\ndef run(): pass"),
        ("", "#!/usr/bin/python\r# coding: latin-1\rdef run(): pass"),
    ] {
        let server = graph_server();
        fs::write(server.workspace().join("ty.toml"), config).unwrap();
        fs::write(server.workspace().join("main.py"), source).unwrap();
        assert!(
            PythonLanguageAnalyzer::new(server.workspace().join("server"))
                .analyze(&AnalysisInput {
                    repository_root: server.workspace().into(),
                    files: vec!["main.py".into()],
                    options: Default::default()
                })
                .is_err()
        );
        assert!(!server.workspace().join("pid").exists());
    }
}

/// 调用方取消、协议坐标错误、超时与 panic 均先回收进程，再允许调用返回。
#[test]
fn failure_and_cancellation_reap_the_python_process() {
    use std::sync::atomic::{AtomicBool, Ordering};
    for mode in ["timeout", "cancel", "panic", "coordinate"] {
        let server = graph_server();
        if mode != "coordinate" {
            fs::write(
                server.workspace().join("server"),
                "#!/bin/sh\nprintf '%s' \"$$\" > pid\n/bin/sleep 30\n",
            )
            .unwrap();
        } else {
            let script = fs::read_to_string(server.workspace().join("server")).unwrap();
            let location = serde_json::json!({"uri":crate::lsp::file_uri(&server.workspace().join("main.py")).unwrap(),"range":{"start":{"line":99,"character":0},"end":{"line":99,"character":1}}});
            fs::write(
                server.workspace().join("server"),
                script.replace("result=null", &format!("result='[{}]'", location)),
            )
            .unwrap();
        }
        fs::write(server.workspace().join("main.py"), "def run(): unknown()\n").unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let pid_file = server.workspace().join("pid");
        let signal = stop.clone();
        let started_file = pid_file.clone();
        let trigger = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while !started_file.exists() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(Duration::from_millis(50));
            signal.store(true, Ordering::SeqCst);
        });
        let analyzer = PythonLanguageAnalyzer::new(server.workspace().join("server"))
            .with_timeout(Duration::from_secs(1))
            .with_cancellation(Arc::new(move || {
                if mode == "panic" && pid_file.exists() {
                    panic!("模拟调用方观察器 panic");
                }
                mode == "cancel" && stop.load(Ordering::SeqCst)
            }));
        let started = std::time::Instant::now();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            analyzer.analyze(&AnalysisInput {
                repository_root: server.workspace().into(),
                files: vec!["main.py".into()],
                options: Default::default(),
            })
        }));
        trigger.join().unwrap();
        if mode == "panic" {
            assert!(result.is_err());
        } else {
            let error = result.unwrap().unwrap_err();
            assert_eq!(error.is_cancelled(), mode == "cancel");
        }
        assert!(started.elapsed() < Duration::from_secs(3), "{mode}");
        assert!(
            server.workspace().join("pid").exists(),
            "{mode}: 未记录启动 PID"
        );
        let pid = server.pid();
        assert_eq!(
            unsafe { libc::kill(pid as i32, 0) },
            -1,
            "{mode}: 进程 {pid} 未退出"
        );
    }
}
