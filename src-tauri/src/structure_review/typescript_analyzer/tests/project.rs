use super::*;

/// 只构造读取源码所需的真实目录和内存上下文，不启动 Node 或 TypeScript 服务。
fn source_project() -> PreparedProject {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let workspace_root = root.join("workspace");
    let support_root = root.join("support");
    let runtime_root = root.join("runtime");
    let library_root = runtime_root.join("typescript/lib");
    for path in [&workspace_root, &support_root, &library_root] {
        std::fs::create_dir_all(path).unwrap();
    }
    let path = workspace_root.join("main.ts");
    let source = SourceText::new("export const value = '固定😀';", LineBreakMode::TypeScript);
    std::fs::write(&path, source.as_str()).unwrap();
    std::fs::write(
        library_root.join("lib.fixture.d.ts"),
        "declare const value: '运行时😀';\u{2029}declare function run(): void;\r\n",
    )
    .unwrap();
    PreparedProject {
        _directory: directory,
        workspace_root: workspace_root.clone(),
        support_root,
        toolchain: TypeScriptToolchain {
            node: root.join("unused-node"),
            runtime_root,
        },
        config_files: Vec::new(),
        registrations: Vec::new(),
        files: Vec::new(),
        allowed_roots: vec![workspace_root, library_root],
        sources: BTreeMap::from([(path.clone(), SourceSnapshot { path, source })]),
        metadata: BTreeSet::new(),
        loaded_sources: RefCell::new(BTreeMap::new()),
    }
}

/// 第一方源码沿用准备时快照，运行时声明首次读取后也固定文本并共享同一分配。
#[test]
fn source_snapshots_and_lazy_runtime_reads_share_cached_text() {
    let project = source_project();
    let file = project.workspace_root.join("main.ts");
    let snapshot = &project.sources[&file].source;
    std::fs::write(&file, "export const replaced = true;").unwrap();
    let first = project.source(&file).unwrap();
    let second = project.source(&file).unwrap();
    assert_eq!(first.as_str(), "export const value = '固定😀';");
    assert_eq!(first.as_str().as_ptr(), snapshot.as_str().as_ptr());
    assert_eq!(second.as_str().as_ptr(), first.as_str().as_ptr());

    let library = project
        .toolchain
        .runtime_root
        .join("typescript/lib/lib.fixture.d.ts");
    let first = project.source(&library).unwrap();
    std::fs::write(&library, "declare const replaced: boolean;").unwrap();
    let second = project.source(&library).unwrap();
    assert_eq!(
        first.as_str(),
        "declare const value: '运行时😀';\u{2029}declare function run(): void;\r\n"
    );
    assert_eq!(second.as_str(), first.as_str());
    assert_eq!(second.as_str().as_ptr(), first.as_str().as_ptr());
    assert_eq!(
        second
            .position(second.find("declare function").unwrap())
            .unwrap(),
        crate::lsp::Position {
            line: 1,
            character: 0,
        }
    );
}

/// 缓存命中前仍拒绝未授权源码和父目录片段，即使后者最终指向已缓存声明。
#[test]
fn source_cache_preserves_authorization_and_lexical_path_checks() {
    let project = source_project();
    let library = project
        .toolchain
        .runtime_root
        .join("typescript/lib/lib.fixture.d.ts");
    project.source(&library).unwrap();
    let outside = project
        .workspace_root
        .parent()
        .unwrap()
        .join("outside.d.ts");
    let untracked = project.workspace_root.join("untracked.ts");
    for path in [&outside, &untracked] {
        std::fs::write(path, "declare const unauthorized: boolean;").unwrap();
    }
    let traversal = project
        .workspace_root
        .join("../runtime/typescript/lib/lib.fixture.d.ts");
    for path in [&outside, &untracked, &traversal] {
        assert!(project.checked_path(path).is_err(), "{}", path.display());
        assert!(project.source(path).is_err(), "{}", path.display());
    }
}

/// 已缓存文件被替换成根外链接后，第一方和运行时读取都重新校验真实路径。
#[cfg(unix)]
#[test]
fn source_cache_rechecks_symlink_targets_after_warmup() {
    let project = source_project();
    let outside = project
        .workspace_root
        .parent()
        .unwrap()
        .join("outside.d.ts");
    std::fs::write(&outside, "declare const unauthorized: boolean;").unwrap();
    for file in [
        project.workspace_root.join("main.ts"),
        project
            .toolchain
            .runtime_root
            .join("typescript/lib/lib.fixture.d.ts"),
    ] {
        project.source(&file).unwrap();
        std::fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink(&outside, &file).unwrap();
        assert!(project.checked_path(&file).is_err(), "{}", file.display());
        assert!(project.source(&file).is_err(), "{}", file.display());
    }
}

/// 空选集在工具未安装时仍可完成，不做多余启动。
#[test]
fn empty_selection_is_lazy() {
    let tools = TypeScriptToolchain {
        node: PathBuf::from("/missing"),
        runtime_root: PathBuf::from("/missing"),
    };
    assert!(PreparedProject::prepare(
        Path::new("/missing-repo"),
        &[],
        &tools,
        Duration::from_secs(1),
        &(std::sync::Arc::new(|| false) as CancellationCheck)
    )
    .unwrap()
    .is_none());
}

/// 助手协议拒绝缺字段、错误类型和未知字段；解析错误不会产生一个空 PreparedProject。
#[test]
fn malformed_helper_output_is_rejected() {
    for output in [
        json!({}),
        json!({"projects":[],"associations":[],"sources":[],"metadata":[],"config_files":[],"surprise":true}),
        json!({"projects":{},"associations":[],"sources":[],"metadata":[],"config_files":[]}),
        json!({"projects":[],"associations":[],"sources":[],"metadata":[],"config_files":[],"notes":[]}),
    ] {
        assert!(serde_json::from_value::<PreparedOutput>(output).is_err());
    }
}

/// 完整助手结果不再要求分析说明字段，空项目结果也能按协议解析。
#[test]
fn helper_output_without_notes_is_accepted() {
    let output =
        json!({"projects":[],"associations":[],"sources":[],"metadata":[],"config_files":[]});
    assert!(serde_json::from_value::<PreparedOutput>(output).is_ok());
}

/// 输入路径在任何工具启动前拒绝父目录跳转和根外符号链接。
#[test]
fn selected_paths_are_validated_before_tools() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let tools = TypeScriptToolchain {
        node: PathBuf::from("/missing"),
        runtime_root: PathBuf::from("/missing"),
    };
    let check: CancellationCheck = std::sync::Arc::new(|| false);
    let error = PreparedProject::prepare(
        &root,
        &[root.join("../outside.ts")],
        &tools,
        Duration::from_secs(1),
        &check,
    )
    .unwrap_err();
    assert!(error.to_string().contains("越界"));
}
