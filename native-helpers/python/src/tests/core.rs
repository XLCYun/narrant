//! 聚焦原生数据库、只读边界与导航候选；完整图与绑定校验另行验收。
use super::*;
use std::collections::BTreeSet;
use std::fs;

/// 在临时磁盘目录写入 fixture，再以明确项目环境加载。
fn fixture(files: &[(&str, &str)], minor: u8) -> (tempfile::TempDir, PythonProject) {
    let directory = tempfile::tempdir().unwrap();
    for (path, source) in files {
        let path = directory.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    let project = PythonProject::load(input(directory.path(), minor)).unwrap();
    (directory, project)
}

/// 每个测试独立传入版本与根，避免从宿主 Python 猜测。
fn input(repository: &Path, minor: u8) -> ProjectInput {
    ProjectInput {
        repository: repository.into(),
        project_root: PathBuf::new(),
        source_roots: vec![PathBuf::new()],
        python_minor: minor,
    }
}

/// 返回最后一次出现的 token 上的原生候选。
fn query(project: &PythonProject, path: &str, token: &str) -> DefinitionQuery {
    let source = project.source(Path::new(path)).unwrap();
    project
        .definitions(Path::new(path), source.rfind(token).unwrap())
        .unwrap()
}

/// 逐个验证 3.7–3.14，不把旧目标提升为新版本；保留标准库全部 overload 候选。
#[test]
fn all_supported_minors_resolve_local_and_standard_definitions() {
    for minor in 7..=14 {
        let (_, project) = fixture(
            &[
                (
                    "main.py",
                    "from other import helper\nhelper()\nlen([])\n''.upper()\n",
                ),
                ("other.py", "def helper():\n    pass\n"),
            ],
            minor,
        );
        let parsed = project.parse(Path::new("main.py")).unwrap();
        assert_eq!(
            parsed.module().python_version(),
            ruff_python_ast::PythonVersion::from((3, minor))
        );
        let local = query(&project, "main.py", "helper");
        assert_eq!(local.candidates.len(), 1, "3.{minor}: {local:?}");
        assert_eq!(
            local.candidates[0].origin,
            SourceOrigin::Repository("other.py".into())
        );
        for token in ["len", "upper"] {
            let standard = query(&project, "main.py", token);
            assert_eq!(
                standard.candidates.len(),
                if token == "upper" { 2 } else { 1 },
                "3.{minor} {token}: {standard:?}"
            );
            let focuses: BTreeSet<_> = standard
                .candidates
                .iter()
                .map(|candidate| (candidate.focus.start(), candidate.focus.end()))
                .collect();
            assert_eq!(
                focuses.len(),
                standard.candidates.len(),
                "overload 焦点必须不同"
            );
            println!(
                "Python 3.{minor} {token}: {:?}",
                standard
                    .candidates
                    .iter()
                    .map(|candidate| (
                        &candidate.origin,
                        candidate.focus,
                        candidate.full,
                        &candidate.standard_symbol
                    ))
                    .collect::<Vec<_>>()
            );
            for candidate in &standard.candidates {
                assert_eq!(
                    candidate.origin,
                    SourceOrigin::StandardDeclaration("stdlib/builtins.pyi".into())
                );
                assert_eq!(&candidate.source[candidate.focus], token);
                assert_eq!(
                    candidate.standard_symbol,
                    Some(StandardSymbolIdentity {
                        module: "builtins".into(),
                        qualified_name: if token == "len" { "len" } else { "str.upper" }.into(),
                    })
                );
            }
        }
    }
}

/// 别名、重绑定、property 和 stub overload 都只产生导航候选，不替代绑定裁决。
#[test]
fn aliases_rebindings_stubs_property_and_overload_remain_candidates() {
    let (_, project) = fixture(
        &[
            ("clean.py", "from other import helper as alias\nalias()\n"),
            (
                "main.py",
                "from other import helper as alias\nfrom stub import typed\nalias()\ntyped(1)\nfrom other import Box\nb = Box()\nb.value\nalias = missing\nalias()\n",
            ),
            (
                "other.py",
                "def helper(): pass\nclass Box:\n    @property\n    def value(self) -> int: return 1\n",
            ),
            (
                "stub.pyi",
                "from typing import overload\n@overload\ndef typed(x: int) -> int: ...\n@overload\ndef typed(x: str) -> str: ...\n",
            ),
        ],
        14,
    );
    let source = project.source(Path::new("main.py")).unwrap();
    let alias = project
        .definitions(Path::new("main.py"), source.find("alias()").unwrap())
        .unwrap();
    // ty 导航是静态声明候选：后续模块赋值也可能影响之前的查询。
    assert_eq!(
        alias.candidates[0].origin,
        SourceOrigin::Repository("main.py".into())
    );
    let clean_alias = query(&project, "clean.py", "alias");
    assert_eq!(
        clean_alias.candidates[0].origin,
        SourceOrigin::Repository("other.py".into())
    );
    let rebound = query(&project, "main.py", "alias");
    assert_eq!(alias.candidates.len(), 2);
    assert_eq!(rebound.candidates.len(), 2);
    assert!(
        rebound
            .candidates
            .iter()
            .any(|candidate| candidate.origin == SourceOrigin::Repository("other.py".into()))
    );
    assert!(
        rebound
            .candidates
            .iter()
            .any(|candidate| candidate.origin == SourceOrigin::Repository("main.py".into()))
    );
    for (label, result) in [("before rebinding", &alias), ("after rebinding", &rebound)] {
        println!(
            "{label}: {:?}",
            result
                .candidates
                .iter()
                .map(|candidate| (&candidate.origin, candidate.focus, candidate.full))
                .collect::<Vec<_>>()
        );
    }
    let overload = query(&project, "main.py", "typed");
    assert_eq!(overload.candidates.len(), 2, "{overload:?}");
    assert_ne!(overload.candidates[0].focus, overload.candidates[1].focus);
    assert!(
        overload
            .candidates
            .iter()
            .all(|candidate| &candidate.source[candidate.focus] == "typed")
    );
    println!(
        "stub overload: {:?}",
        overload
            .candidates
            .iter()
            .map(|candidate| (&candidate.origin, candidate.focus, candidate.full))
            .collect::<Vec<_>>()
    );
    assert!(
        overload
            .candidates
            .iter()
            .all(|candidate| candidate.origin == SourceOrigin::Repository("stub.pyi".into()))
    );
    let property = query(&project, "main.py", "value");
    assert!(!property.candidates.is_empty());
    assert_eq!(
        property.candidates[0].origin,
        SourceOrigin::Repository("other.py".into())
    );
}

/// 缺依赖保持未知，多平台标准声明保持多候选。
#[test]
fn missing_dependencies_and_multiple_platforms_are_not_collapsed() {
    let (_, project) = fixture(
        &[(
            "main.py",
            "import absent\nabsent.fetch()\nfrom pathlib import Path\nPath('.').exists()\n",
        )],
        14,
    );
    assert!(query(&project, "main.py", "fetch").candidates.is_empty());
    let paths = query(&project, "main.py", "exists");
    assert!(paths.candidates.len() > 1, "{paths:?}");
}

/// BOM、中文、emoji 和 LF/CRLF/单独 CR 的原文与字节位置都来自同一快照。
#[test]
fn unicode_source_and_invalid_positions_use_same_snapshot() {
    for ending in ["\n", "\r\n", "\r"] {
        let original =
            "\u{feff}# 😀\ndef 中文():\n    return 'a\\nb'\n中文()\n".replace('\n', ending);
        let (directory, project) = fixture(&[("main.py", &original)], 7);
        assert_eq!(project.source(Path::new("main.py")).unwrap(), original);
        let definitions = query(&project, "main.py", "中文");
        assert_eq!(
            &definitions.candidates[0].source[definitions.candidates[0].focus],
            "中文"
        );
        fs::write(directory.path().join("main.py"), "bad syntax$").unwrap();
        assert_eq!(project.source(Path::new("main.py")).unwrap(), original);
        assert_eq!(query(&project, "main.py", "中文"), definitions);
        for offset in [original.len(), original.find('😀').unwrap() + 1, usize::MAX] {
            assert!(project.definitions(Path::new("main.py"), offset).is_err());
        }
        assert!(project.source(Path::new("../main.py")).is_err());
        assert!(project.parse(Path::new("unknown.py")).is_err());
    }
}

/// 语法、旧目标不支持的语法、编码及版本输入错误均明确失败。
#[test]
fn syntax_version_and_loading_errors_are_explicit() {
    let (_, project) = fixture(&[("main.py", "match x:\n    case 1: pass\n")], 7);
    assert!(project.parse(Path::new("main.py")).is_err());
    assert!(query_error(&project));
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("bad.py"), [0xff, 0xfe]).unwrap();
    assert!(PythonProject::load(input(directory.path(), 14)).is_err());
    fs::write(
        directory.path().join("bad.py"),
        "# coding: latin-1\nx = 1\n",
    )
    .unwrap();
    assert!(PythonProject::load(input(directory.path(), 14)).is_err());
    assert!(PythonProject::load(input(directory.path(), 15)).is_err());
}

