//! Cargo 与工具链政策是明确的类型，调用者不能注入命令或任意环境覆盖。

use anyhow::{Context, Result, bail, ensure};
use hir_def::{
    AdtId, ExpressionStoreOwnerId, GenericDefId, ModuleDefId,
    expr_store::ExpressionStore,
    nameres::{DefMap, crate_def_map, diagnostics::DefDiagnosticKind},
};
use ide::RootDatabase;
use load_cargo::{LoadCargoConfig, ProcMacroServerChoice};
use project_model::{
    CargoConfig, ProjectManifest, ProjectWorkspace, ProjectWorkspaceKind, RustLibSource,
};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::Mutex,
};
use vfs::{AbsPathBuf, Vfs};

/// 与现有 Rust 设置对应的三档语义依赖范围。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DependencyScope {
    RepositoryOnly,
    StandardLibrary,
    AllDependencies,
}

/// 全部依赖档沿用用户网络政策，其余两档始终离线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NetworkPolicy {
    Offline,
    Allow,
}

/// 已由项目工具链发现阶段确认的路径，与 helper 的构建工具链相互独立。
#[derive(Clone, Debug)]
pub struct ProjectToolchain {
    pub sysroot: PathBuf,
    pub rust_src: PathBuf,
}

/// 单次加载的明确政策；测试 cfg 不等同于后续图中测试节点的过滤开关。
#[derive(Clone, Debug)]
pub struct LoadPolicy {
    pub dependency_scope: DependencyScope,
    pub network: NetworkPolicy,
    pub test_cfg: bool,
    pub project_toolchain: Option<ProjectToolchain>,
}

impl LoadPolicy {
    /// 构造默认的仅仓库离线分析，沿用旧入口启用 cfg(test) 的语义上下文。
    pub fn repository_only() -> Self {
        Self {
            dependency_scope: DependencyScope::RepositoryOnly,
            network: NetworkPolicy::Offline,
            test_cfg: true,
            project_toolchain: None,
        }
    }

    /// 只对必需的项目组件做存在检查，不使用会自动安装 rust-src 的 discover 分支。
    fn cargo_config(&self) -> Result<CargoConfig> {
        let mut config = CargoConfig {
            set_test: self.test_cfg,
            no_deps: self.dependency_scope == DependencyScope::RepositoryOnly,
            ..CargoConfig::default()
        };
        config
            .extra_env
            .insert("RUSTUP_AUTO_INSTALL".into(), Some("0".into()));
        // 即使 target 已存在，Cargo 的版本探测也不能在其中写入 .rustc_info.json。
        config
            .extra_env
            .insert("CARGO_CACHE_RUSTC_INFO".into(), Some("0".into()));
        // Cargo +toolchain 会把构建版本传播给测试/子进程，不能据此覆盖被分析项目。
        config.extra_env.insert("RUSTUP_TOOLCHAIN".into(), None);
        if self.dependency_scope != DependencyScope::AllDependencies
            || self.network == NetworkPolicy::Offline
        {
            config
                .extra_env
                .insert("CARGO_NET_OFFLINE".into(), Some("true".into()));
        }
        if self.dependency_scope == DependencyScope::StandardLibrary {
            // 只限制项目 metadata；不能用 no_deps 截断标准库内部依赖。
            config.metadata_extra_args.push("--no-deps".into());
        }
        if self.dependency_scope != DependencyScope::RepositoryOnly {
            let toolchain = self
                .project_toolchain
                .as_ref()
                .context("标准库分析需要已发现的项目工具链与 rust-src")?;
            ensure!(
                toolchain.sysroot.is_dir(),
                "项目 sysroot 不存在：{}",
                toolchain.sysroot.display()
            );
            ensure!(
                toolchain.rust_src.join("core/src/lib.rs").is_file()
                    && toolchain.rust_src.join("std/src/lib.rs").is_file(),
                "项目工具链缺少 rust-src：{}；不会自动安装组件",
                toolchain.rust_src.display()
            );
            let sysroot = absolute_utf8(&toolchain.sysroot)?;
            let sources = absolute_utf8(&toolchain.rust_src)?;
            // 显式覆盖由 helper 构建进程继承的版本，Cargo/rustc 均使用已发现的项目 sysroot。
            config
                .extra_env
                .insert("RUSTUP_TOOLCHAIN".into(), Some(sysroot.to_string()));
            config.sysroot = Some(RustLibSource::Path(sysroot));
            config.sysroot_src = Some(sources);
        }
        Ok(config)
    }
}

