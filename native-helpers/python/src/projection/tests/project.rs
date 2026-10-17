use super::project::Project;
use crate::{ProjectInput, PythonProject};
use std::{fs, path::PathBuf};

/// 迁移旧初始化协议断言，实际原生 AST target 必须等于精确项目/脚本选择。
#[test]
fn target_selection_drives_native_database_version() {
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
        let repository = tempfile::tempdir().unwrap();
        for (name, source) in [("pyproject.toml", pyproject), ("ty.toml", ty)] {
            if !source.is_empty() {
                fs::write(repository.path().join(name), source).unwrap();
            }
        }
        let source = format!("{prefix}def service(): pass\n");
        let file = repository.path().join("main.py");
        fs::write(&file, &source).unwrap();
        let root = repository.path().canonicalize().unwrap();
        let policy = Project::for_file(&root, &root.join("main.py"), &source).unwrap();
        assert_eq!(format!("3.{}", policy.minor), expected);
        let native = PythonProject::load(ProjectInput {
            repository: root.clone(),
            project_root: policy.root.strip_prefix(&root).unwrap().into(),
            source_roots: policy
                .source_roots
                .iter()
                .map(|path| PathBuf::from(path.strip_prefix(&root).unwrap()))
                .collect(),
            python_minor: policy.minor,
        })
        .unwrap();
        assert_eq!(
            native
                .parse(std::path::Path::new("main.py"))
                .unwrap()
                .module()
                .python_version(),
            ruff_python_ast::PythonVersion::from((3, policy.minor))
        );
    }
    for (explicit, requires) in [
        ("3.8", ">=3.10"),
        ("3.6", ">=3.7"),
        ("3.15", ""),
        ("3.10", ">=3.10,!=3.10.*"),
    ] {
        let repository = tempfile::tempdir().unwrap();
        let root = repository.path().canonicalize().unwrap();
        fs::write(
            root.join("ty.toml"),
            format!("[environment]\npython-version='{explicit}'\n"),
        )
        .unwrap();
        fs::write(
            root.join("pyproject.toml"),
            format!("[project]\nrequires-python='{requires}'\n"),
        )
        .unwrap();
        assert!(Project::for_file(&root, &root.join("main.py"), "def service(): pass\n").is_err());
    }
}