/// 语法错误不能变成空导航成功。
fn query_error(project: &PythonProject) -> bool {
    project.definitions(Path::new("main.py"), 6).is_err()
}

#[cfg(unix)]
/// 内部链接保留各自模块路径，外部链接在原生数据库加载前拒绝。
#[test]
fn internal_links_preserve_module_paths_and_escape_links_fail() {
    use std::os::unix::fs::symlink;
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("impl")).unwrap();
    fs::write(
        directory.path().join("impl/__init__.py"),
        "def target(): pass\n",
    )
    .unwrap();
    symlink("impl", directory.path().join("alias")).unwrap();
    symlink("impl/__init__.py", directory.path().join("api.pyi")).unwrap();
    fs::write(
        directory.path().join("main.py"),
        "import impl\nimpl.target()\n",
    )
    .unwrap();
    let project = PythonProject::load(input(directory.path(), 14)).unwrap();
    for path in ["impl/__init__.py", "alias/__init__.py", "api.pyi"] {
        assert!(project.parse(Path::new(path)).is_ok());
    }
    let definition = query(&project, "main.py", "target");
    assert_eq!(
        definition.candidates[0].origin,
        SourceOrigin::Repository("impl/__init__.py".into())
    );
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("poison.py"), "def fetch(): pass\n").unwrap();
    symlink(
        outside.path().join("poison.py"),
        directory.path().join("escaped.py"),
    )
    .unwrap();
    assert!(
        PythonProject::load(input(directory.path(), 14))
            .err()
            .unwrap()
            .to_string()
            .contains("逃逸")
    );
}