/// 一次加载事务隔离原生内部异常；失败即丢弃数据库，不探测输入、不重试或返回部分结果。
pub(super) fn load(manifest: &Path, policy: &LoadPolicy) -> Result<(RootDatabase, Vfs)> {
    crate::native_transaction("Rust 工作区未完整加载", || {
        load_native(manifest, policy)
    })
}

/// 先检查 ProjectWorkspace 的可降级错误，再交给原生 loader 建立数据库并检查模块。
fn load_native(manifest: &Path, policy: &LoadPolicy) -> Result<(RootDatabase, Vfs)> {
    let mut config = policy.cargo_config()?;
    ensure!(
        manifest
            .file_name()
            .is_some_and(|name| name == "Cargo.toml"),
        "此核心阶段只接受 Cargo.toml"
    );
    let manifest = ProjectManifest::from_manifest_file(absolute_utf8(manifest)?)?;
    // tempfile 必须存活到原生 metadata 完成，路径永远不在分析工作区中。
    let lockfile_directory = tempfile::tempdir().context("无法准备只读 metadata 锁文件目录")?;
    let workspace = if policy.dependency_scope == DependencyScope::AllDependencies {
        // no-deps 只读发现真实 workspace 根和项目版本，同时完整加载标准库。
        let mut discovery = config.clone();
        discovery.metadata_extra_args.push("--no-deps".into());
        let standard_policy = LoadPolicy {
            dependency_scope: DependencyScope::StandardLibrary,
            ..policy.clone()
        };
        let standard = load_complete(manifest.clone(), &discovery, &standard_policy)?;
        redirect_project_lockfile(&mut config, &standard, lockfile_directory.path())?;
        // sysroot 的 metadata 使用自己的 Cargo.lock，不能继承项目锁文件重定向。
        // 后续只替换 Cargo 项目模型；sysroot、cfg 和工具链仍来自同一真实项目配置。
        config.sysroot = None;
        config.sysroot_src = None;
        let repository_policy = LoadPolicy {
            dependency_scope: DependencyScope::RepositoryOnly,
            ..policy.clone()
        };
        let mut complete = load_complete(manifest, &config, &repository_policy)?;
        complete.sysroot = standard.sysroot;
        complete
    } else {
        load_complete(manifest, &config, policy)?
    };
    let load_config = LoadCargoConfig {
        load_out_dirs_from_check: false,
        with_proc_macro_server: ProcMacroServerChoice::None,
        prefill_caches: false,
        num_worker_threads: 1,
        proc_macro_processes: 1,
    };
    let (database, vfs, macro_client) =
        load_cargo::load_workspace(workspace, &config.extra_env, &load_config)?;
    ensure!(macro_client.is_none(), "禁止启动过程宏服务");
    ensure!(vfs.iter().next().is_some(), "Cargo 工作区未加载任何文件");
    ensure_sources_complete(&database, &vfs)?;
    Ok((database, vfs))
}

/// 上游会丢弃读取错误；沿原生 crate 与 block 模块图检查活动声明的源码候选。
fn ensure_sources_complete(database: &RootDatabase, vfs: &Vfs) -> Result<()> {
    // 读取/遍历失败的文件不在 VFS 中，使用原生 cfg、#[path] 与嵌套目录解析结果定位。
    let mut maps: Vec<_> = base_db::all_crates(database)
        .iter()
        .map(|&krate| crate_def_map(database, krate))
        .collect();
    let mut definitions = Vec::new();
    let mut stores = Vec::new();
    let mut checked_stores = HashSet::new();
    let mut checked_blocks = HashSet::new();
    loop {
        if let Some(map) = maps.pop() {
            ensure_module_candidates_complete(database, vfs, map)?;
            for (_, module) in map.modules() {
                definitions.extend(module.scope.declarations());
                definitions.extend(module.scope.unnamed_consts().map(ModuleDefId::from));
                for implementation in module.scope.impls() {
                    stores.push(implementation.into());
                    definitions.extend(
                        implementation
                            .impl_items(database)
                            .items
                            .iter()
                            .map(|&(_, item)| ModuleDefId::from(item)),
                    );
                }
            }
        } else if let Some(definition) = definitions.pop() {
            queue_definition_stores(database, definition, &mut definitions, &mut stores);
        } else if let Some(owner) = stores.pop() {
            if checked_stores.insert(owner) {
                for (block, map) in ExpressionStore::of(database, owner).blocks(database) {
                    if checked_blocks.insert(block) {
                        maps.push(map);
                    }
                }
            }
        } else {
            break;
        }
    }
    Ok(())
}

