//! 旧 native.rs 的业务断言迁至实际 Python helper；生产双平台安装验收仍待后续门禁。
use super::python_graph::{analyzer, named};
use crate::structure_review::code_graph::{
    AnalysisInput, AnalysisOptions, CodeGraph, LanguageAnalyzer,
};
use crate::structure_review::code_graph::{CodeNode, ReferenceTarget};
use std::fs;
use std::path::PathBuf;

/// 使用真实二进制分析临时仓库，不依赖宿主 Python 或项目依赖安装。
fn analyze_native(files: &[(&str, &str)], selected: &[&str]) -> CodeGraph {
    analyze_with_options(files, selected, Default::default())
}

/// 在公开分析接口传递测试选项，仍通过同一个真实 helper。
fn analyze_with_options(
    files: &[(&str, &str)],
    selected: &[&str],
    options: AnalysisOptions,
) -> CodeGraph {
    let root = tempfile::tempdir().unwrap();
    for (name, source) in files {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    analyzer()
        .analyze(&AnalysisInput {
            repository_root: root.path().into(),
            files: selected.iter().map(PathBuf::from).collect(),
            options,
        })
        .unwrap()
}

/// 仓库调用连到选中节点，标准库有固定外部身份，缺第三方保持未解析。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_repository_stdlib_and_missing_dependency_relations() {
    let graph = analyze_native(&[
        ("main.py", "from api import service\nimport absent_package\n\ndef run():\n    service()\n    len([])\n    absent_package.fetch()\n"),
        ("api.py", "def service():\n    return 1\n"),
    ], &["main.py", "api.py"]);
    let refs = &named(&graph, "run").data().references;
    let service = &named(&graph, "service").data().id;
    assert!(
        refs.iter()
            .any(|reference| reference.semantic == "code.calls"
                && reference.target == ReferenceTarget::Node(service.clone())),
        "{refs:?}"
    );
    assert!(refs.iter().any(|reference| matches!(&reference.target, ReferenceTarget::External { symbol } if graph.external_symbols().get(symbol).is_some_and(|symbol| symbol.name == "len"))), "{refs:?}");
    assert!(refs.iter().any(|reference| matches!(&reference.target, ReferenceTarget::Unresolved { name } if name == "absent_package.fetch")), "{refs:?}");
}

/// 工具返回单一定义也不能覆盖 AST 中已知的条件或重复赋值歧义。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_aliases_require_single_unconditional_binding() {
    let graph = analyze_native(&[("main.py", "def first(): pass\ndef second(): pass\ngood = first\nif unknown:\n    conditional = first\nelse:\n    conditional = second\nrebound = first\nrebound = second\ndef run():\n    good()\n    conditional()\n    rebound()\n")], &["main.py"]);
    let refs = &named(&graph, "run").data().references;
    assert!(refs.iter().any(|reference| reference.target
        == ReferenceTarget::Node(named(&graph, "first").data().id.clone())));
    for alias in ["conditional", "rebound"] {
        assert!(refs.iter().any(|reference| matches!(&reference.target, ReferenceTarget::Unresolved { name } if name == alias)), "{refs:?}");
    }
    assert!(
        !refs.iter().any(|reference| reference.target
            == ReferenceTarget::Node(named(&graph, "second").data().id.clone())),
        "{refs:?}"
    );
}

