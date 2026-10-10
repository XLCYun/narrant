use super::*;

/// 只模拟发现阶段的目录与 release，真实语法/API 隔离由 helper 集成用例覆盖。
fn installation(container: &Path, name: &str, version: &str, legacy: bool) -> PathBuf {
    let root = container.join(name);
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join("release"),
        format!("JAVA_VERSION=\"{version}\"\n"),
    )
    .unwrap();
    let files: &[&str] = if legacy {
        &["jre/lib/rt.jar"]
    } else {
        &["lib/jrt-fs.jar", "lib/modules"]
    };
    for file in files {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "discovery fixture").unwrap();
    }
    root.canonicalize().unwrap()
}

/// 经典与模块化 JDK 按实际声明识别，同 major 使用最高稳定版本，EA 不覆盖稳定目标。
#[test]
fn jdk_catalog_uses_installed_versions_and_keeps_majors_separate() {
    let root = tempfile::tempdir().unwrap();
    let java8 = installation(root.path(), "jdk8", "1.8.0_504", true);
    let older = installation(root.path(), "jdk17-old", "17.0.9", false);
    let java17 = installation(root.path(), "jdk17-new", "17.0.20.1", false);
    let java42 = installation(root.path(), "custom.jdk/Contents/Home", "42.0.2", false);
    let ea = installation(root.path(), "early-access", "17.0.99-ea", false);
    let catalog = in_roots(&[
        java8.clone(),
        older,
        java17.clone(),
        root.path().join("custom.jdk"),
        ea,
    ])
    .unwrap();
    assert_eq!(catalog.len(), 3);
    assert_eq!(catalog[0].root, java8);
    assert_eq!(catalog[0].metadata["release"], "8");
    assert_eq!(catalog[1].root, java17);
    assert_eq!(catalog[1].version, "17.0.20.1");
    // Host 不假装具备未来语法能力；它发现候选，JDT 再判断是否能分析目标。
    assert_eq!(catalog[2].root, java42);
    assert_eq!(catalog[2].metadata["release"], "42");
}

/// 有版本文件而没有标准声明的目录不能冒充可分析的 JDK。
#[test]
fn jdk_missing_system_libraries_is_not_a_target() {
    let root = tempfile::tempdir().unwrap();
    let jdk = installation(root.path(), "jdk", "25.0.4.1", false);
    fs::remove_file(jdk.join("lib/modules")).unwrap();
    assert!(in_roots(&[jdk]).is_err());
}