/// 真实污染子进程与仓库配置仍不能扩充上下文或赋予命令执行能力。
#[test]
fn polluted_process_and_repository_configuration_cannot_expand_context() {
    const CHILD_ROOT: &str = "NARRANT_PYTHON_POISON_FIXTURE";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        use ruff_db::Db as _;
        use ruff_db::system::{Command, SystemPath};
        let project = PythonProject::load(input(Path::new(&root), 14)).unwrap();
        assert!(query(&project, "main.py", "fetch").candidates.is_empty());
        assert_eq!(query(&project, "main.py", "len").candidates.len(), 1);
        let system = project.db.system();
        assert!(system.as_writable().is_none());
        assert!(system.command_executor().is_none());
        assert!(system.which("python").is_err());
        assert!(system.run_command(Command::new("python")).is_err());
        for name in [
            "HOME",
            "PYTHONPATH",
            "VIRTUAL_ENV",
            "CONDA_PREFIX",
            "TY_INSTALL_DIR",
            "PATH",
        ] {
            assert!(system.env_var(name).is_err());
        }
        assert!(
            system
                .read_to_string(SystemPath::new("/etc/passwd"))
                .is_err()
        );
        return;
    }
    let directory = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("poison.py"), "def fetch(): pass\n").unwrap();
    fs::write(
        outside.path().join("ty.toml"),
        "[environment]\npython-version='3.15'\n",
    )
    .unwrap();
    fs::create_dir_all(directory.path().join(".venv/lib/python3.14/site-packages")).unwrap();
    fs::write(
        directory
            .path()
            .join(".venv/lib/python3.14/site-packages/poison.py"),
        "def fetch(): pass\n",
    )
    .unwrap();
    fs::write(
        directory.path().join("ty.toml"),
        format!(
            "[environment]\nextra-paths=[{:?}]\npython={:?}\ntypeshed={:?}\n",
            outside.path(),
            outside.path(),
            outside.path()
        ),
    )
    .unwrap();
    fs::write(directory.path().join("main.py"), "# /// script\n# requires-python = '>=3.15'\n# dependencies = ['poison']\n# ///\nimport poison\npoison.fetch()\nlen([])\n").unwrap();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "tests::polluted_process_and_repository_configuration_cannot_expand_context",
        "--nocapture",
    ]);
    command.env(CHILD_ROOT, directory.path());
    for name in [
        "HOME",
        "USERPROFILE",
        "XDG_CONFIG_HOME",
        "XDG_CACHE_HOME",
        "APPDATA",
        "LOCALAPPDATA",
        "PYTHONPATH",
        "PYTHONHOME",
        "VIRTUAL_ENV",
        "CONDA_PREFIX",
        "TY_INSTALL_DIR",
        "PATH",
    ] {
        command.env(name, outside.path());
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// 可达 site-packages 或安装 prefix 不会被误认成仓库源码。
#[test]
fn reachable_installation_environments_are_rejected() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("site-packages")).unwrap();
    fs::write(
        directory.path().join("site-packages/poison.py"),
        "def fetch(): pass\n",
    )
    .unwrap();
    assert!(PythonProject::load(input(directory.path(), 14)).is_err());
    fs::remove_dir_all(directory.path().join("site-packages")).unwrap();
    fs::write(directory.path().join("pyvenv.cfg"), "home = /usr/bin\n").unwrap();
    assert!(PythonProject::load(input(directory.path(), 14)).is_err());
}

