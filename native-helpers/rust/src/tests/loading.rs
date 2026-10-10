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
    assert!(error.to_string().contains("缺少 rust-src"));
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
