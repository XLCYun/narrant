//! 退役前旧 Ruff 0.0.15 实测输出固定为业务快照，继续验证同源 AST 的内容和原文契约。
use super::*;

/// 退役前实际旧引擎完整输出保留类、property、overload 和字段内容断言。
#[test]
fn historical_ast_unparse_preserves_projection_contract() {
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
    let old_projection: Vec<String> =
        serde_json::from_str(include_str!("fixtures/full.json")).unwrap();
    let native_projection = native
        .syntax()
        .body
        .iter()
        .map(normalize_statement)
        .collect::<Vec<_>>();
    assert_eq!(native_projection, old_projection);
    assert_eq!(native.syntax().body.len(), old_projection.len());
    assert!(native_projection[1].contains("a  b"));
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
fn historical_projection_interfaces_match_across_supported_syntax() {
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
        let historical = match minor {
            7 => include_str!("fixtures/minor-7.json"),
            8 => include_str!("fixtures/minor-8.json"),
            9 => include_str!("fixtures/minor-9.json"),
            10 => include_str!("fixtures/minor-10.json"),
            11 => include_str!("fixtures/minor-11.json"),
            12 => include_str!("fixtures/minor-12.json"),
            13 => include_str!("fixtures/minor-13.json"),
            14 => include_str!("fixtures/minor-14.json"),
            _ => unreachable!(),
        };
        let expected: Vec<(String, String)> = serde_json::from_str(historical).unwrap();
        assert_eq!(native.syntax().body.len(), expected.len());
        for (statement, (header, original)) in native.syntax().body.iter().zip(&expected) {
            assert_eq!(&native_projection(statement), header, "Python 3.{minor}");
            assert_eq!(&source[statement.range()], original, "Python 3.{minor}");
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
        let native = normalize_statement(&native.syntax().body[0]);
        outputs.push(native);
    }
    assert_ne!(outputs[0], outputs[1]);
    assert_ne!(outputs[2], outputs[3]);
}

/// 父内容中的合成声明锚点保留旧引擎确认的具体打印值和顺序语义。
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
    assert_eq!(normalize_statement(&native), name);
}