/// 只有真实标准库绑定可改变对象分类，访问器与合法 overload 组保留源码角色。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_properties_overloads_and_explicit_aliases_keep_source_ownership() {
    let graph = analyze_native(&[("main.py", "from typing import overload, TypeAlias\nAlias: TypeAlias = int\ntype Other = str\nclass Service:\n    @property\n    def item(self): return self._item\n    @item.setter\n    def item(self, value): self._item = value\n    @item.deleter\n    def item(self): del self._item\n    @overload\n    def transform(self, value: int) -> int: ...\n    @overload\n    def transform(self, value: str) -> str: ...\n    def transform(self, value): return value\n")], &["main.py"]);
    assert!(matches!(named(&graph, "Alias"), CodeNode::Type(_)));
    assert!(matches!(named(&graph, "Other"), CodeNode::Type(_)));
    let members = named(&graph, "Service")
        .data()
        .contains
        .iter()
        .map(|id| graph.node(id).unwrap())
        .collect::<Vec<_>>();
    let mut roles = members
        .iter()
        .filter(|node| node.data().name.as_deref() == Some("item"))
        .map(|node| node.data().native_kind.as_str())
        .collect::<Vec<_>>();
    roles.sort();
    assert_eq!(roles, ["deleter", "getter", "setter"]);
    assert_eq!(
        members
            .iter()
            .filter(|node| node.data().name.as_deref() == Some("transform"))
            .count(),
        1
    );
    assert!(named(&graph, "transform")
        .data()
        .signature
        .as_ref()
        .unwrap()
        .contains("value: str"));
    assert!(matches!(named(&graph, "_item"), CodeNode::Value(_)));
}

/// 未知装饰器是目标歧义的反证，不能删掉后把另一个联合成员当成唯一调用。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_union_with_unknown_decorator_stays_unresolved() {
    let graph = analyze_native(&[("main.py", "def opaque(f): return f\nclass A:\n    def act(self): pass\nclass B:\n    @opaque\n    def act(self): pass\ndef run(obj: A | B):\n    obj.act()\n")], &["main.py"]);
    let calls = named(&graph, "run")
        .data()
        .references
        .iter()
        .filter(|reference| reference.semantic == "code.calls")
        .collect::<Vec<_>>();
    assert_eq!(calls.len(), 1, "{calls:?}");
    assert!(
        matches!(&calls[0].target, ReferenceTarget::Unresolved { name } if name == "obj.act"),
        "{calls:?}"
    );
}

/// 物理文件相同不意味着 Python 模块相同；链接中的显式 import 保留真正目标路径。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_internal_links_do_not_rewrite_definition_ownership() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("impl")).unwrap();
    fs::write(
        root.path().join("impl/__init__.py"),
        "import impl\ndef service(): return impl.service()\n",
    )
    .unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink("impl", root.path().join("alias")).unwrap();
    #[cfg(windows)]
    assert!(std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(root.path().join("alias"))
        .arg(root.path().join("impl"))
        .status()
        .unwrap()
        .success());
    let analyzer = analyzer();
    let input = |files: &[&str]| AnalysisInput {
        repository_root: root.path().into(),
        files: files.iter().map(PathBuf::from).collect(),
        options: Default::default(),
    };
    let single = analyzer.analyze(&input(&["alias/__init__.py"])).unwrap();
    let both = analyzer
        .analyze(&input(&["alias/__init__.py", "impl/__init__.py"]))
        .unwrap();
    let alias = named(&single, "service");
    let reference = alias
        .data()
        .references
        .iter()
        .find(|reference| reference.semantic == "code.calls")
        .unwrap();
    let ReferenceTarget::External { symbol } = &reference.target else {
        panic!("import impl 不得重写为 alias 自调用：{reference:?}");
    };
    let actual = crate::structure_review::code_graph::NodeId(symbol.clone());
    assert_ne!(actual, alias.data().id);
    assert!(both.node(&actual).is_some());
    assert!(both
        .node(&alias.data().id)
        .unwrap()
        .data()
        .references
        .iter()
        .any(|reference| reference.target == ReferenceTarget::Node(actual.clone())));

    fs::write(
        root.path().join("main.py"),
        "import alias\ndef run(): return alias.service()\n",
    )
    .unwrap();
    let unselected = analyzer.analyze(&input(&["main.py"])).unwrap();
    let selected = analyzer
        .analyze(&input(&["main.py", "alias/__init__.py"]))
        .unwrap();
    let target = named(&unselected, "run")
        .data()
        .references
        .iter()
        .find(|reference| reference.semantic == "code.calls")
        .unwrap();
    let ReferenceTarget::External { symbol } = &target.target else {
        panic!("{target:?}")
    };
    let target_id = crate::structure_review::code_graph::NodeId(symbol.clone());
    assert!(selected.node(&target_id).is_some());
    assert!(named(&selected, "run")
        .data()
        .references
        .iter()
        .any(|reference| reference.target == ReferenceTarget::Node(target_id.clone())));
}

