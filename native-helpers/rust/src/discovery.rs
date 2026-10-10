//! 保留旧入口的清单优先级与工具链覆盖；只探测已有工具，不安装或运行构建。

use crate::{DependencyScope, LoadPolicy, NetworkPolicy, ProjectToolchain};
use anyhow::{Context, Result, ensure};
use std::{
    path::{Path, PathBuf},
    process::Command,
};

/// 父级显式项目优先，否则只检查直接子目录，禁止把整个仓库递归并成配置。
pub(crate) fn manifests(repository: &Path) -> Result<Vec<PathBuf>> {
    for name in ["rust-project.json", ".rust-project.json", "Cargo.toml"] {
        if let Some(path) = repository
            .ancestors()
            .map(|root| root.join(name))
            .find(|path| path.is_file())
        {
            return Ok(vec![path.canonicalize()?]);
        }
    }
    let mut manifests = Vec::new();
    for entry in std::fs::read_dir(repository)? {
        let path = entry?.path().join("Cargo.toml");
        if path.is_file() {
            manifests.push(path.canonicalize()?);
        }
    }
    manifests.sort();
    manifests.dedup();
    Ok(manifests)
}

/// 逐目录探测用户覆盖/目录工具链；标准库模式要求所有项目与已批准平台一致。
pub(crate) fn policies(
    manifests: &[PathBuf],
    scope: DependencyScope,
    platform: Option<&crate::protocol::Platform>,
) -> Result<Vec<LoadPolicy>> {
    let mut policies = Vec::new();
    let mut previous = None;
    for manifest in manifests {
        let directory = manifest.parent().context("清单没有父目录")?;
        probe("cargo", &["--version"], directory)?;
        let sysroot = PathBuf::from(probe("rustc", &["--print", "sysroot"], directory)?.trim())
            .canonicalize()?;
        ensure!(
            sysroot.is_absolute() && sysroot.is_dir(),
            "项目工具链返回无效 sysroot"
        );
        let project_toolchain = if scope == DependencyScope::RepositoryOnly {
            None
        } else {
            ensure!(
                previous.as_ref().is_none_or(|root| root == &sysroot),
                "此仓库的多个 Rust 项目使用不同工具链；当前标准库分析不支持这种组合"
            );
            previous = Some(sysroot.clone());
            let platform = platform.context("标准库分析缺少批准的项目工具链平台")?;
            ensure!(
                platform.kind == "rust-standard-library"
                    && platform.root.canonicalize()? == sysroot,
                "批准的平台与实际项目 sysroot 不一致"
            );
            let rust_src = platform.root.join(&platform.metadata.rust_src);
            ensure!(
                rust_src.canonicalize()?
                    == sysroot
                        .join("lib/rustlib/src/rust/library")
                        .canonicalize()?,
                "平台 rust_src 与实际项目工具链不一致"
            );
            Some(ProjectToolchain { sysroot, rust_src })
        };
        policies.push(LoadPolicy {
            dependency_scope: scope,
            network: if std::env::var("CARGO_NET_OFFLINE")
                .is_ok_and(|value| value == "true" || value == "1")
            {
                NetworkPolicy::Offline
            } else {
                NetworkPolicy::Allow
            },
            test_cfg: true,
            project_toolchain,
        });
    }
    Ok(policies)
}

/// 探测继承 Host 的进程树所有权与运行时覆盖，强制禁止自动安装和版本缓存写回。
fn probe(tool: &str, arguments: &[&str], directory: &Path) -> Result<String> {
    let output = Command::new(tool)
        .args(arguments)
        .current_dir(directory)
        .env("PATH", runtime_path()?)
        .env("RUSTUP_AUTO_INSTALL", "0")
        .env("CARGO_CACHE_RUSTC_INFO", "0")
        .output()
        .with_context(|| format!("无法使用项目的 {tool}"))?;
    ensure!(
        output.status.success(),
        "项目的 {tool} 不可用：{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).context("工具链探针输出不是 UTF-8")
}

/// 保留工厂 PATH 顺序，仅在末尾补充旧入口已有的 Cargo 安装目录。
pub(crate) fn runtime_path() -> Result<std::ffi::OsString> {
    let mut paths: Vec<_> =
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
    let cargo_home = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                .map(|home| PathBuf::from(home).join(".cargo"))
        });
    if let Some(home) = cargo_home {
        let bin = home.join("bin");
        if !paths.contains(&bin) {
            paths.push(bin);
        }
    }
    Ok(std::env::join_paths(paths)?)
}
