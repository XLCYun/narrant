use super::*;
use std::os::unix::fs::PermissionsExt;

/// 隔离的工具链只响应只读查询，记录环境以证明没有执行组件安装。
fn tools(root: &Path) -> OsString {
    let bin = root.join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::create_dir(root.join("sysroot")).unwrap();
    for tool in ["cargo", "rustc"] {
        let executable = bin.join(tool);
        std::fs::write(&executable, format!("#!/bin/sh\n[ \"$RUSTUP_AUTO_INSTALL\" = 0 ] || exit 8\nprintf '%s\\n' \"{tool} $*\" >> probes\ncase \"$*\" in\n  --version) echo cargo-fixture;;\n  '--print sysroot') echo \"$PWD/sysroot\";;\n  *) exit 9;;\nesac\n")).unwrap();
        std::fs::set_permissions(executable, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin.into_os_string()
}

/// 缺源码时只报告诊断，已有源码与仅仓库模式均不执行安装。
#[test]
fn missing_sources_fail_without_installing_and_repository_scope_needs_no_sources() {
    let root = tempfile::tempdir().unwrap();
    let path = tools(root.path());
    let cancelled: CancellationCheck = std::sync::Arc::new(|| false);
    let missing = RustToolchain::resolve_with_path(
        root.path(),
        RustDependencyScope::StandardLibrary,
        &cancelled,
        path.clone(),
    )
    .err()
    .unwrap();
    assert!(missing.to_string().contains("rust-src"));
    let limited = RustToolchain::resolve_with_path(
        root.path(),
        RustDependencyScope::RepositoryOnly,
        &cancelled,
        path.clone(),
    )
    .unwrap();
    assert!(limited.sysroot.is_none());
    assert!(limited.sources.is_none());
    for name in ["core", "std"] {
        let source = root
            .path()
            .join(format!("sysroot/lib/rustlib/src/rust/library/{name}/src"));
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("lib.rs"), "").unwrap();
    }
    let installed = RustToolchain::resolve_with_path(
        root.path(),
        RustDependencyScope::StandardLibrary,
        &cancelled,
        path,
    )
    .unwrap();
    assert_eq!(
        installed.sysroot.unwrap(),
        root.path().join("sysroot").canonicalize().unwrap()
    );
    assert!(installed
        .sources
        .unwrap()
        .ends_with("lib/rustlib/src/rust/library"));
    let calls = std::fs::read_to_string(root.path().join("probes")).unwrap();
    assert_eq!(calls, "cargo --version\nrustc --print sysroot\n".repeat(3));
}

/// 子项目工具链在该项目 cwd 解析；多个工具链不会被仓库根默认值覆盖。
#[test]
fn child_projects_are_checked_individually_and_mixed_sysroots_are_explicit() {
    let root = tempfile::tempdir().unwrap();
    let path = tools(root.path());
    let cancelled: CancellationCheck = std::sync::Arc::new(|| false);
    let project = root.path().join("src-tauri");
    std::fs::create_dir(&project).unwrap();
    std::fs::write(project.join("Cargo.toml"), "[package]").unwrap();
    for name in ["core", "std"] {
        let source = project.join(format!("sysroot/lib/rustlib/src/rust/library/{name}/src"));
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("lib.rs"), "").unwrap();
    }
    let resolved = RustToolchain::resolve_projects(
        root.path(),
        RustDependencyScope::StandardLibrary,
        &cancelled,
        path.clone(),
    )
    .unwrap();
    assert_eq!(
        resolved.sysroot.unwrap(),
        project.join("sysroot").canonicalize().unwrap()
    );
    assert_eq!(resolved.manifests, vec![project.join("Cargo.toml")]);
    assert!(!root.path().join("probes").exists());
    let second = root.path().join("other-project");
    std::fs::create_dir(&second).unwrap();
    std::fs::write(second.join("Cargo.toml"), "[package]").unwrap();
    for name in ["core", "std"] {
        let source = second.join(format!("sysroot/lib/rustlib/src/rust/library/{name}/src"));
        std::fs::create_dir_all(&source).unwrap();
        std::fs::write(source.join("lib.rs"), "").unwrap();
    }
    let failure = RustToolchain::resolve_projects(
        root.path(),
        RustDependencyScope::StandardLibrary,
        &cancelled,
        path.clone(),
    )
    .err()
    .unwrap();
    assert!(failure.to_string().contains("不同工具链"));
    let limited = RustToolchain::resolve_projects(
        root.path(),
        RustDependencyScope::RepositoryOnly,
        &cancelled,
        path,
    )
    .unwrap();
    assert_eq!(limited.manifests.len(), 2);
    assert!(limited.sysroot.is_none());
}