/// 实现与 stub 并存时保留两个文件身份，使用原生实际导航目标。
#[test]
fn coexistence_of_py_and_pyi_uses_native_source_navigation() {
    let (_, project) = fixture(
        &[
            ("main.py", "from dual import run\nrun()\n"),
            ("dual.py", "def run(): pass\n"),
            ("dual.pyi", "def run() -> None: ...\n"),
        ],
        14,
    );
    for path in ["dual.py", "dual.pyi"] {
        assert!(project.parse(Path::new(path)).is_ok());
    }
    let candidates = query(&project, "main.py", "run");
    assert_eq!(
        candidates.candidates[0].origin,
        SourceOrigin::Repository("dual.py".into())
    );
}

/// 明确源码根的先后顺序决定上下文；仓库其他路径不会被自动加到搜索根。
#[test]
fn explicit_source_roots_are_ordered_and_loading_is_read_only() {
    let directory = tempfile::tempdir().unwrap();
    for (path, source) in [
        ("app/main.py", "from worker import run\nrun()\n"),
        ("app/worker.py", "def run(): return 'app'\n"),
        ("shared/worker.py", "def run(): return 'shared'\n"),
        ("unrelated/poison.py", "def fetch(): pass\n"),
    ] {
        let path = directory.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    let before = tree(directory.path());
    for (roots, expected) in [
        (["shared", "app"], "shared/worker.py"),
        (["app", "shared"], "app/worker.py"),
    ] {
        let project = PythonProject::load(ProjectInput {
            repository: directory.path().into(),
            project_root: "app".into(),
            source_roots: roots.into_iter().map(PathBuf::from).collect(),
            python_minor: 14,
        })
        .unwrap();
        let candidates = query(&project, "app/main.py", "run");
        assert_eq!(
            candidates.candidates[0].origin,
            SourceOrigin::Repository(expected.into())
        );
        assert!(project.source(Path::new("unrelated/poison.py")).is_err());
    }
    assert_eq!(tree(directory.path()), before);
    let mut bad = input(directory.path(), 14);
    bad.source_roots = vec!["../escape".into()];
    assert!(PythonProject::load(bad).is_err());
}

/// 对照 fixture 的全部文件路径和字节，验证分析没有生成配置、锁或缓存。
fn tree(root: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| {
            (
                entry.path().strip_prefix(root).unwrap().to_owned(),
                fs::read(entry.path()).unwrap(),
            )
        })
        .collect()
}

#[cfg(unix)]
/// 链接不能把被排除的安装环境或带 pyvenv.cfg 的隐藏 prefix 伪装为普通源码。
#[test]
fn internal_links_to_installation_environments_are_rejected() {
    use std::os::unix::fs::symlink;
    for (target, prefix, directory_link) in [
        (
            ".venv/lib/python3.14/site-packages/poison.py",
            ".venv",
            false,
        ),
        ("runtime/lib/poison.py", "runtime", false),
        ("runtime/lib/poison.py", "runtime", true),
    ] {
        let directory = tempfile::tempdir().unwrap();
        fs::create_dir(directory.path().join("src")).unwrap();
        fs::create_dir_all(directory.path().join(target).parent().unwrap()).unwrap();
        fs::write(
            directory.path().join(prefix).join("pyvenv.cfg"),
            "home = /usr/bin\n",
        )
        .unwrap();
        fs::write(directory.path().join(target), "def fetch(): pass\n").unwrap();
        fs::write(
            directory.path().join("src/main.py"),
            "import poison\npoison.fetch()\nlen([])\n",
        )
        .unwrap();
        let (link_target, link) = if directory_link {
            (
                "../runtime/lib".into(),
                directory.path().join("src/environment"),
            )
        } else {
            (
                format!("../{target}"),
                directory.path().join("src/poison.py"),
            )
        };
        symlink(link_target, link).unwrap();
        let before = tree(directory.path());
        let result = PythonProject::load(ProjectInput {
            repository: directory.path().into(),
            project_root: "src".into(),
            source_roots: vec![
                if directory_link {
                    "src/environment"
                } else {
                    "src"
                }
                .into(),
            ],
            python_minor: 14,
        });
        let error = result.err().expect("安装环境链接必须在原生加载前拒绝");
        assert!(error.to_string().contains("安装环境"), "{error:#}");
        assert_eq!(tree(directory.path()), before);
    }
}
