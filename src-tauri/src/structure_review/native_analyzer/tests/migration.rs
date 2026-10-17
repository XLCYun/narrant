use super::*;

/// 测试复用公共 Host 夹具，只替换固定语言身份与对应标准声明。
fn migrating(fixture: &Fixture, profile: NativeProfile) -> NativeLanguageAnalyzer {
    let mut analyzer = fixture.analyzer();
    analyzer.identity.profile = profile.as_str().into();
    analyzer.identity.languages = match profile {
        NativeProfile::Python => vec!["python".into()],
        NativeProfile::TypeScriptJavascript => vec!["typescript".into(), "javascript".into()],
        _ => vec!["rust".into()],
    };
    analyzer.platforms.clear();
    if profile == NativeProfile::Python {
        analyzer.platforms.push(Platform {
            id: "stdlib".into(),
            kind: "python-embedded-stdlib".into(),
            version: "0.0.84".into(),
            root: fixture.tools.clone(),
            metadata: json!({"source_revision":"11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a"}),
        });
    }
    analyzer
}

/// 三档 Rust 的空平台约束不同，消息保持 /1 的原字段，空选集和测试开关不丢失。
#[test]
fn rust_profiles_preserve_wire_contract_and_require_only_needed_declarations() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.files.clear();
    input.options.include_tests = false;
    for profile in [
        NativeProfile::RustRepository,
        NativeProfile::RustStandardLibrary,
        NativeProfile::RustAllDependencies,
    ] {
        let mut analyzer = migrating(&fixture, profile);
        if profile != NativeProfile::RustRepository {
            assert!(input::validate(&analyzer, &input).is_err());
            for entry in ["core/src/lib.rs", "std/src/lib.rs"] {
                let file = fixture
                    .platform
                    .join("lib/rustlib/src/rust/library")
                    .join(entry);
                fs::create_dir_all(file.parent().unwrap()).unwrap();
                fs::write(file, "").unwrap();
            }
            analyzer.platforms.push(Platform {
                id: "rust".into(),
                kind: "rust-standard-library".into(),
                version: "1.98.0".into(),
                root: fixture.platform.clone(),
                metadata: json!({"rust_src":"lib/rustlib/src/rust/library"}),
            });
        }
        let checked = input::validate(&analyzer, &input).unwrap();
        let request = checked.request(&analyzer, &fixture.tools).unwrap();
        assert_eq!(request["protocol"], "narrant.native-analyzer/1");
        assert_eq!(
            request["input"],
            json!({"repository_root":fixture.repository.canonicalize().unwrap(), "files":[], "include_tests":false, "profile":profile.as_str()})
        );
        analyzer
            .platforms
            .push(fixture.analyzer().platforms.remove(0));
        assert!(input::validate(&analyzer, &input).is_err());
    }
}

/// 同一物理目录的两个 Python 词法身份共存，但现有四语言仍拒绝别名，逃逸始终失败。
#[test]
fn python_lexical_aliases_keep_identity_without_weakening_escape_checks() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.repository.join("impl")).unwrap();
    fs::write(fixture.repository.join("impl/__init__.py"), "import impl\n").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        fixture.repository.join("impl"),
        fixture.repository.join("alias"),
    )
    .unwrap();
    #[cfg(windows)]
    assert!(std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(fixture.repository.join("alias"))
        .arg(fixture.repository.join("impl"))
        .status()
        .unwrap()
        .success());
    let mut input = fixture.input();
    input.files = vec!["alias/__init__.py".into(), "impl/__init__.py".into()];
    let analyzer = migrating(&fixture, NativeProfile::Python);
    let checked = input::validate(&analyzer, &input).unwrap();
    assert_eq!(
        checked.files,
        [
            "alias/__init__.py".to_owned(),
            "impl/__init__.py".to_owned()
        ]
        .into()
    );
    assert!(input::validate(&fixture.analyzer(), &input).is_err());
    input.files.push("alias/__init__.py".into());
    assert!(input::validate(&analyzer, &input).is_err());
    #[cfg(unix)]
    std::os::unix::fs::symlink(&fixture.tools, fixture.repository.join("outside")).unwrap();
    #[cfg(windows)]
    assert!(std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(fixture.repository.join("outside"))
        .arg(&fixture.tools)
        .status()
        .unwrap()
        .success());
    input.files = vec!["outside/executable".into()];
    assert!(input::validate(&analyzer, &input).is_err());
}

