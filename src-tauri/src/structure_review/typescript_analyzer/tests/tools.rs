use super::*;

/// 最低版本边界不能被字符串排序或预发布后缀绕过。
#[test]
fn node_version_boundary() {
    for version in ["v22.22.2", "24.18.0", "23.0.0"] {
        assert!(supported_node(version));
    }
    for version in [
        "v22.22.1",
        "v22.13.0",
        "21.99.99",
        "24.0.0-beta",
        "24.0",
        "garbage",
    ] {
        assert!(!supported_node(version));
    }
}

/// 工具不存在时报告明确的安装入口；不启动任何下载。
#[test]
fn missing_tools_are_actionable() {
    let toolchain = TypeScriptToolchain {
        node: PathBuf::from("/not-installed/narrant-node"),
        runtime_root: PathBuf::from("/not-installed/runtime"),
    };
    let error = toolchain
        .validate(
            Duration::from_secs(1),
            &(std::sync::Arc::new(|| false) as CancellationCheck),
        )
        .unwrap_err();
    assert!(error.to_string().contains("setup:typescript-analyzer"));
}

/// 仅构造包元数据便可验证错误版本或缺失闭包不会误用系统工具。
#[test]
fn fixed_versions_and_runtime_closure_are_required() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let node = root.join("node");
    std::fs::write(&node, "unused").unwrap();
    let runtime = root.join("runtime");
    let package = runtime.join("typescript");
    std::fs::create_dir_all(package.join("lib")).unwrap();
    std::fs::write(package.join("lib/tsserver.js"), "").unwrap();
    std::fs::write(
        package.join("package.json"),
        r#"{"name":"typescript","version":"5.9.3"}"#,
    )
    .unwrap();
    let tools = TypeScriptToolchain {
        node,
        runtime_root: runtime,
    };
    let check: CancellationCheck = std::sync::Arc::new(|| false);
    assert!(tools
        .validate(Duration::from_secs(1), &check)
        .unwrap_err()
        .to_string()
        .contains("6.0.3"));
    std::fs::write(
        package.join("package.json"),
        r#"{"name":"typescript","version":"6.0.3"}"#,
    )
    .unwrap();
    let error = tools.validate(Duration::from_secs(1), &check).unwrap_err();
    assert!(
        error.to_string().contains("typescript-language-server")
            && error.to_string().contains("setup:typescript-analyzer")
    );
}