/// overload 的有效文档属于对应签名，修改文档必须修改合并后的 Callable。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_overload_docstrings_participate_in_content_comparison() {
    for name in ["api.py", "api.pyi"] {
        let mut source = "from typing import overload\n@overload\ndef convert(x: int) -> int:\n    \"old doc\"\n    ...\n@overload\ndef convert(x: str) -> str: ...\n".to_owned();
        if name.ends_with(".py") {
            source.push_str("def convert(x): return x\n");
        }
        let before = analyze_native(&[(name, &source)], &[name]);
        let after = analyze_native(&[(name, &source.replace("old doc", "new doc"))], &[name]);
        let difference = before.diff(&after).unwrap();
        assert_eq!(difference.nodes.len(), 1, "{name}: {difference:?}");
        assert_eq!(difference.nodes[0].id, named(&before, "convert").data().id);
        assert_eq!(
            difference.nodes[0].content,
            Some(crate::structure_review::code_graph::ChangeKind::Modified)
        );
    }
}

/// property 链遇到显式重绑定后失效，合法 getter 替换则保留访问器身份。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_property_chains_require_current_lexical_binding() {
    for (middle, decorator, expected) in [
        ("", "getter", "getter"),
        (
            "    from helper import replacement as item\n",
            "setter",
            "method",
        ),
        ("    import helper as item\n", "setter", "method"),
        ("    del item\n", "setter", "method"),
        ("    type item = int\n", "setter", "method"),
        (
            "    def other(value=(item := replacement)): pass\n",
            "setter",
            "method",
        ),
    ] {
        let source = format!("class Service:\n    @property\n    def item(self): return 1\n{middle}    @item.{decorator}\n    def item(self, value=None): pass\n");
        let graph = analyze_native(
            &[
                ("main.py", &source),
                ("helper.py", "replacement = object()\n"),
            ],
            &["main.py"],
        );
        let roles = named(&graph, "Service")
            .data()
            .contains
            .iter()
            .map(|id| graph.node(id).unwrap())
            .filter(|node| {
                matches!(node, CodeNode::Callable(_)) && node.data().name.as_deref() == Some("item")
            })
            .map(|node| node.data().native_kind.as_str())
            .collect::<Vec<_>>();
        assert_eq!(roles, ["getter", expected], "{middle:?} {decorator}");
    }
    // 当前 decorator 已先取得 setter，默认值临时重绑不改变最终重新赋回的 property。
    let graph = analyze_native(&[("main.py", "class Service:\n    @property\n    def item(self): return 1\n    @item.setter\n    def item(self, value=(item := None)): pass\n    @item.deleter\n    def item(self): pass\n")], &["main.py"]);
    let roles = named(&graph, "Service")
        .data()
        .contains
        .iter()
        .map(|id| graph.node(id).unwrap())
        .filter(|node| matches!(node, CodeNode::Callable(_)))
        .map(|node| node.data().native_kind.as_str())
        .collect::<Vec<_>>();
    assert_eq!(roles, ["getter", "setter", "deleter"]);
}

/// 各受支持 minor 都使用对应目标；旧版标准库关系实际运行而非只检查 --version。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_all_supported_minors_preserve_local_and_stdlib_navigation() {
    for minor in 7..=14 {
        let configuration = format!("[environment]\npython-version = '3.{minor}'\n");
        let graph = analyze_native(&[("ty.toml", &configuration), ("main.py", "import pathlib\ndef service(): return 1\nclass Worker:\n    def run(this):\n        service()\n        len([])\n        pathlib.Path('x').exists()\n        'abc'.upper()\n")], &["main.py"]);
        let refs = &named(&graph, "run").data().references;
        assert!(
            refs.iter().any(|reference| reference.target
                == ReferenceTarget::Node(named(&graph, "service").data().id.clone())),
            "3.{minor}: {refs:?}"
        );
        assert!(refs.iter().any(|reference| matches!(&reference.target, ReferenceTarget::External { symbol } if graph.external_symbols()[symbol].name == "len")), "3.{minor}: {refs:?}");
        assert!(refs.iter().any(|reference| matches!(&reference.target, ReferenceTarget::External { symbol } if graph.external_symbols()[symbol].name == "upper")), "3.{minor}: {refs:?}");
        // 固定 all 平台时 ty 返回 exists 的两个声明，保留歧义而不选第一项。
        assert!(refs.iter().any(|reference| matches!(&reference.target, ReferenceTarget::Unresolved { name } if name == "pathlib.Path('x').exists")), "3.{minor}: {refs:?}");
    }
}