/// Python 的 stdlib 是内嵌虚拟来源，不要求用户安装 typeshed；来源必须关联 helper 安装根。
#[test]
fn embedded_python_stdlib_is_strict_and_tied_to_the_helper() {
    let fixture = Fixture::new();
    let input = fixture.input();
    let mut analyzer = migrating(&fixture, NativeProfile::Python);
    assert!(input::validate(&analyzer, &input).is_ok());
    analyzer.platforms[0].root = fixture.platform.clone();
    assert!(input::validate(&analyzer, &input).is_err());
    analyzer.platforms[0].root = fixture.tools.clone();
    analyzer.platforms[0].metadata["python_path"] = json!("/user/venv");
    assert!(input::validate(&analyzer, &input).is_err());
}

/// TS 保留现有三套声明；未知包、额外 metadata 和丢失文件均不能进入分析。
#[test]
fn typescript_requires_approved_standard_declaration_packages() {
    let fixture = Fixture::new();
    let mut input = fixture.input();
    input.files = vec!["main.ts".into(), "main.js".into()];
    for file in &input.files {
        fs::write(fixture.repository.join(file), "").unwrap();
    }
    let mut analyzer = migrating(&fixture, NativeProfile::TypeScriptJavascript);
    for (index, package) in ["typescript", "@types/node", "undici-types"]
        .into_iter()
        .enumerate()
    {
        let root = fixture.platform.join(index.to_string());
        fs::create_dir(&root).unwrap();
        fs::write(
            root.join(if index == 0 { "lib.d.ts" } else { "index.d.ts" }),
            "",
        )
        .unwrap();
        analyzer.platforms.push(Platform {
            id: index.to_string(),
            kind: "typescript-declarations".into(),
            version: "1.0.0".into(),
            root,
            metadata: json!({"package":package}),
        });
    }
    let checked = input::validate(&analyzer, &input).unwrap();
    let mut result = result();
    let mut ts = node("typescript:Run");
    ts["language"] = json!("typescript");
    ts["references"] =
        json!([{"semantic":"code.calls","target":{"kind":"node","id":"javascript:Run"}}]);
    let mut js = node("javascript:Run");
    js["language"] = json!("javascript");
    result["graph"]["nodes"] = json!([ts, js]);
    result["graph"]["root_contains"] = json!(["typescript:Run", "javascript:Run"]);
    result["evidence"]["configurations"][0]["platform_ids"] = json!(["0", "1", "2"]);
    result["evidence"]["files"][0]["path"] = json!("main.ts");
    result["evidence"]["files"][1]["path"] = json!("main.js");
    result["evidence"]["origins"] = json!([
        {"node_id":"typescript:Run","selected_paths":["main.ts"]},
        {"node_id":"javascript:Run","selected_paths":["main.js"]}
    ]);
    let protocol::Message::Ok(completed) =
        protocol::decode(&serde_json::to_vec(&result).unwrap()).unwrap()
    else {
        panic!("成功结果")
    };
    let graph = graph::build(completed, &checked, &analyzer.identity).unwrap();
    assert!(graph.node(&NodeId("javascript:Run".into())).is_some());
    assert_eq!(
        graph
            .node(&NodeId("typescript:Run".into()))
            .unwrap()
            .data()
            .references[0]
            .target,
        ReferenceTarget::Node(NodeId("javascript:Run".into()))
    );
    analyzer.platforms.pop();
    assert!(input::validate(&analyzer, &input).is_err());
    analyzer.platforms[0].metadata["package"] = json!("project-module");
    assert!(input::validate(&analyzer, &input).is_err());
}

