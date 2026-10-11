use super::*;
use crate::RustProject;
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use syntax::AstNode;

use crate::test_support as support;

/// 已声明的非根模块不能因无效 UTF-8 被 loader 静默丢弃后仍返回成功。
#[test]
fn invalid_utf8_in_declared_module_is_observable() {
    let root = support::fixture();
    std::fs::write(root.path().join("src/lib.rs"), "mod bad;\n").unwrap();
    std::fs::write(root.path().join("src/bad.rs"), [0xff, 0xfe]).unwrap();
    let before = support::snapshot(root.path());
    let error = match RustProject::load(
        &root.path().join("Cargo.toml"),
        &LoadPolicy::repository_only(),
    ) {
        Ok(_) => panic!("非根模块源码解码失败不能变成部分加载成功"),
        Err(error) => error,
    };
    let message = format!("{error:#}");
    assert!(
        message.contains("未完整加载") && message.contains("file text"),
        "{message}"
    );
    support::assert_unchanged(root.path(), &before);
}

/// #[path] 可以引用其他原生 VFS 文本扩展，块遍历不能因漏解码输入触发上游 panic。
#[test]
fn invalid_utf8_in_block_module_with_text_extension_is_observable() {
    let root = support::fixture();
    std::fs::write(
        root.path().join("src/lib.rs"),
        "pub fn run() { #[path=\"bad.md\"] mod bad; }",
    )
    .unwrap();
    std::fs::write(root.path().join("src/bad.md"), [0xff, 0xfe]).unwrap();
    let before = support::snapshot(root.path());
    let error = match RustProject::load(
        &root.path().join("Cargo.toml"),
        &LoadPolicy::repository_only(),
    ) {
        Ok(_) => panic!("块内模块源码解码失败不能变成部分加载成功"),
        Err(error) => error,
    };
    let message = format!("{error:#}");
    assert!(
        message.contains("未完整加载") && message.contains("file text"),
        "{message}"
    );
    support::assert_unchanged(root.path(), &before);
}

/// 同一路径的二进制资源可用于 include_bytes!，不能因为扩展名被当作 Rust 文本拒绝。
#[test]
fn binary_resource_is_not_misclassified_as_module_source() {
    for extension in ["md", "rs", "bin"] {
        let root = support::fixture();
        let source = format!("pub fn run() {{ let bytes = include_bytes!(\"bad.{extension}\"); }}");
        std::fs::write(root.path().join("src/lib.rs"), &source).unwrap();
        std::fs::write(
            root.path().join(format!("src/bad.{extension}")),
            [0xff, 0xfe],
        )
        .unwrap();
        let before = support::snapshot(root.path());
        let project = RustProject::load(
            &root.path().join("Cargo.toml"),
            &LoadPolicy::repository_only(),
        )
        .unwrap();
        let file = project.file_id(&root.path().join("src/lib.rs")).unwrap();
        assert_eq!(project.source(file).unwrap(), source);
        support::assert_unchanged(root.path(), &before);
    }
}

/// 不可读取的模块路径不在 VFS 中，也必须由原生模块候选定位并报告错误。
#[test]
fn unreadable_declared_module_is_observable() {
    let root = support::fixture();
    std::fs::write(root.path().join("src/lib.rs"), "mod bad;\n").unwrap();
    // 目录能稳定触发 read 错误，不依赖测试进程的权限或操作系统 chmod 行为。
    std::fs::create_dir(root.path().join("src/bad.rs")).unwrap();
    let before = support::snapshot(root.path());
    let error = match RustProject::load(
        &root.path().join("Cargo.toml"),
        &LoadPolicy::repository_only(),
    ) {
        Ok(_) => panic!("非根模块源码读取失败不能变成部分加载成功"),
        Err(error) => error,
    };
    let message = format!("{error:#}");
    assert!(
        message.contains("bad.rs") && message.contains("未完整加载"),
        "{message}"
    );
    support::assert_unchanged(root.path(), &before);
}