/// 实例字段注解归字段，赋值构造和嵌套调用仍归外层方法；外部源码不扩充成员范围。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_field_type_relations_and_nested_calls_have_distinct_owners() {
    let graph = analyze_native(&[
        ("base.py", "class Base: pass\n"),
        ("main.py", "from base import Base\nclass Payload: pass\ndef helper(): pass\nclass Worker(Base):\n    def run(receiver):\n        receiver.payload: Payload = Payload()\n        def nested():\n            helper()\n        return helper\n"),
    ], &["main.py"]);
    let field = named(&graph, "payload");
    let payload = named(&graph, "Payload").data().id.clone();
    assert!(
        field
            .data()
            .references
            .iter()
            .any(|reference| reference.semantic == "code.type_use"
                && reference.target == ReferenceTarget::Node(payload.clone())),
        "{:?}",
        field.data().references
    );
    let refs = &named(&graph, "run").data().references;
    assert!(refs
        .iter()
        .any(|reference| reference.semantic == "code.calls"
            && reference.target == ReferenceTarget::Node(payload.clone())));
    let helper = named(&graph, "helper").data().id.clone();
    for semantic in ["code.calls", "code.references"] {
        assert!(
            refs.iter().any(|reference| reference.semantic == semantic
                && reference.target == ReferenceTarget::Node(helper.clone())),
            "{refs:?}"
        );
    }
    assert!(named(&graph, "Worker").data().references.iter().any(|reference| reference.semantic == "code.extends" && matches!(&reference.target, ReferenceTarget::External { symbol } if graph.external_symbols()[symbol].scope == crate::structure_review::code_graph::SymbolScope::Repository)));
    assert_eq!(graph.root().contains.len(), 1);
}

/// 别名转发不能绕开它所引用的重绑定证据。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_alias_chains_do_not_erase_rebinding_evidence() {
    let graph = analyze_native(&[("main.py", "def first(): pass\ndef second(): pass\noriginal = first\noriginal = second\nforward = original\ndef run(): forward()\n")], &["main.py"]);
    assert!(named(&graph, "run").data().references.iter().any(|reference| matches!(&reference.target, ReferenceTarget::Unresolved { name } if name == "forward")));
}

/// 不同项目的同名模块和相对 src 路径必须分别解释，不能共用混合搜索根。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_projects_keep_module_search_and_selected_identity_separate() {
    let files = [
        ("alpha/pyproject.toml", "[project]\nname='alpha'\nrequires-python='>=3.8'\n[tool.ty.environment]\nroot=['src']\n"),
        ("alpha/src/api.py", "def serve_alpha(): return 1\n"),
        ("alpha/src/main.py", "from api import serve_alpha\ndef alpha_run(): return serve_alpha()\n"),
        ("beta/ty.toml", "[environment]\nroot=['src']\npython-version='3.14'\n"),
        ("beta/src/api.py", "def serve_beta(): return 2\n"),
        ("beta/src/main.py", "from api import serve_beta\ndef beta_run(): return serve_beta()\n"),
    ];
    let graph = analyze_native(
        &files,
        &[
            "alpha/src/main.py",
            "alpha/src/api.py",
            "beta/src/main.py",
            "beta/src/api.py",
        ],
    );
    for (caller, target) in [("alpha_run", "serve_alpha"), ("beta_run", "serve_beta")] {
        assert!(named(&graph, caller)
            .data()
            .references
            .iter()
            .any(|reference| reference.target
                == ReferenceTarget::Node(named(&graph, target).data().id.clone())));
    }
    let selected = analyze_native(&files, &["alpha/src/main.py"]);
    assert_eq!(
        named(&graph, "alpha_run").data().id,
        named(&selected, "alpha_run").data().id
    );
    assert!(named(&selected, "alpha_run").data().references.iter().any(|reference| matches!(&reference.target, ReferenceTarget::External { symbol } if symbol == &named(&graph, "serve_alpha").data().id.0)));
}

