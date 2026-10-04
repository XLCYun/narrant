use super::*;

/// 只构造SDK发现所需的标准源码标记，不把合成内容用于真实Go语义测试。
fn installation(container: &Path, name: &str, version: &str) -> PathBuf {
    let root = container.join(name);
    for file in ["src/builtin/builtin.go", "src/runtime/runtime.go"] {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "discovery fixture").unwrap();
    }
    fs::write(root.join("VERSION"), format!("{version}\ntime fixture\n")).unwrap();
    root.canonicalize().unwrap()
}

/// 每个语言minor独立发现最高稳定patch，不能让新标准库覆盖旧目标。
#[test]
fn sdk_catalog_keeps_minor_versions_and_selects_stable_patches() {
    let root = tempfile::tempdir().unwrap();
    let older = installation(root.path(), "old-patch", "go1.23.8");
    let old = installation(root.path(), "old", "go1.23.12");
    let newer = installation(root.path(), "new", "go1.27.1");
    let preview = installation(root.path(), "preview", "go1.27rc2");
    let catalog = in_roots(&[older, newer.clone(), old.clone(), preview]).unwrap();
    assert_eq!(catalog.len(), 2);
    assert_eq!(catalog[0].version, "1.23.12");
    assert_eq!(catalog[0].root, old);
    assert_eq!(catalog[1].version, "1.27.1");
    assert_eq!(catalog[1].root, newer);
}

/// 只有 go 可执行文件或 VERSION 不足以提供目标标准库；不得在此自动安装源码。
#[test]
fn sdk_without_standard_sources_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let sdk = installation(root.path(), "incomplete", "go1.27.1");
    fs::remove_file(sdk.join("src/builtin/builtin.go")).unwrap();
    assert!(in_roots(&[sdk]).is_err());
}