/// 函数内模块拥有独立 Block DefMap，读取失败也必须在接受数据库前报告。
#[test]
fn unreadable_block_local_module_is_observable() {
    // 不只函数：原生 ExpressionStore 也持有闭包、关联项、常量和签名中的块。
    for source in [
        "pub fn run() { #[path=\"bad.rs\"] mod bad; }",
        "pub fn run() { fn nested() { #[path=\"bad.rs\"] mod bad; } }",
        "pub fn run() { let closure = || { #[path=\"bad.rs\"] mod bad; }; }",
        "struct S; impl S { fn run() { #[path=\"bad.rs\"] mod bad; } }",
        "trait T { fn run() { #[path=\"bad.rs\"] mod bad; } }",
        "struct S; impl S { const VALUE: () = { #[path=\"bad.rs\"] mod bad; }; }",
        "static VALUE: () = { #[path=\"bad.rs\"] mod bad; };",
        "enum E { A = { #[path=\"bad.rs\"] mod bad; 0 } }",
        "type Alias = [u8; { #[path=\"bad.rs\"] mod bad; 0 }];",
        "struct S { field: [u8; { #[path=\"bad.rs\"] mod bad; 0 }] }",
        "macro_rules! declare { () => { #[path=\"bad.rs\"] mod bad; }; } fn run() { declare!(); }",
    ] {
        let root = support::fixture();
        std::fs::write(root.path().join("src/lib.rs"), source).unwrap();
        std::fs::create_dir(root.path().join("src/bad.rs")).unwrap();
        let before = support::snapshot(root.path());
        let error = match RustProject::load(
            &root.path().join("Cargo.toml"),
            &LoadPolicy::repository_only(),
        ) {
            Ok(_) => panic!("块内模块的读取失败不能变成部分加载成功：{source}"),
            Err(error) => error,
        };
        let message = format!("{error:#}");
        assert!(
            message.contains("bad.rs") && message.contains("未完整加载"),
            "{source}: {message}"
        );
        support::assert_unchanged(root.path(), &before);
    }
}

/// 缺失或 cfg 排除的模块仍是项目诊断，Cargo 排除目录中的无效文件不扩大加载范围。
#[test]
fn source_validation_preserves_native_scope_and_missing_module_diagnostics() {
    let root = support::fixture();
    std::fs::write(
        root.path().join("src/lib.rs"),
        "mod missing;\n#[cfg(any())] mod inactive;\n\
        fn run() { #[path=\"missing_block.rs\"] mod missing_block;\n\
        #[cfg(any())] #[path=\"inactive.rs\"] mod inactive_block; }\n\
        #[cfg(any())] fn inactive_body() { #[path=\"inactive.rs\"] mod inactive_block; }\n",
    )
    .unwrap();
    std::fs::create_dir(root.path().join("src/inactive.rs")).unwrap();
    std::fs::create_dir(root.path().join(".git")).unwrap();
    std::fs::write(root.path().join(".git/ignored.rs"), [0xff, 0xfe]).unwrap();
    std::fs::create_dir(root.path().join("target")).unwrap();
    std::fs::write(root.path().join("target/ignored.rs"), [0xff, 0xfe]).unwrap();
    let before = support::snapshot(root.path());
    RustProject::load(
        &root.path().join("Cargo.toml"),
        &LoadPolicy::repository_only(),
    )
    .unwrap();
    support::assert_unchanged(root.path(), &before);
}