/// UTF-8 BOM、非 BMP 字符和 Python 的三种换行都使用同一原文坐标。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_unicode_bom_and_python_line_endings_preserve_definitions() {
    for newline in ["\n", "\r\n", "\r"] {
        let source = "\u{feff}# coding: utf-8\ndef 服务(): return 1\ndef 运行():\n    marker = '🐍'; 服务()\n".replace('\n', newline);
        let graph = analyze_native(&[("中文.py", &source)], &["中文.py"]);
        assert!(
            named(&graph, "运行")
                .data()
                .references
                .iter()
                .any(|reference| reference.target
                    == ReferenceTarget::Node(named(&graph, "服务").data().id.clone())),
            "换行 {newline:?}"
        );
        assert!(named(&graph, "运行")
            .data()
            .source
            .as_ref()
            .unwrap()
            .contains("🐍"));
    }
}

/// .pyi 和 .py 保持独立所有权；伪装的装饰器与普通同名声明不能合并。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_stubs_and_shadowed_decorators_keep_separate_declarations() {
    let graph = analyze_native(&[
        ("api.pyi", "from typing import overload\n@overload\ndef convert(x: int) -> int: ...\n@overload\ndef convert(x: str) -> str: ...\n"),
        ("api.py", "def convert(x): return x\n"),
        ("main.py", "from api import convert\ndef run(): return convert(1)\ndef overload(f): return f\n@overload\ndef duplicate(x: int): ...\n@overload\ndef duplicate(x: str): ...\ndef duplicate(x): return x\ndef property(f): return f\nclass Service:\n    @property\n    def item(self): return 1\n"),
    ], &["api.pyi", "api.py", "main.py"]);
    let stub = named(&graph, "api.pyi");
    let stub_convert = stub
        .data()
        .contains
        .iter()
        .filter_map(|id| graph.node(id))
        .find(|node| node.data().name.as_deref() == Some("convert"))
        .unwrap();
    assert_eq!(stub_convert.data().code, None);
    assert!(stub_convert
        .data()
        .signature
        .as_ref()
        .unwrap()
        .contains("x: str"));
    let implementation = named(&graph, "api.py")
        .data()
        .contains
        .iter()
        .filter_map(|id| graph.node(id))
        .find(|node| node.data().name.as_deref() == Some("convert"))
        .unwrap();
    assert_ne!(implementation.data().id, stub_convert.data().id);
    assert!(named(&graph, "run").data().references.iter().any(
        |reference| reference.target == ReferenceTarget::Node(implementation.data().id.clone())
    ));
    let main = named(&graph, "main.py");
    assert_eq!(
        main.data()
            .contains
            .iter()
            .filter(|id| graph.node(id).unwrap().data().name.as_deref() == Some("duplicate"))
            .count(),
        3
    );
    assert_eq!(named(&graph, "item").data().native_kind, "method");
}

/// 继承的 property 写入与接收者 except 重绑定不能制造实例字段。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_descriptor_writes_and_exception_receivers_do_not_create_fields() {
    let graph = analyze_native(&[("main.py", "class Base:\n    @property\n    def item(self): return 1\n    @item.setter\n    def item(self, value): pass\nclass Derived(Base):\n    def update(receiver):\n        receiver.item = 2\n        receiver.safe = 3\nclass Rebound:\n    def run(receiver):\n        try: pass\n        except Exception as receiver: pass\n        receiver.wrong = 1\n")], &["main.py"]);
    let derived = named(&graph, "Derived");
    let fields = derived
        .data()
        .contains
        .iter()
        .filter_map(|id| graph.node(id))
        .filter(|node| matches!(node, CodeNode::Value(_)))
        .map(|node| node.data().name.as_deref().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(fields, ["safe"]);
    assert!(!named(&graph, "Rebound")
        .data()
        .contains
        .iter()
        .any(|id| matches!(graph.node(id).unwrap(), CodeNode::Value(_))));
}

