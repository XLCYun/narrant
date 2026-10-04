use super::*;

/// 构造本机 SDK 安装布局，只提供发现阶段所需的标准引用标记。
fn reference(root: &Path, version: &str, tfm: &str) -> PathBuf {
    let directory = root
        .join("packs/Microsoft.NETCore.App.Ref")
        .join(version)
        .join("ref")
        .join(tfm);
    fs::create_dir_all(&directory).unwrap();
    for file in ["System.Runtime.dll", "mscorlib.dll"] {
        fs::write(directory.join(file), "reference fixture").unwrap();
    }
    directory.canonicalize().unwrap()
}

/// 未列举过的目标和补丁能够被发现；跨安装根按数值选最高稳定补丁，保留目标隔离。
#[test]
fn installed_dotnet_targets_are_discovered_without_a_version_list() {
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let net8 = reference(first.path(), "8.0.31", "net8.0");
    reference(first.path(), "9.0.9", "net9.0");
    let net9 = reference(second.path(), "9.0.20", "net9.0");
    reference(second.path(), "9.0.99-preview.1", "net9.0");
    // 目录版本和实际 TFM 不匹配的引用不能冒充目标标准库。
    reference(second.path(), "12.0.0", "net10.0");
    let catalog = dotnet_in(&[first.path().into(), second.path().into()]).unwrap();
    assert_eq!(catalog.len(), 2);
    assert_eq!(
        (&catalog[0].version, &catalog[0].root),
        (&"8.0.31".to_owned(), &net8)
    );
    assert_eq!(
        (&catalog[1].version, &catalog[1].root),
        (&"9.0.20".to_owned(), &net9)
    );
    assert_eq!(catalog[1].metadata["tfm"], "net9.0");
    assert_eq!(catalog[1].metadata["reference_pack_version"], "9.0.20");
}

/// 安装运行时不等于安装参考程序集，不以 runtime DLL 冒充目标声明。
#[test]
fn runtime_only_installation_cannot_supply_target_references() {
    let root = tempfile::tempdir().unwrap();
    let runtime = root.path().join("shared/Microsoft.NETCore.App/10.0.0");
    fs::create_dir_all(&runtime).unwrap();
    fs::write(runtime.join("System.Runtime.dll"), "runtime fixture").unwrap();
    assert!(dotnet_in(&[root.path().into()]).is_err());
}

/// SDK 版本来自安装本身，部署目标使用 SDK 的默认声明而非强制等于 SDK 版本。
#[test]
fn macos_sdk_uses_installed_metadata_without_a_whitelist() {
    let root = tempfile::tempdir().unwrap();
    for file in ["usr/include/stdio.h", "usr/include/c++/v1/vector"] {
        let path = root.path().join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "header fixture").unwrap();
    }
    fs::write(
        root.path().join("SDKSettings.json"),
        r#"{"Version":"26.5","DefaultProperties":{"MACOSX_DEPLOYMENT_TARGET":"15.0"}}"#,
    )
    .unwrap();
    let platform = macos_at(root.path()).unwrap();
    assert_eq!(platform.version, "26.5");
    assert_eq!(platform.metadata["target_triple"], "arm64-apple-macosx15.0");
    fs::remove_file(root.path().join("usr/include/stdio.h")).unwrap();
    assert!(macos_at(root.path()).is_err());
}

/// Windows 系统头与 C++ 标准头可以使用未预先列举的本机版本，来源根仍相互独立。
#[test]
fn windows_sdk_and_msvc_versions_come_from_installed_directories() {
    let root = tempfile::tempdir().unwrap();
    let sdk = root.path().join("Windows Kits/10/Include/10.0.99999.0");
    let msvc = root.path().join("VC/Tools/MSVC/14.99.12345/include");
    for directory in ["ucrt", "shared", "um", "winrt", "cppwinrt"] {
        fs::create_dir_all(sdk.join(directory)).unwrap();
    }
    fs::create_dir_all(&msvc).unwrap();
    fs::write(sdk.join("um/Windows.h"), "header fixture").unwrap();
    for file in ["vector", "yvals_core.h"] {
        fs::write(msvc.join(file), "header fixture").unwrap();
    }
    let platforms = windows_at(&sdk, &msvc).unwrap();
    assert_eq!(platforms[0].version, "14.99.12345");
    assert_eq!(platforms[1].version, "10.0.99999.0");
    assert_ne!(platforms[0].root, platforms[1].root);
    assert_eq!(
        platforms[1].metadata["additional_system_platform_ids"],
        serde_json::json!(["msvc-standard-headers"])
    );
}
