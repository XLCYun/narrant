use super::*;

/// 空选集在工具未安装时仍可完成，不做多余启动。
#[test]
fn empty_selection_is_lazy() {
    let tools = TypeScriptToolchain {
        node: PathBuf::from("/missing"),
        runtime_root: PathBuf::from("/missing"),
    };
    assert!(PreparedProject::prepare(
        Path::new("/missing-repo"),
        &[],
        &tools,
        Duration::from_secs(1),
        &(std::sync::Arc::new(|| false) as CancellationCheck)
    )
    .unwrap()
    .is_none());
}

/// 助手协议拒绝缺字段、错误类型和未知字段；解析错误不会产生一个空 PreparedProject。
#[test]
fn malformed_helper_output_is_rejected() {
    for output in [
        json!({}),
        json!({"projects":[],"associations":[],"sources":[],"metadata":[],"config_files":[],"surprise":true}),
        json!({"projects":{},"associations":[],"sources":[],"metadata":[],"config_files":[]}),
        json!({"projects":[],"associations":[],"sources":[],"metadata":[],"config_files":[],"notes":[]}),
    ] {
        assert!(serde_json::from_value::<PreparedOutput>(output).is_err());
    }
}

/// 完整助手结果不再要求分析说明字段，空项目结果也能按协议解析。
#[test]
fn helper_output_without_notes_is_accepted() {
    let output =
        json!({"projects":[],"associations":[],"sources":[],"metadata":[],"config_files":[]});
    assert!(serde_json::from_value::<PreparedOutput>(output).is_ok());
}

/// 输入路径在任何工具启动前拒绝父目录跳转和根外符号链接。
#[test]
fn selected_paths_are_validated_before_tools() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let tools = TypeScriptToolchain {
        node: PathBuf::from("/missing"),
        runtime_root: PathBuf::from("/missing"),
    };
    let check: CancellationCheck = std::sync::Arc::new(|| false);
    let error = PreparedProject::prepare(
        &root,
        &[root.join("../outside.ts")],
        &tools,
        Duration::from_secs(1),
        &check,
    )
    .unwrap_err();
    assert!(error.to_string().contains("越界"));
}