/// 在独立测试进程中毒化用户环境，避免并发测试修改全局变量。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_user_environment_and_repository_configuration_cannot_add_dependencies() {
    const MARKER: &str = "NARRANT_PYTHON_BOUNDARY_CHILD";
    if let Some(outside) = std::env::var_os(MARKER) {
        let outside = PathBuf::from(outside).to_string_lossy().replace('\\', "/");
        let config = format!("[environment]\nextra-paths=['{outside}']\npython='{outside}/venv'\ntypeshed='{outside}/typeshed'\n");
        let graph = analyze_native(
            &[
                ("ty.toml", &config),
                (
                    ".venv/lib/python3.14/site-packages/poison.py",
                    "def fetch(): return 42\n",
                ),
                (
                    ".venv/Lib/site-packages/poison.py",
                    "def fetch(): return 42\n",
                ),
                (
                    "main.py",
                    "import poison\ndef run():\n    poison.fetch()\n    len([])\n",
                ),
            ],
            &["main.py"],
        );
        let refs = &named(&graph, "run").data().references;
        assert!(refs.iter().any(|reference| matches!(&reference.target, ReferenceTarget::Unresolved { name } if name == "poison.fetch")), "{refs:?}");
        assert!(refs.iter().any(|reference| matches!(&reference.target, ReferenceTarget::External { symbol } if graph.external_symbols()[symbol].name == "len")));
        return;
    }
    let outside = tempfile::tempdir().unwrap();
    let root = outside.path();
    fs::create_dir_all(root.join("ty")).unwrap();
    fs::create_dir_all(root.join("venv/Lib/site-packages")).unwrap();
    fs::create_dir_all(root.join("venv/lib/python3.14/site-packages")).unwrap();
    fs::write(root.join("poison.py"), "def fetch(): return 42\n").unwrap();
    fs::write(root.join("venv/pyvenv.cfg"), "home = /not-a-real-python\n").unwrap();
    let config = format!(
        "[environment]\nextra-paths=['{}']\n",
        root.to_string_lossy().replace('\\', "/")
    );
    fs::write(root.join("ty/ty.toml"), &config).unwrap();
    fs::write(root.join("ty.toml"), &config).unwrap();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "structure_review::native_analyzer::tests::python_native::native_user_environment_and_repository_configuration_cannot_add_dependencies", "--ignored", "--nocapture"])
        .env(MARKER, root).env("HOME", root).env("USERPROFILE", root).env("XDG_CONFIG_HOME", root).env("APPDATA", root)
        .env("PYTHONPATH", root).env("PYTHONHOME", root.join("venv")).env("VIRTUAL_ENV", root.join("venv"))
        .env("CONDA_PREFIX", root.join("venv")).env("TY_CONFIG_FILE", root.join("ty.toml"))
        .output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// 大批查询途中取消必须保持取消类别，并等待原生进程守卫完成回收。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_cancellation_finishes_promptly() {
    use std::{
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    let source = (0..2000)
        .map(|index| format!("def value_{index}(): return len([])\n"))
        .collect::<String>();
    fs::write(root.path().join("main.py"), source).unwrap();
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    let timer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(120));
        signal.store(true, Ordering::Release);
    });
    let start = Instant::now();
    let result = analyzer()
        .with_cancellation(Arc::new(move || cancelled.load(Ordering::Acquire)))
        .analyze(&AnalysisInput {
            repository_root: root.path().into(),
            files: vec!["main.py".into()],
            options: Default::default(),
        });
    timer.join().unwrap();
    assert!(result.unwrap_err().is_cancelled());
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "取消耗时 {:?}",
        start.elapsed()
    );
    eprintln!("Python 原生取消耗时：{:?}", start.elapsed());
}