/// 本地 HTTP 计数器捕获 Cargo 的任何下载尝试，不依赖公网是否可用。
struct NetworkTrap {
    url: String,
    requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl NetworkTrap {
    /// 启动稀疏 registry 的错误响应服务，所有网络访问都会留下证据。
    fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("sparse+http://{}/", listener.local_addr().unwrap());
        let requests = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (count, stopped) = (requests.clone(), stop.clone());
        let thread = std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                if let Ok((mut stream, _)) = listener.accept() {
                    count.fetch_add(1, Ordering::Relaxed);
                    stream
                        .set_read_timeout(Some(Duration::from_secs(1)))
                        .unwrap();
                    let _ = stream.read(&mut [0; 2048]);
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                } else {
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        });
        Self {
            url,
            requests,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for NetworkTrap {
    /// 回收本地观测线程，不留下常驻服务。
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().unwrap().join().unwrap();
    }
}

/// 使用唯一 registry 地址，保证依赖从未在缓存中，下载证据可归属于本次 fixture。
fn uncached_dependency(root: &Path, trap: &NetworkTrap) {
    std::fs::create_dir_all(root.join(".cargo")).unwrap();
    std::fs::write(root.join(".cargo/config.toml"), format!("[source.crates-io]\nreplace-with = \"network-trap\"\n[source.network-trap]\nregistry = \"{}\"\n[net]\nretry=0\n", trap.url)).unwrap();
    let manifest = root.join("Cargo.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        manifest,
        format!("{text}\nnonexistent_narrant_proof_dependency = \"=0.0.123\"\n"),
    )
    .unwrap();
}

/// 离线两档从未触达 registry；仅仓库可以完整加载有缺失第三方依赖的源码。
#[test]
fn restricted_scopes_do_not_download_or_rewrite_fixture() {
    let trap = NetworkTrap::start();
    let root = support::fixture();
    uncached_dependency(root.path(), &trap);
    let before = support::snapshot(root.path());
    let repository = RustProject::load(
        &root.path().join("Cargo.toml"),
        &LoadPolicy::repository_only(),
    )
    .unwrap();
    let file = repository.file_id(&root.path().join("src/lib.rs")).unwrap();
    assert!(!repository.source(file).unwrap().is_empty());
    let policy = LoadPolicy {
        dependency_scope: DependencyScope::StandardLibrary,
        network: NetworkPolicy::Allow,
        test_cfg: true,
        project_toolchain: Some(support::project_toolchain(root.path())),
    };
    let standard = RustProject::load(&root.path().join("Cargo.toml"), &policy).unwrap();
    let file = standard.file_id(&root.path().join("src/lib.rs")).unwrap();
    assert!(matches!(
        standard.parent_modules(file).unwrap().as_slice(),
        [crate::ModuleParent::CrateRoot]
    ));
    assert_eq!(trap.requests.load(Ordering::Relaxed), 0);
    support::assert_unchanged(root.path(), &before);
}

/// 全部依赖遵守离线政策；上游 metadata 的 no-deps 回退必须作为错误返回。
#[test]
fn dependency_metadata_degradation_is_observable() {
    let trap = NetworkTrap::start();
    let root = support::fixture();
    uncached_dependency(root.path(), &trap);
    let before = support::snapshot(root.path());
    let policy = LoadPolicy {
        dependency_scope: DependencyScope::AllDependencies,
        network: NetworkPolicy::Offline,
        test_cfg: true,
        project_toolchain: Some(support::project_toolchain(root.path())),
    };
    let error = match RustProject::load(&root.path().join("Cargo.toml"), &policy) {
        Ok(_) => panic!("metadata 失败不能变成部分加载成功"),
        Err(error) => error,
    };
    assert!(format!("{error:#}").contains("未完整加载"), "{error:#}");
    assert_eq!(trap.requests.load(Ordering::Relaxed), 0);
    support::assert_unchanged(root.path(), &before);
}

/// 明确的网络允许档可以访问 registry，但失败仍不能伪装为成功。
#[test]
fn all_dependencies_obeys_explicit_network_policy() {
    let trap = NetworkTrap::start();
    let root = support::fixture();
    uncached_dependency(root.path(), &trap);
    let before = support::snapshot(root.path());
    let policy = LoadPolicy {
        dependency_scope: DependencyScope::AllDependencies,
        network: NetworkPolicy::Allow,
        test_cfg: true,
        project_toolchain: Some(support::project_toolchain(root.path())),
    };
    assert!(RustProject::load(&root.path().join("Cargo.toml"), &policy).is_err());
    assert!(trap.requests.load(Ordering::Relaxed) > 0);
    support::assert_unchanged(root.path(), &before);
}

/// 缺 rust-src 在任何加载进程启动之前拒绝，不安装组件、不改写项目。
#[test]
fn missing_rust_src_fails_without_installing_or_modifying_workspace() {
    let root = support::fixture();
    let fake_sysroot = root.path().join("fake-sysroot");
    std::fs::create_dir(&fake_sysroot).unwrap();
    let before = support::snapshot(root.path());
    let policy = LoadPolicy {
        dependency_scope: DependencyScope::StandardLibrary,
        network: NetworkPolicy::Allow,
        test_cfg: true,
        project_toolchain: Some(ProjectToolchain {
            sysroot: fake_sysroot.clone(),
            rust_src: fake_sysroot.join("lib/rustlib/src/rust/library"),
        }),
    };
    let error = match RustProject::load(&root.path().join("Cargo.toml"), &policy) {
        Ok(_) => panic!("缺少 rust-src 必须报错"),
        Err(error) => error,
    };
    // 原生事务会增加上下文；保留对实际缺少源码原因的断言，检查完整错误链。
    assert!(format!("{error:#}").contains("缺少 rust-src"), "{error:#}");
    support::assert_unchanged(root.path(), &before);
}

/// 禁止自动安装项目指定的未安装版本；Cargo/rustup 失败必须保留为错误。
#[test]
fn missing_project_toolchain_is_not_installed() {
    let root = support::fixture();
    std::fs::write(
        root.path().join("rust-toolchain.toml"),
        "[toolchain]\nchannel=\"1.0.0-narrant-uninstalled\"\n",
    )
    .unwrap();
    let before = support::snapshot(root.path());
    let error = match RustProject::load(
        &root.path().join("Cargo.toml"),
        &LoadPolicy::repository_only(),
    ) {
        Ok(_) => panic!("缺失项目工具链不能使用 helper 工具链代替"),
        Err(error) => error,
    };
    assert!(format!("{error:#}").contains("未完整加载"), "{error:#}");
    support::assert_unchanged(root.path(), &before);
}

/// 全部依赖成功加载本地过程宏 crate，构建脚本和宏执行标记仍应不存在。
#[test]
fn all_dependencies_loads_local_crates_without_executing_user_code() {
    let root = support::fixture();
    let before = support::snapshot(root.path());
    let policy = LoadPolicy {
        dependency_scope: DependencyScope::AllDependencies,
        network: NetworkPolicy::Offline,
        test_cfg: true,
        project_toolchain: Some(support::project_toolchain(root.path())),
    };
    let project = RustProject::load(&root.path().join("Cargo.toml"), &policy).unwrap();
    let file = project.file_id(&root.path().join("src/lib.rs")).unwrap();
    let source = project.source(file).unwrap();
    let offset = source.find("marker_macros::marker").unwrap() as u32 + 15;
    let definitions = project.definitions(file, offset).unwrap();
    assert!(
        definitions
            .iter()
            .any(|target| target.name.as_str() == "marker")
    );
    assert!(
        project
            .parse(file)
            .unwrap()
            .syntax()
            .text()
            .to_string()
            .contains("marker_macros::marker!()")
    );
    support::assert_unchanged(root.path(), &before);
}

/// sysroot metadata 失败后上游会拼接部分标准库，核心必须拒绝这种降级成功。
#[test]
fn sysroot_metadata_failure_cannot_be_a_successful_stitched_load() {
    let root = support::fixture();
    let sources = root.path().join("broken-rust-src");
    for name in ["core", "std"] {
        std::fs::create_dir_all(sources.join(name).join("src")).unwrap();
        std::fs::write(sources.join(name).join("src/lib.rs"), "").unwrap();
    }
    std::fs::write(sources.join("Cargo.toml"), "[invalid malformed manifest").unwrap();
    let before = support::snapshot(root.path());
    let mut toolchain = support::project_toolchain(root.path());
    toolchain.rust_src = sources;
    let policy = LoadPolicy {
        dependency_scope: DependencyScope::StandardLibrary,
        network: NetworkPolicy::Offline,
        test_cfg: true,
        project_toolchain: Some(toolchain),
    };
    let error = match RustProject::load(&root.path().join("Cargo.toml"), &policy) {
        Ok(_) => panic!("sysroot metadata 错误不能变成成功"),
        Err(error) => error,
    };
    assert!(format!("{error:#}").contains("降级"), "{error:#}");
    support::assert_unchanged(root.path(), &before);
}

/// 成员清单的无锁工作区仍可完整分析，Cargo.lock 只在独立临时目录生成。
#[test]
fn member_manifest_without_lockfile_keeps_real_workspace_unchanged() {
    let root = support::fixture();
    let before = support::snapshot(root.path());
    let policy = LoadPolicy {
        dependency_scope: DependencyScope::AllDependencies,
        network: NetworkPolicy::Offline,
        test_cfg: true,
        project_toolchain: Some(support::project_toolchain(root.path())),
    };
    let project = RustProject::load(&root.path().join("macros/Cargo.toml"), &policy).unwrap();
    let file = project.file_id(&root.path().join("src/lib.rs")).unwrap();
    assert!(matches!(
        project.parent_modules(file).unwrap().as_slice(),
        [crate::ModuleParent::CrateRoot]
    ));
    support::assert_unchanged(root.path(), &before);
}

/// 已有锁文件从真实 workspace 根复制，成员目录不能误选另一份解析约束。
#[test]
fn lockfile_redirection_copies_real_workspace_constraints() {
    let root = support::fixture();
    let original = b"# existing workspace constraint\nversion = 4\n";
    std::fs::write(root.path().join("Cargo.lock"), original).unwrap();
    let policy = LoadPolicy::repository_only();
    let mut config = policy.cargo_config().unwrap();
    let manifest = ProjectManifest::from_manifest_file(
        absolute_utf8(&root.path().join("macros/Cargo.toml")).unwrap(),
    )
    .unwrap();
    let workspace = load_complete(manifest, &config, &policy).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    redirect_project_lockfile(&mut config, &workspace, temporary.path()).unwrap();
    assert_eq!(
        std::fs::read(temporary.path().join("Cargo.lock")).unwrap(),
        original
    );
    assert_eq!(
        std::fs::read(root.path().join("Cargo.lock")).unwrap(),
        original
    );
    assert!(!root.path().join("macros/Cargo.lock").exists());
}

/// 用实际旧 Cargo 覆盖三条只读锁分支；准备的锁只属于临时夹具，不改写真实项目。
#[test]
#[ignore = "需要 NARRANT_TEST_CARGO_TOOLCHAINS 指定已安装的 1.81/1.94/1.96 工具链目录"]
fn real_old_cargo_versions_preserve_valid_locks_and_reject_only_required_writes() {
    let toolchains = std::env::var_os("NARRANT_TEST_CARGO_TOOLCHAINS").expect("实际旧工具链路径");
    for toolchain in std::env::split_paths(&toolchains) {
        for (mode, member) in [
            ("valid", false),
            ("missing", false),
            ("stale", false),
            ("valid", true),
            ("missing", true),
            ("stale", true),
        ] {
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir(root.path().join("dep")).unwrap();
            std::fs::write(root.path().join("Cargo.toml"),"[package]\nname=\"lock_probe\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\npath=\"lib.rs\"\n[dependencies]\ndep={path=\"dep\"}\n[workspace]\nmembers=[\"dep\"]\n").unwrap();
            std::fs::write(
                root.path().join("lib.rs"),
                "pub fn run() { dep::target(); }",
            )
            .unwrap();
            std::fs::write(root.path().join("dep/lib.rs"), "pub fn target() {}").unwrap();
            std::fs::write(root.path().join("dep/Cargo.toml"),format!("[package]\nname=\"dep\"\nversion=\"{}\"\nedition=\"2021\"\n[lib]\npath=\"lib.rs\"\n",if mode=="stale" {"0.2.0"} else {"0.1.0"})).unwrap();
            if mode != "missing" {
                std::fs::write(root.path().join("Cargo.lock"),"version = 3\n[[package]]\nname = \"dep\"\nversion = \"0.1.0\"\n[[package]]\nname = \"lock_probe\"\nversion = \"0.1.0\"\ndependencies = [\"dep\"]\n").unwrap();
            }
            let before = support::snapshot(root.path());
            let policy = LoadPolicy::repository_only();
            let mut config = policy.cargo_config().unwrap();
            config.extra_env.insert(
                "RUSTUP_TOOLCHAIN".into(),
                Some(toolchain.to_str().unwrap().into()),
            );
            let manifest = project_model::ProjectManifest::from_manifest_file(
                super::absolute_utf8(&root.path().join(if member {
                    "dep/Cargo.toml"
                } else {
                    "Cargo.toml"
                }))
                .unwrap(),
            )
            .unwrap();
            let discovery = super::load_complete(manifest.clone(), &config, &policy).unwrap();
            let version = discovery.toolchain.as_ref().unwrap().clone();
            let locks = tempfile::tempdir().unwrap();
            super::redirect_project_lockfile(&mut config, &discovery, locks.path()).unwrap();
            config.no_deps = false;
            let result = if (82..95).contains(&version.minor) {
                crate::readonly_metadata::complete(discovery, &config)
            } else {
                super::load_complete(manifest, &config, &policy)
            };
            eprintln!(
                "真实 Cargo {version} / {mode} / member={member}: {}",
                if result.is_ok() { "ok" } else { "error" }
            );
            if version.minor < 82 && mode != "valid" {
                assert!(format!("{:#}", result.unwrap_err()).contains("只读分析不能更新"));
            } else {
                let workspace = result.unwrap();
                let ProjectWorkspaceKind::Cargo { cargo, .. } = workspace.kind else {
                    panic!("Cargo 模型丢失")
                };
                let package = cargo
                    .packages()
                    .find(|&package| cargo[package].name == "lock_probe")
                    .unwrap();
                assert_eq!(
                    cargo[package].dependencies.len(),
                    1,
                    "必须保留真实解析的依赖边"
                );
                assert_eq!(
                    cargo.workspace_root().as_str(),
                    root.path().canonicalize().unwrap().to_str().unwrap()
                );
            }
            support::assert_unchanged(root.path(), &before);
        }
    }
}

/// 旧版补偿 metadata 保留显式 features 与 Cargo 合并 target 配置，包括 JSON target 的来源目录。
#[test]
#[ignore = "需要 NARRANT_TEST_CARGO_194 指定实际 1.94.0 工具链目录"]
fn old_cargo_metadata_preserves_features_and_effective_targets() {
    let toolchain = std::env::var("NARRANT_TEST_CARGO_194").expect("实际1.94工具链");
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".cargo")).unwrap();
    std::fs::write(
        root.path().join(".cargo/config.toml"),
        "[build]\ntarget=\"x86_64-unknown-linux-gnu\"\n",
    )
    .unwrap();
    std::fs::write(root.path().join("Cargo.toml"),"[package]\nname=\"target_probe\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\npath=\"lib.rs\"\n[dependencies]\noptional_dep={path=\"optional_dep\",optional=true}\n[features]\nselected=[\"dep:optional_dep\"]\n[target.'cfg(target_os = \"windows\")'.dependencies]\nwindows_dep={path=\"windows_dep\"}\n[workspace]\nmembers=[\"optional_dep\",\"windows_dep\"]\n").unwrap();
    std::fs::write(root.path().join("lib.rs"), "pub fn root() {}").unwrap();
    for name in ["optional_dep", "windows_dep"] {
        std::fs::create_dir(root.path().join(name)).unwrap();
        std::fs::write(root.path().join(name).join("Cargo.toml"),format!("[package]\nname=\"{name}\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\npath=\"lib.rs\"\n")).unwrap();
        std::fs::write(root.path().join(name).join("lib.rs"), "pub fn target() {}").unwrap();
    }
    for selected in [false, true] {
        let before = support::snapshot(root.path());
        let policy = LoadPolicy::repository_only();
        let mut config = policy.cargo_config().unwrap();
        config
            .extra_env
            .insert("RUSTUP_TOOLCHAIN".into(), Some(toolchain.clone()));
        config.features = project_model::CargoFeatures::Selected {
            features: if selected {
                vec!["selected".into()]
            } else {
                vec![]
            },
            no_default_features: true,
        };
        let manifest = ProjectManifest::from_manifest_file(
            absolute_utf8(&root.path().join("Cargo.toml")).unwrap(),
        )
        .unwrap();
        let workspace = load_complete(manifest, &config, &policy).unwrap();
        let locks = tempfile::tempdir().unwrap();
        redirect_project_lockfile(&mut config, &workspace, locks.path()).unwrap();
        config.no_deps = false;
        let workspace = crate::readonly_metadata::complete(workspace, &config).unwrap();
        let ProjectWorkspaceKind::Cargo { cargo, .. } = workspace.kind else {
            panic!("Cargo 模型丢失")
        };
        let package = cargo
            .packages()
            .find(|&package| cargo[package].name == "target_probe")
            .unwrap();
        let dependencies = &cargo[package].dependencies;
        assert_eq!(
            dependencies.len(),
            usize::from(selected),
            "feature/target 过滤不能丢失"
        );
        if selected {
            assert_eq!(cargo[dependencies[0].pkg].name, "optional_dep");
        }
        support::assert_unchanged(root.path(), &before);
    }
}
