//! 只检查用户已有的项目工具链；显式源码路径阻止 rust-analyzer 自动安装 rust-src。
use super::{failure, Result, RustDependencyScope};
use crate::lsp::CancellationCheck;
use std::{
    ffi::OsString,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

/// 每一侧按自己的项目目录解析工具链，前后提交可能指定不同 Rust 版本。
pub(super) struct RustToolchain {
    pub sysroot: Option<PathBuf>,
    pub sources: Option<PathBuf>,
    pub path: OsString,
    pub manifests: Vec<PathBuf>,
}

impl RustToolchain {
    /// 不运行安装命令；缺工具或源码时向用户报告需要自行准备的项目组件。
    pub fn resolve(
        repository: &Path,
        scope: RustDependencyScope,
        cancelled: &CancellationCheck,
    ) -> Result<Self> {
        let mut paths: Vec<_> =
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()).collect();
        if let Some(cargo_home) = std::env::var_os("CARGO_HOME")
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
                    .map(|home| PathBuf::from(home).join(".cargo"))
            })
        {
            let bin = cargo_home.join("bin");
            if !paths.contains(&bin) {
                paths.push(bin);
            }
        }
        let path = std::env::join_paths(paths).map_err(failure)?;
        Self::resolve_projects(repository, scope, cancelled, path)
    }

    /// 固定发现的项目集合，逐个使用 manifest 目录解析 rustup 的项目覆盖。
    fn resolve_projects(
        repository: &Path,
        scope: RustDependencyScope,
        cancelled: &CancellationCheck,
        path: OsString,
    ) -> Result<Self> {
        let manifests = discover_manifests(repository)?;
        let directories = if manifests.is_empty() {
            vec![repository.to_owned()]
        } else {
            manifests
                .iter()
                .filter_map(|manifest| manifest.parent().map(Path::to_owned))
                .collect()
        };
        let mut resolved: Option<Self> = None;
        for directory in directories {
            let candidate = Self::resolve_with_path(&directory, scope, cancelled, path.clone())?;
            if let Some(previous) = &resolved {
                if previous.sysroot != candidate.sysroot {
                    return Err(failure("此仓库的多个 Rust 项目使用不同工具链；当前标准库分析尚不支持这种组合。请切换为仅仓库范围，Narrant 不会自动安装组件或替换项目工具链。"));
                }
            } else {
                resolved = Some(candidate);
            }
        }
        let mut resolved = resolved.ok_or_else(|| failure("没有可检查的 Rust 项目"))?;
        resolved.manifests = manifests;
        Ok(resolved)
    }

    /// 将工具发现与项目检查分开，测试使用隔离工具目录而不修改进程环境。
    fn resolve_with_path(
        repository: &Path,
        scope: RustDependencyScope,
        cancelled: &CancellationCheck,
        path: OsString,
    ) -> Result<Self> {
        probe("cargo", &["--version"], repository, &path, cancelled)?;
        let root = PathBuf::from(
            probe(
                "rustc",
                &["--print", "sysroot"],
                repository,
                &path,
                cancelled,
            )?
            .trim(),
        );
        if !root.is_absolute() || !root.is_dir() {
            return Err(failure(
                "项目 Rust 工具链返回了无效 sysroot，请先准备项目对应的 Cargo/rustc。",
            ));
        }
        if scope == RustDependencyScope::RepositoryOnly {
            return Ok(Self {
                sysroot: None,
                sources: None,
                path,
                manifests: vec![],
            });
        }
        let sources = root.join("lib/rustlib/src/rust/library");
        if !sources.join("core/src/lib.rs").is_file() || !sources.join("std/src/lib.rs").is_file() {
            return Err(failure(format!("项目对应的 Rust 工具链缺少 rust-src（{}）。请自行安装该工具链的 rust-src，或将分析范围改为仅仓库。Narrant 不会自动安装组件。", root.display())));
        }
        Ok(Self {
            sysroot: Some(root),
            sources: Some(sources),
            path,
            manifests: vec![],
        })
    }
}

/// 对齐固定 rust-analyzer 的发现层级：父级显式项目优先，否则只看直接子目录。
fn discover_manifests(repository: &Path) -> Result<Vec<PathBuf>> {
    for name in ["rust-project.json", ".rust-project.json", "Cargo.toml"] {
        if let Some(manifest) = repository
            .ancestors()
            .map(|directory| directory.join(name))
            .find(|path| path.is_file())
        {
            return Ok(vec![manifest]);
        }
    }
    let mut manifests = Vec::new();
    for entry in std::fs::read_dir(repository).map_err(failure)? {
        let manifest = entry.map_err(failure)?.path().join("Cargo.toml");
        if manifest.is_file() {
            manifests.push(manifest);
        }
    }
    manifests.sort();
    Ok(manifests)
}

/// 短探测进程在超时、取消和提前返回时均被回收。
struct ProbeProcess(Child);
impl Drop for ProbeProcess {
    /// 正常完成后的 kill 无影响，异常时保证 rustup 代理不会继续运行。
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            // SAFETY: 此探测使用独立进程组；负 PID 只回收自己创建的组。
            unsafe {
                libc::kill(-(self.0.id() as i32), libc::SIGKILL);
            }
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// 在项目目录执行有界只读探测，禁止 rustup 自动获取缺失工具链。
fn probe(
    tool: &str,
    args: &[&str],
    repository: &Path,
    path: &OsString,
    cancelled: &CancellationCheck,
) -> Result<String> {
    let temporary = tempfile::tempdir().map_err(failure)?;
    let stdout = temporary.path().join("stdout");
    let stderr = temporary.path().join("stderr");
    let mut command = Command::new(tool);
    command
        .args(args)
        .current_dir(repository)
        .env("PATH", path)
        .env("RUSTUP_AUTO_INSTALL", "0")
        .stdin(Stdio::null())
        .stdout(File::create(&stdout).map_err(failure)?)
        .stderr(File::create(&stderr).map_err(failure)?);
    crate::child_process::configure_std_command(&mut command);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut process = ProbeProcess(command.spawn().map_err(|error| {
        failure(format!(
            "无法使用项目的 {tool}：{error}。请先安装项目对应的 Rust 工具链。"
        ))
    })?);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if cancelled() {
            return Err(super::super::code_graph::AnalysisError::cancelled(
                "Rust 工具链检查已取消",
            ));
        }
        if let Some(status) = process.0.try_wait().map_err(failure)? {
            let mut output = String::new();
            File::open(if status.success() { stdout } else { stderr })
                .map_err(failure)?
                .take(8192)
                .read_to_string(&mut output)
                .map_err(failure)?;
            if !status.success() {
                return Err(failure(format!(
                    "项目的 {tool} 不可用：{output}。请自行准备项目对应的 Rust 工具链。"
                )));
            }
            return Ok(output);
        }
        if Instant::now() >= deadline {
            return Err(failure(format!("检查项目 {tool} 超时")));
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(all(test, unix))]
#[path = "tests/toolchain.rs"]
mod tests;
