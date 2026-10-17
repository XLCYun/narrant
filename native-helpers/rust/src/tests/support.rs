use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// 从提交的多文件 Cargo fixture 建立独立副本，永不修改真实业务源码。
pub(crate) fn fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src/tests/fixtures/core"),
        directory.path(),
    );
    directory
}

/// 复制 fixture 的每个文本文件；Cargo.lock 也必须纳入不改写断言。
fn copy_tree(source: &Path, target: &Path) {
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir_all(&destination).unwrap();
            copy_tree(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).unwrap();
        }
    }
}

/// fixture 条目同时区分目录和文件，空 target 目录也不能被忽略。
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum SnapshotEntry {
    Directory,
    File(Vec<u8>),
}

/// 为完整目录保存字节级快照，检测 metadata 生成锁文件或任何执行标记。
pub(crate) fn snapshot(root: &Path) -> BTreeMap<PathBuf, SnapshotEntry> {
    let mut result = BTreeMap::new();
    snapshot_inner(root, root, &mut result);
    result
}

/// 递归读取 fixture 文件，不忽略 Cargo 输出、锁文件或 marker。
fn snapshot_inner(root: &Path, directory: &Path, result: &mut BTreeMap<PathBuf, SnapshotEntry>) {
    for entry in fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            result.insert(
                entry.path().strip_prefix(root).unwrap().to_owned(),
                SnapshotEntry::Directory,
            );
            snapshot_inner(root, &entry.path(), result);
        } else {
            result.insert(
                entry.path().strip_prefix(root).unwrap().to_owned(),
                SnapshotEntry::File(fs::read(entry.path()).unwrap()),
            );
        }
    }
}

/// 通过项目目录的 rustup 只读探测获取已有工具链，不使用 helper 构建版本。
pub(crate) fn project_toolchain(root: &Path) -> crate::ProjectToolchain {
    let output = std::process::Command::new("rustc")
        .args(["--print", "sysroot"])
        .current_dir(root)
        // 项目测试版本独立于 Cargo 为 helper 构建注入的 RUSTUP_TOOLCHAIN。
        .env(
            "RUSTUP_TOOLCHAIN",
            std::env::var_os("NARRANT_PROJECT_RUST_TOOLCHAIN").unwrap_or_else(|| "stable".into()),
        )
        .env("RUSTUP_AUTO_INSTALL", "0")
        .output()
        .unwrap();
    assert!(output.status.success());
    let sysroot = PathBuf::from(String::from_utf8(output.stdout).unwrap().trim());
    crate::ProjectToolchain {
        rust_src: sysroot.join("lib/rustlib/src/rust/library"),
        sysroot,
    }
}

/// 报告改变的条目路径，避免失败日志输出整份源码字节数组。
pub(crate) fn assert_unchanged(root: &Path, before: &BTreeMap<PathBuf, SnapshotEntry>) {
    let after = snapshot(root);
    let changed: std::collections::BTreeSet<_> = before
        .keys()
        .chain(after.keys())
        .filter(|path| before.get(*path) != after.get(*path))
        .collect();
    assert!(changed.is_empty(), "分析改写了 fixture 条目：{changed:?}");
}