/// 工具链发现保留用户覆盖，只有全部依赖允许沿用联网政策，运行时不猜测覆盖来源。
#[test]
fn rust_environment_preserves_network_policy_and_blocks_automatic_installation() {
    for profile in [
        NativeProfile::RustRepository,
        NativeProfile::RustStandardLibrary,
        NativeProfile::RustAllDependencies,
    ] {
        let env = profiles::rust_environment(profile, |key| {
            Some(
                if key == "CARGO_NET_OFFLINE" {
                    "false"
                } else {
                    "fixture"
                }
                .into(),
            )
        });
        assert_eq!(env[std::ffi::OsStr::new("PATH")], "fixture");
        assert_eq!(env[std::ffi::OsStr::new("CARGO_HOME")], "fixture");
        assert_eq!(env[std::ffi::OsStr::new("RUSTUP_TOOLCHAIN")], "fixture");
        assert_eq!(env[std::ffi::OsStr::new("HTTPS_PROXY")], "fixture");
        assert_eq!(
            env[std::ffi::OsStr::new("CARGO_NET_OFFLINE")],
            if profile == NativeProfile::RustAllDependencies {
                "false"
            } else {
                "true"
            }
        );
        assert_eq!(env[std::ffi::OsStr::new("RUSTUP_AUTO_INSTALL")], "0");
        for key in ["RUSTC_WRAPPER", "RUSTFLAGS", "NODE_OPTIONS", "PYTHONPATH"] {
            assert!(!env.contains_key(std::ffi::OsStr::new(key)));
        }
    }
}

/// 构造器与原有项目探针读取同一环境覆盖；不存在覆盖时也不能注入 helper 构建版本。
#[test]
fn rust_constructor_preserves_inherited_toolchain_and_discovery_path() {
    let fixture = Fixture::new();
    let prepared = migrating(&fixture, NativeProfile::RustAllDependencies);
    let analyzer =
        NativeLanguageAnalyzer::new(prepared.command, prepared.identity, prepared.platforms);
    for key in ["PATH", "RUSTUP_TOOLCHAIN", "CARGO_HOME", "RUSTUP_HOME"] {
        assert_eq!(
            analyzer
                .command
                .environment
                .get(std::ffi::OsStr::new(key))
                .cloned(),
            std::env::var_os(key)
        );
    }
    // 使用确定的输入覆盖有/无用户 override 两种情况，不修改测试进程的全局环境。
    for project_toolchain in [None, Some("user-selected-project-toolchain")] {
        let environment = profiles::rust_environment(NativeProfile::RustAllDependencies, |key| {
            (key == "RUSTUP_TOOLCHAIN")
                .then_some(project_toolchain)
                .flatten()
                .map(Into::into)
        });
        assert_eq!(
            environment
                .get(std::ffi::OsStr::new("RUSTUP_TOOLCHAIN"))
                .cloned(),
            project_toolchain.map(Into::into)
        );
    }
}

/// 工厂确认的项目 PATH/工具链及联网政策优先，禁止自动安装和只读约束不能被覆盖。
#[test]
fn rust_constructor_keeps_factory_resolution_and_forces_safety_policy() {
    let fixture = Fixture::new();
    for profile in [
        NativeProfile::RustRepository,
        NativeProfile::RustStandardLibrary,
        NativeProfile::RustAllDependencies,
    ] {
        let mut prepared = migrating(&fixture, profile);
        let resolved_path = std::env::join_paths([
            fixture.tools.join("project-toolchain/bin"),
            fixture.tools.join("cargo/bin"),
        ])
        .unwrap();
        prepared.command.environment = BTreeMap::from([
            ("PATH".into(), resolved_path.clone()),
            (
                "RUSTUP_TOOLCHAIN".into(),
                "factory-approved-project-toolchain".into(),
            ),
            ("CARGO_NET_OFFLINE".into(), "false".into()),
            ("RUSTUP_AUTO_INSTALL".into(), "1".into()),
            ("CARGO_CACHE_RUSTC_INFO".into(), "1".into()),
        ]);
        let analyzer =
            NativeLanguageAnalyzer::new(prepared.command, prepared.identity, prepared.platforms);
        let environment = &analyzer.command.environment;
        assert_eq!(environment[std::ffi::OsStr::new("PATH")], resolved_path);
        assert_eq!(
            environment[std::ffi::OsStr::new("RUSTUP_TOOLCHAIN")],
            "factory-approved-project-toolchain"
        );
        assert_eq!(
            environment[std::ffi::OsStr::new("RUSTUP_AUTO_INSTALL")],
            "0"
        );
        assert_eq!(
            environment[std::ffi::OsStr::new("CARGO_CACHE_RUSTC_INFO")],
            "0"
        );
        assert_eq!(
            environment[std::ffi::OsStr::new("CARGO_NET_OFFLINE")],
            if profile == NativeProfile::RustAllDependencies {
                "false"
            } else {
                "true"
            }
        );
    }
}