/// 显式跨项目搜索不能把另一套配置的结果直接升级为确定调用。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_cross_project_configuration_conflicts_remain_unresolved() {
    let root = tempfile::tempdir().unwrap();
    for (name, source) in [
        (
            "alpha/ty.toml",
            "[environment]\npython-version='3.8'\nextra-paths=['../beta/src']\n",
        ),
        (
            "alpha/main.py",
            "from api import serve\ndef run(): return serve()\n",
        ),
        (
            "beta/ty.toml",
            "[environment]\npython-version='3.14'\nroot=['src']\n",
        ),
        (
            "beta/src/api.py",
            "type Payload = int\ndef serve(): return 1\n",
        ),
    ] {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    // 上下文与选中两种情况都不按 alpha 版本解析 beta；选中时 beta 仍在自身组中校验。
    for selected in [
        vec!["alpha/main.py"],
        vec!["alpha/main.py", "beta/src/api.py"],
    ] {
        let input = AnalysisInput {
            repository_root: root.path().into(),
            files: selected.iter().map(PathBuf::from).collect(),
            options: Default::default(),
        };
        let graph = analyzer().analyze(&input).unwrap();
        assert!(named(&graph, "run").data().references.iter().any(|reference| matches!(&reference.target, ReferenceTarget::Unresolved { name } if name == "serve")));
        if selected.len() == 2 {
            assert!(matches!(named(&graph, "Payload"), CodeNode::Type(_)));
            assert!(matches!(named(&graph, "serve"), CodeNode::Callable(_)));
        }
        let old = crate::structure_review::python_analyzer::PythonLanguageAnalyzer::new(
            std::env::var_os("NARRANT_TY_PATH").expect("需固定旧 ty 路径"),
        )
        .analyze(&input)
        .unwrap();
        assert!(old.diff(&graph).unwrap().nodes.is_empty());
        assert_eq!(old.root(), graph.root());
        assert_eq!(old.external_symbols(), graph.external_symbols());
        let mut pending = old.root().contains.clone();
        while let Some(id) = pending.pop() {
            let node = old.node(&id).unwrap();
            pending.extend(node.data().contains.clone());
            assert_eq!(node.data().source, graph.node(&id).unwrap().data().source);
        }
    }
}

/// 装饰器与继承候选同样先应用项目冲突，未知基类不能确认实例字段。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_cross_project_modern_decorator_and_base_remain_unproven() {
    let graph = analyze_native(&[
        ("alpha/ty.toml", "[environment]\npython-version='3.8'\nextra-paths=['../beta/src']\n"),
        ("alpha/main.py", "from api import Base, decorate\n@decorate\ndef wrapped(): return 1\nclass Child(Base):\n    def run(self): self.possible_field = 1\n"),
        ("beta/ty.toml", "[environment]\npython-version='3.14'\nroot=['src']\n"),
        ("beta/src/api.py", "type Payload = int\nclass Base: pass\ndef decorate(value): return value\n"),
    ], &["alpha/main.py", "beta/src/api.py"]);
    assert!(matches!(named(&graph, "wrapped"), CodeNode::Callable(_)));
    let child = named(&graph, "Child");
    assert!(child.data().references.iter().any(|reference| reference.semantic == "code.extends" && matches!(&reference.target, ReferenceTarget::Unresolved { name } if name == "Base")));
    assert!(child
        .data()
        .contains
        .iter()
        .all(|id| !matches!(graph.node(id).unwrap(), CodeNode::Value(_))));
    assert!(matches!(named(&graph, "Payload"), CodeNode::Type(_)));
}

