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