/// Rust 三档允许 canonical 别名交给 helper 合并；重复词法路径仍由 Host 拒绝。
#[test]
fn rust_profiles_admit_physical_aliases_without_loosening_other_languages() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.repository.join("src")).unwrap();
    fs::write(fixture.repository.join("src/lib.rs"), "pub fn run() {}").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(
        fixture.repository.join("src"),
        fixture.repository.join("alias"),
    )
    .unwrap();
    #[cfg(windows)]
    assert!(std::process::Command::new("cmd")
        .args(["/c", "mklink", "/J"])
        .arg(fixture.repository.join("alias"))
        .arg(fixture.repository.join("src"))
        .status()
        .unwrap()
        .success());
    let mut input = fixture.input();
    input.files = vec!["src/lib.rs".into(), "alias/lib.rs".into()];
    for profile in [
        NativeProfile::RustRepository,
        NativeProfile::RustStandardLibrary,
        NativeProfile::RustAllDependencies,
    ] {
        let mut analyzer = migrating(&fixture, profile);
        if profile != NativeProfile::RustRepository {
            for name in ["core/src/lib.rs", "std/src/lib.rs"] {
                let file = fixture
                    .platform
                    .join("lib/rustlib/src/rust/library")
                    .join(name);
                fs::create_dir_all(file.parent().unwrap()).unwrap();
                fs::write(file, "").unwrap();
            }
            analyzer.platforms.push(Platform {
                id: "project-rust".into(),
                kind: "rust-standard-library".into(),
                version: "test".into(),
                root: fixture.platform.clone(),
                metadata: json!({"rust_src":"lib/rustlib/src/rust/library"}),
            });
        }
        assert_eq!(input::validate(&analyzer, &input).unwrap().files.len(), 2);
        let mut duplicate = input.clone();
        duplicate.files.push("src/lib.rs".into());
        assert!(input::validate(&analyzer, &duplicate).is_err());
    }
    assert!(input::validate(&fixture.analyzer(), &input).is_err());
}

/// 工厂按三档重新配置时只使用被冻结的环境，系统加载变量与联网政策不丢失。
#[test]
fn rust_project_configuration_keeps_frozen_runtime_environment() {
    use crate::structure_review::code_graph::RustDependencyScope;
    let fixture = Fixture::new();
    for (scope, profile) in [
        (
            RustDependencyScope::RepositoryOnly,
            NativeProfile::RustRepository,
        ),
        (
            RustDependencyScope::StandardLibrary,
            NativeProfile::RustStandardLibrary,
        ),
        (
            RustDependencyScope::AllDependencies,
            NativeProfile::RustAllDependencies,
        ),
    ] {
        let snapshot = BTreeMap::from([
            ("PATH".into(), "project-tools".into()),
            ("RUSTUP_TOOLCHAIN".into(), "project-version".into()),
            ("CARGO_NET_OFFLINE".into(), "false".into()),
            ("RUSTC_WRAPPER".into(), "rejected-wrapper".into()),
            ("SystemRoot".into(), "frozen-system-root".into()),
        ]);
        let analyzer = migrating(&fixture, NativeProfile::RustRepository).with_rust_project(
            scope,
            snapshot,
            vec![],
        );
        let env = &analyzer.command.environment;
        assert_eq!(analyzer.identity.profile, profile.as_str());
        assert_eq!(env[std::ffi::OsStr::new("PATH")], "project-tools");
        assert_eq!(
            env[std::ffi::OsStr::new("RUSTUP_TOOLCHAIN")],
            "project-version"
        );
        assert_eq!(
            env[std::ffi::OsStr::new("CARGO_NET_OFFLINE")],
            if scope == RustDependencyScope::AllDependencies {
                "false"
            } else {
                "true"
            }
        );
        assert!(!env.contains_key(std::ffi::OsStr::new("RUSTC_WRAPPER")));
        assert_eq!(
            env.get(std::ffi::OsStr::new("SystemRoot"))
                .map(|value| value.to_str().unwrap()),
            if cfg!(windows) {
                Some("frozen-system-root")
            } else {
                None
            }
        );
    }
}
