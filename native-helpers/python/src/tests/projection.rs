//! 固定旧 registry Ruff 与新同源 API 的实际对照，不用新引擎自证兼容。
use super::*;

/// 旧 registry 引擎与新同源引擎实际对照类、property、overload 和字段内容。
#[test]
fn old_ruff_and_native_ast_unparse_preserve_projection_contract() {
    let source = "from typing import overload, TypeAlias\n\nclass Box:\n    \"\"\"docs  with spaces\\nand newline\"\"\"\n    field: str = 'a  b'\n    @property\n    def value(self) -> str:\n        return self.field\n    @value.setter\n    def value(self, value: str):\n        self.field = value\n\n@overload\ndef run(x: int) -> int: ...\n@overload\ndef run(x: str) -> str: ...\ndef run(x):\n    return x\n\nAlias: TypeAlias = list[int]\n";
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("main.py"), source).unwrap();
    let project = PythonProject::load(ProjectInput {
        repository: directory.path().into(),
        project_root: PathBuf::new(),
        source_roots: vec![PathBuf::new()],
        python_minor: 14,
    })
    .unwrap();
    let native = project.parse(Path::new("main.py")).unwrap();
    let old = old_parser::parse_module(source).unwrap();
    let old_projection = old
        .syntax()
        .body
        .iter()
        .map(|statement| {
            old_codegen::Generator::new(
                &old_codegen::Indentation::default(),
                old_source::LineEnding::Lf,
            )
            .with_mode(old_codegen::Mode::AstUnparse)
            .stmt(statement)
        })
        .collect::<Vec<_>>();
    let native_projection = native
        .syntax()
        .body
        .iter()
        .map(normalize_statement)
        .collect::<Vec<_>>();
    assert_eq!(native_projection, old_projection);
    assert_eq!(native.syntax().body.len(), old.syntax().body.len());
    assert!(native_projection[1].contains("a  b"));
}

/// 旧 API 投影使用原有 AST 模式与空函数/类体头，不迁移任何图身份或绑定逻辑。
fn old_projection(statement: &old_ast::Stmt) -> String {
    let mut header = statement.clone();
    match &mut header {
        old_ast::Stmt::FunctionDef(value) => value.body.clear(),
        old_ast::Stmt::ClassDef(value) => value.body.clear(),
        _ => {}
    }
    old_codegen::Generator::new(
        &old_codegen::Indentation::default(),
        old_source::LineEnding::Lf,
    )
    .with_mode(old_codegen::Mode::AstUnparse)
    .stmt(&header)
}

/// 新 API 对同样的 AST 字段执行同样投影，检查真实类型接口而非手工预期字符串。
fn native_projection(statement: &Stmt) -> String {
    let mut header = statement.clone();
    match &mut header {
        Stmt::FunctionDef(value) => value.body.clear(),
        Stmt::ClassDef(value) => value.body.clear(),
        _ => {}
    }
    normalize_statement(&header)
}