/// 沿原生声明取得 body、签名和字段中的块，关联项和块内嵌套声明都继续检查。
fn queue_definition_stores(
    database: &RootDatabase,
    definition: ModuleDefId,
    definitions: &mut Vec<ModuleDefId>,
    stores: &mut Vec<ExpressionStoreOwnerId>,
) {
    match definition {
        ModuleDefId::FunctionId(id) => {
            stores.extend([
                ExpressionStoreOwnerId::Signature(id.into()),
                ExpressionStoreOwnerId::Body(id.into()),
            ]);
        }
        ModuleDefId::ConstId(id) => {
            stores.extend([
                ExpressionStoreOwnerId::Signature(id.into()),
                ExpressionStoreOwnerId::Body(id.into()),
            ]);
        }
        ModuleDefId::StaticId(id) => {
            stores.extend([
                ExpressionStoreOwnerId::Signature(id.into()),
                ExpressionStoreOwnerId::Body(id.into()),
            ]);
        }
        ModuleDefId::EnumVariantId(id) => {
            stores.extend([
                ExpressionStoreOwnerId::VariantFields(id.into()),
                ExpressionStoreOwnerId::Body(id.into()),
            ]);
        }
        ModuleDefId::AdtId(AdtId::EnumId(id)) => {
            stores.push(GenericDefId::from(id).into());
            definitions.extend(
                id.enum_variants(database)
                    .variants
                    .values()
                    .map(|&(variant, _)| ModuleDefId::from(variant)),
            );
        }
        ModuleDefId::AdtId(AdtId::StructId(id)) => {
            stores.extend([
                ExpressionStoreOwnerId::Signature(id.into()),
                ExpressionStoreOwnerId::VariantFields(id.into()),
            ]);
        }
        ModuleDefId::AdtId(AdtId::UnionId(id)) => {
            stores.extend([
                ExpressionStoreOwnerId::Signature(id.into()),
                ExpressionStoreOwnerId::VariantFields(id.into()),
            ]);
        }
        ModuleDefId::TraitId(id) => {
            stores.push(GenericDefId::from(id).into());
            definitions.extend(
                id.trait_items(database)
                    .items
                    .iter()
                    .map(|&(_, item)| ModuleDefId::from(item)),
            );
        }
        ModuleDefId::TypeAliasId(id) => stores.push(GenericDefId::from(id).into()),
        ModuleDefId::ModuleId(_) | ModuleDefId::MacroId(_) | ModuleDefId::BuiltinType(_) => {}
    }
}

/// crate 与 block 的 DefMap 使用相同候选检查，真正缺失模块仍保留为项目诊断。
fn ensure_module_candidates_complete(
    database: &RootDatabase,
    vfs: &Vfs,
    map: &DefMap,
) -> Result<()> {
    for diagnostic in map.diagnostics() {
        let DefDiagnosticKind::UnresolvedModule { ast, candidates } = &diagnostic.kind else {
            continue;
        };
        let anchor = ast
            .file_id
            .original_file_respecting_includes(database)
            .file_id(database);
        let directory = vfs
            .file_path(anchor)
            .parent()
            .context("未解析模块的源码没有父目录")?;
        for candidate in candidates {
            let candidate = directory.join(candidate).context("模块候选路径不可用")?;
            let path = candidate.as_path().context("模块候选没有真实源码路径")?;
            match std::fs::read_to_string(path) {
                // 真正缺失的模块属于项目诊断；现有源码被 loader 漏掉才是加载错误。
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| format!("Rust 模块源码未完整加载：{path}"));
                }
                Ok(_) => bail!("Rust 模块源码未完整加载到 VFS：{path}"),
            }
        }
    }
    Ok(())
}

/// 对单次原生加载收集进度错误，不能把上游的部分 workspace 当成完整结果。
fn load_complete(
    manifest: ProjectManifest,
    config: &CargoConfig,
    policy: &LoadPolicy,
) -> Result<ProjectWorkspace> {
    let progress = Mutex::new(Vec::new());
    let workspace = ProjectWorkspace::load(manifest, config, &|message| {
        progress.lock().expect("加载进度锁被破坏").push(message);
    })
    .with_context(|| {
        if config
            .metadata_extra_args
            .iter()
            .any(|arg| arg == "--locked")
        {
            "只读分析不能更新此项目的 Cargo.lock；请先准备与清单一致的锁文件"
        } else {
            "Rust 原生项目加载失败"
        }
    })?;
    ensure_complete(
        &workspace,
        &progress.into_inner().expect("加载进度锁被破坏"),
        policy,
    )
    .with_context(|| {
        if config
            .metadata_extra_args
            .iter()
            .any(|arg| arg == "--locked")
        {
            "只读分析不能更新此项目的 Cargo.lock；请先准备与清单一致的锁文件"
        } else {
            "Rust 原生项目加载未完成"
        }
    })?;
    Ok(workspace)
}