/// 跳过冲突上下文的 AST 不可跳过选集校验：beta 的版本限制和普通语法错误仍失败。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_cross_project_selected_target_still_validates_own_syntax() {
    for (minor, target, diagnostic) in [
        (
            "3.8",
            "type Payload = int\ndef serve(): return 1\n",
            "目标版本不支持语法",
        ),
        (
            "3.14",
            "type Payload =\ndef serve(): return 1\n",
            "Python 语法错误",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let config = format!("[environment]\npython-version='{minor}'\nroot=['src']\n");
        for (name, source) in [
            (
                "alpha/ty.toml",
                "[environment]\npython-version='3.8'\nextra-paths=['../beta/src']\n",
            ),
            (
                "alpha/main.py",
                "from api import serve\ndef run(): return serve()\n",
            ),
            ("beta/ty.toml", config.as_str()),
            ("beta/src/api.py", target),
        ] {
            let path = root.path().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, source).unwrap();
        }
        let error = analyzer()
            .analyze(&AnalysisInput {
                repository_root: root.path().into(),
                files: vec!["alpha/main.py".into(), "beta/src/api.py".into()],
                options: Default::default(),
            })
            .unwrap_err();
        assert!(error.to_string().contains(diagnostic), "{minor}: {error}");
        assert!(
            error.to_string().contains("beta/src/api.py"),
            "{minor}: {error}"
        );
        eprintln!("beta {minor} 选集校验：{error}");
    }
}

/// 重复分析记录新会话冷启动和系统缓存后的耗时；查询统计来自实际适配器。
#[test]
#[ignore = "性能验收需要已构建 Python helper，单独串行运行"]
fn native_multifile_performance_records_cold_and_warm_sessions() {
    let mut files = Vec::new();
    for project in 0..3 {
        files.push((
            format!("project_{project}/ty.toml"),
            "[environment]\npython-version='3.14'\n".to_owned(),
        ));
        for file in 0..8 {
            let methods = (0..25)
                .map(|method| format!("    def method_{method}(self): return len([])\n"))
                .collect::<String>();
            files.push((
                format!("project_{project}/module_{file}.py"),
                format!(
                    "class Worker:\n    @property\n    def title(self): return 'title'\n{methods}"
                ),
            ));
        }
    }
    let selected = files
        .iter()
        .filter(|(path, _)| path.ends_with(".py"))
        .map(|(path, _)| path.as_str())
        .collect::<Vec<_>>();
    let borrowed = files
        .iter()
        .map(|(path, source)| (path.as_str(), source.as_str()))
        .collect::<Vec<_>>();
    for round in 0..2 {
        let start = std::time::Instant::now();
        let graph = analyze_native(&borrowed, &selected);
        assert_eq!(graph.root().contains.len(), 24);
        eprintln!(
            "Python 性能 round={round}, projects=3, files=24, methods=600, elapsed={:?}",
            start.elapsed()
        );
    }
}

/// 排除测试时不经关系泄露测试对象；启用后按原选中文件恢复节点目标。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_test_filter_hides_targets_until_enabled() {
    let files = [
        (
            "main.py",
            "from test_api import helper\ndef run(): helper()\n",
        ),
        ("test_api.py", "def helper(): pass\n"),
    ];
    let excluded = analyze_native(&files, &["main.py", "test_api.py"]);
    assert!(named(&excluded, "run").data().references.is_empty());
    assert_eq!(excluded.root().contains.len(), 1);
    let included = analyze_with_options(
        &files,
        &["main.py", "test_api.py"],
        AnalysisOptions {
            include_tests: true,
        },
    );
    assert_eq!(included.root().contains.len(), 2);
    assert!(named(&included, "run")
        .data()
        .references
        .iter()
        .any(|reference| reference.target
            == ReferenceTarget::Node(named(&included, "helper").data().id.clone())));
}

/// Windows junction 同样属于真实路径边界，发现逃逸后不得输出成功图。
#[cfg(windows)]
#[test]
#[ignore = "需要 Windows 原生 Python helper"]
fn native_windows_junction_escape_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(root.path().join("main.py"), "def run(): pass\n").unwrap();
    fs::write(outside.path().join("external.py"), "def external(): pass\n").unwrap();
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(root.path().join("escaped"))
        .arg(outside.path())
        .status()
        .unwrap();
    assert!(status.success());
    let error = analyzer()
        .analyze(&AnalysisInput {
            repository_root: root.path().into(),
            files: vec!["main.py".into()],
            options: Default::default(),
        })
        .unwrap_err();
    assert!(error.message.contains("逃逸仓库"), "{error:?}");
}