/// 对照每个目标版本的 AST 字段操作、声明头规范化与原文范围。
#[test]
fn old_and_native_projection_interfaces_match_across_supported_syntax() {
    use old_text::Ranged as _;
    let rows = [
        (
            7,
            "\u{feff}# comment\r\nasync def 中文(x: str = 'a  b') -> str:\r\n    \"\"\"line one\r\nline two\"\"\"\r\n    return x\r\n",
        ),
        (8, "def f(xs):\n    return [y for x in xs if (y := x)]\n"),
        (9, "class Box:\n    field: list[int] = []\n"),
        (10, "match subject:\n    case {'name': name}: print(name)\n"),
        (11, "try:\n    run()\nexcept* ValueError:\n    pass\n"),
        (
            12,
            "type Alias[T] = list[T]\ndef identity[T](x: T) -> T:\n    return x\n",
        ),
        (13, "def identity[T = str](x: T) -> T:\n    return x\n"),
        (14, "message = t'hello {name}'\n"),
    ];
    for (minor, source) in rows {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("main.py"), source).unwrap();
        let project = PythonProject::load(ProjectInput {
            repository: directory.path().into(),
            project_root: PathBuf::new(),
            source_roots: vec![PathBuf::new()],
            python_minor: minor,
        })
        .unwrap();
        let native = project.parse(Path::new("main.py")).unwrap();
        let old = old_parser::parse_unchecked(
            source,
            old_parser::ParseOptions::from(old_ast::PySourceType::Python)
                .with_target_version(old_ast::PythonVersion { major: 3, minor }),
        );
        assert!(old.errors().is_empty());
        assert!(old.unsupported_syntax_errors().is_empty());
        let old_ast::Mod::Module(old) = old.into_syntax() else {
            panic!("模块 AST")
        };
        assert_eq!(native.syntax().body.len(), old.body.len());
        for (native, old) in native.syntax().body.iter().zip(&old.body) {
            assert_eq!(
                native_projection(native),
                old_projection(old),
                "Python 3.{minor}"
            );
            let native_range = native.range();
            let old_range = old.range();
            assert_eq!(
                &source[native_range], &source[old_range],
                "Python 3.{minor}"
            );
        }
    }
}

/// 字符串内部空白和文档字符串换行的语义差异必须参与内容比较。
#[test]
fn normalization_keeps_semantic_string_and_docstring_whitespace() {
    let rows = [
        "def f():\n    return 'a  b'\n",
        "def f():\n    return 'a b'\n",
        "def f():\n    \"\"\"a\nb\"\"\"\n    pass\n",
        "def f():\n    \"\"\"a b\"\"\"\n    pass\n",
    ];
    let mut outputs = Vec::new();
    for source in rows {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("main.py"), source).unwrap();
        let project = PythonProject::load(ProjectInput {
            repository: directory.path().into(),
            project_root: PathBuf::new(),
            source_roots: vec![PathBuf::new()],
            python_minor: 14,
        })
        .unwrap();
        let native = project.parse(Path::new("main.py")).unwrap();
        let old = old_parser::parse_module(source).unwrap();
        let native = normalize_statement(&native.syntax().body[0]);
        let old = old_codegen::Generator::new(
            &old_codegen::Indentation::default(),
            old_source::LineEnding::Lf,
        )
        .with_mode(old_codegen::Mode::AstUnparse)
        .stmt(&old.syntax().body[0]);
        assert_eq!(native, old);
        outputs.push(native);
    }
    assert_ne!(outputs[0], outputs[1]);
    assert_ne!(outputs[2], outputs[3]);
}

/// 父内容中的合成声明锚点沿用两套 AST 的同名字段与打印规则，避免迁移时改变顺序语义。
#[test]
fn synthetic_declaration_anchor_interfaces_match() {
    let name = "<declare module:class:Box:0#0>";
    let native = Stmt::Expr(ruff_python_ast::StmtExpr {
        node_index: ruff_python_ast::AtomicNodeIndex::NONE,
        range: Default::default(),
        value: Box::new(ruff_python_ast::Expr::Name(ruff_python_ast::ExprName {
            node_index: ruff_python_ast::AtomicNodeIndex::NONE,
            range: Default::default(),
            id: name.into(),
            ctx: ruff_python_ast::ExprContext::Load,
        })),
    });
    let old = old_ast::Stmt::Expr(old_ast::StmtExpr {
        node_index: old_ast::AtomicNodeIndex::NONE,
        range: Default::default(),
        value: Box::new(old_ast::Expr::Name(old_ast::ExprName {
            node_index: old_ast::AtomicNodeIndex::NONE,
            range: Default::default(),
            id: name.into(),
            ctx: old_ast::ExprContext::Load,
        })),
    });
    assert_eq!(normalize_statement(&native), old_projection(&old));
    assert_eq!(normalize_statement(&native), name);
}