/// 沿用固定上游按真实 Cargo 版本选择的锁文件机制，补齐缺失锁文件的只读处理。
fn redirect_project_lockfile(
    config: &mut CargoConfig,
    workspace: &ProjectWorkspace,
    directory: &Path,
) -> Result<()> {
    let version = workspace
        .toolchain
        .as_ref()
        .context("无法确定项目 Cargo 工具链版本")?;
    if (version.major, version.minor) < (1, 82) {
        // 老 Cargo 不支持独立锁文件；已有有效锁文件仍可解析，只拒绝需要写回的情况。
        config.metadata_extra_args.push("--locked".into());
        return Ok(());
    }
    let ProjectWorkspaceKind::Cargo { cargo, .. } = &workspace.kind else {
        bail!("只读锁文件处理需要 Cargo workspace");
    };
    let original = cargo.workspace_root().join("Cargo.lock");
    let temporary = directory.join("Cargo.lock");
    if Path::new(original.as_str()).exists() {
        std::fs::copy(&original, &temporary).context("无法复制项目的 Cargo.lock 解析约束")?;
    }
    let temporary = temporary
        .to_str()
        .context("临时 metadata 锁文件路径必须为 UTF-8")?;
    if (version.major, version.minor) >= (1, 97) {
        config.extra_env.insert(
            "CARGO_RESOLVER_LOCKFILE_PATH".into(),
            Some(temporary.into()),
        );
    } else {
        config.extra_env.insert(
            "__CARGO_TEST_CHANNEL_OVERRIDE_DO_NOT_USE_THIS".into(),
            Some("nightly".into()),
        );
        config.metadata_extra_args.push("-Zunstable-options".into());
        if (version.major, version.minor) >= (1, 95) {
            config.extra_env.insert(
                "CARGO_RESOLVER_LOCKFILE_PATH".into(),
                Some(temporary.into()),
            );
            config.metadata_extra_args.push("-Zlockfile-path".into());
        } else {
            config
                .metadata_extra_args
                .extend(["--lockfile-path".into(), temporary.into()]);
        }
    }
    Ok(())
}

/// 原生 API 的 Ok 可能包含 metadata 回退；这些情况必须转成可观察失败。
fn ensure_complete(
    workspace: &ProjectWorkspace,
    progress: &[String],
    policy: &LoadPolicy,
) -> Result<()> {
    if let ProjectWorkspaceKind::Cargo {
        error: Some(error), ..
    } = &workspace.kind
    {
        bail!("Cargo 工作区未完整加载：{error:#}");
    }
    if let ProjectWorkspaceKind::Cargo { cargo, .. } = &workspace.kind {
        for package in cargo.packages() {
            for target in &cargo[package].targets {
                let root = &cargo[*target].root;
                std::fs::read_to_string(root)
                    .with_context(|| format!("Cargo crate 根源码未完整加载：{root}"))?;
            }
        }
    }
    if let Some(error) = workspace
        .sysroot
        .error()
        .or_else(|| workspace.sysroot.metadata_error())
    {
        bail!("Rust sysroot 未完整加载：{error}");
    }
    if let Err(error) = &workspace.target {
        bail!("Rust 目标数据未完整加载：{error}");
    }
    ensure!(
        workspace.toolchain.is_some() && !workspace.rustc_cfg.is_empty(),
        "项目工具链或目标 cfg 未完整加载"
    );
    if let Some(error) = progress
        .iter()
        .find(|line| line.starts_with("cargo metadata: failed"))
    {
        bail!("Rust metadata 发生降级：{error}");
    }
    if policy.dependency_scope != DependencyScope::RepositoryOnly {
        ensure!(
            workspace.sysroot.num_packages() > 0,
            "Rust 标准库未完整加载"
        );
    }
    Ok(())
}

/// 将存在的路径规范化为上游所需的 UTF-8 绝对路径，避免上游 assert 引发 panic。
fn absolute_utf8(path: &Path) -> Result<AbsPathBuf> {
    let absolute = path
        .canonicalize()
        .with_context(|| format!("路径不可用：{}", path.display()))?;
    ensure!(
        absolute.to_str().is_some(),
        "路径必须为 UTF-8 绝对路径：{}",
        path.display()
    );
    Ok(AbsPathBuf::assert_utf8(absolute))
}

#[cfg(test)]
#[path = "tests/loading.rs"]
mod tests;
