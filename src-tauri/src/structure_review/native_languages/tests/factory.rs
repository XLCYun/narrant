use super::*;

/// 未安装工具时明确失败，不能跳过已识别语言。
#[test]
fn missing_pack_is_an_error() {
    let root = tempfile::tempdir().unwrap();
    assert!(from_pack(PackId::Go, root.path()).is_err());
}

/// 控制文件不能把工具或标准库指向包外。
#[test]
fn relative_paths_cannot_escape_pack() {
    let root = tempfile::tempdir().unwrap();
    for path in ["../sdk", "/sdk", "C:/sdk", "runtime/../sdk", "runtime\\sdk"] {
        assert!(inside(root.path(), path).is_err(), "{path}");
    }
}

/// 仅仓库 Rust 的新政策不能顺带放松已迁移四语言的标准声明要求。
#[test]
fn existing_factories_still_require_target_declarations() {
    for &id in PackId::NATIVE_MIGRATED
        .iter()
        .filter(|id| **id != PackId::Rust)
    {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        for entry in id.required(if cfg!(windows) {
            "x86_64-pc-windows-msvc"
        } else {
            "aarch64-apple-darwin"
        }) {
            let file = root.join(entry);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, "").unwrap();
        }
        if id == PackId::Java {
            fs::create_dir_all(root.join("runtime/helper/lib")).unwrap();
        }
        fs::write(
            root.join("control/native.json"),
            r#"{"schema_version":1,"helper_version":"0.1.0","platforms":[]}"#,
        )
        .unwrap();
        let error = from_pack(id, root).err().expect("不能缺少目标标准库");
        assert!(
            error.to_string().contains("target_unavailable"),
            "{id:?}: {error}"
        );
    }
}

/// Rust 仅仓库工厂允许空标准声明，但入口与控制文件仍必须真实存在。
#[test]
fn rust_repository_factory_accepts_only_verified_pack_entry() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("bin")).unwrap();
    fs::create_dir(root.path().join("control")).unwrap();
    fs::write(
        root.path().join(if cfg!(windows) {
            "bin/narrant-rust.exe"
        } else {
            "bin/narrant-rust"
        }),
        "fixture",
    )
    .unwrap();
    fs::write(
        root.path().join("control/native.json"),
        r#"{"schema_version":1,"helper_version":"0.1.0","platforms":[]}"#,
    )
    .unwrap();
    assert!(RustPackAnalyzer::from_pack(root.path()).is_ok());
    fs::write(
        root.path().join("control/native.json"),
        r#"{"schema_version":1,"helper_version":"0.1.0","platforms":[],"command":"untrusted"}"#,
    )
    .unwrap();
    assert!(RustPackAnalyzer::from_pack(root.path()).is_err());
}

/// Python 内嵌标准声明只归属于固定 helper 的唯一安装根，工厂不能与开发入口产生不同根。
#[test]
fn python_factory_keeps_embedded_stdlib_in_helper_trusted_root() {
    let root = tempfile::tempdir().unwrap();
    for directory in ["bin", "control"] {
        fs::create_dir(root.path().join(directory)).unwrap();
    }
    fs::write(
        root.path().join(if cfg!(windows) {
            "bin/narrant-python.exe"
        } else {
            "bin/narrant-python"
        }),
        "fixture",
    )
    .unwrap();
    fs::write(root.path().join("control/native.json"),r#"{"schema_version":1,"helper_version":"0.1.0","platforms":[{"id":"python-stdlib","kind":"python-embedded-stdlib","version":"0.0.84","root":"bin","metadata":{"source_revision":"11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a"}}]}"#).unwrap();
    let analyzer = from_pack(PackId::Python, root.path()).unwrap();
    let repository = tempfile::tempdir().unwrap();
    fs::write(repository.path().join("main.py"), "def run(): pass\n").unwrap();
    analyzer
        .validate_input(&crate::structure_review::code_graph::AnalysisInput {
            repository_root: repository.path().into(),
            files: vec!["main.py".into()],
            options: Default::default(),
        })
        .unwrap();
}
