use super::*;

/// 所有新扩展均进入明确支持组，头文件只进入一个 C/C++ helper 请求。
#[test]
fn recognizes_native_languages_and_headers() {
    let reference = crate::review::FileContentRef {
        object_id: "fixture".into(),
        mode: 0o100644,
    };
    for (extension, id) in [
        ("java", PackId::Java),
        ("go", PackId::Go),
        ("cs", PackId::Csharp),
        ("c", PackId::CCpp),
        ("h", PackId::CCpp),
        ("cpp", PackId::CCpp),
        ("hpp", PackId::CCpp),
        ("C", PackId::CCpp),
    ] {
        let path = format!("source.{extension}");
        assert_eq!(
            language_group(&path, Some(&reference)),
            Some(LanguageGroup::Native(id))
        );
        assert_eq!(coverage_kind(&path, Some(&reference)), "analyzed");
    }
}

/// 原生语言分组在同侧各返回完整图，并与既有语言共享原始仓库目录。
#[test]
fn native_groups_are_merged_once_each() {
    let repository = tempfile::tempdir().unwrap();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let mut application = StructureReview::new(Arc::new(|_, _, _| panic!("不应调用 Rust")));
    let mut files = SideFiles::default();
    for (id, language, paths) in [
        (PackId::Go, "go", vec!["main.go"]),
        (PackId::Java, "java", vec!["Main.java"]),
        (PackId::Csharp, "csharp", vec!["Main.cs"]),
        (PackId::CCpp, "cpp", vec!["main.cpp", "shared.h"]),
    ] {
        let observed = observed.clone();
        application.analyzers.native.insert(
            id,
            Arc::new(move |input, _, scope| {
                assert_eq!(scope, RustDependencyScope::StandardLibrary);
                observed.lock().unwrap().push((
                    id,
                    input.repository_root.clone(),
                    input.files.clone(),
                ));
                Ok(single_node_graph(
                    &format!("{language}:main"),
                    language,
                    vec![],
                ))
            }),
        );
        for path in paths {
            files.push(Some(LanguageGroup::Native(id)), path.into());
        }
    }
    let cancelled: CancellationCheck = Arc::new(|| false);
    let graph = analyze_side(
        &application.analyzers,
        repository.path(),
        files,
        AnalysisOptions::default(),
        &cancelled,
        RustDependencyScope::StandardLibrary,
        &CancellationToken::new(),
    )
    .unwrap();
    for language in ["go", "java", "csharp", "cpp"] {
        assert!(graph.node(&NodeId(format!("{language}:main"))).is_some());
    }
    let observed = observed.lock().unwrap();
    assert_eq!(observed.len(), 4);
    assert!(observed
        .iter()
        .all(|(_, root, _)| root == repository.path()));
    assert_eq!(
        observed
            .iter()
            .find(|(id, _, _)| *id == PackId::CCpp)
            .unwrap()
            .2
            .len(),
        2
    );
}

/// 缺失原生包时保留能力错误，绝不能静默返回一个空图。
#[test]
fn missing_native_analyzer_fails_explicitly() {
    let repository = tempfile::tempdir().unwrap();
    let application = StructureReview::new(Arc::new(|_, _, _| panic!("不应调用 Rust")));
    let mut files = SideFiles::default();
    files.push(
        Some(LanguageGroup::Native(PackId::Java)),
        "Main.java".into(),
    );
    let cancelled: CancellationCheck = Arc::new(|| false);
    let error = analyze_side(
        &application.analyzers,
        repository.path(),
        files,
        AnalysisOptions::default(),
        &cancelled,
        RustDependencyScope::StandardLibrary,
        &CancellationToken::new(),
    )
    .unwrap_err();
    assert_eq!(error.code, "language-pack-unavailable");
}

/// 原生组失败会取消同侧其他 helper，并等待其完成回收后才返回根因。
#[test]
fn native_failure_cancels_and_joins_other_language_workers() {
    let repository = tempfile::tempdir().unwrap();
    let stop = CancellationToken::new();
    let signal = stop.clone();
    let cancelled: CancellationCheck = Arc::new(move || signal.is_cancelled());
    let started = Arc::new(std::sync::Barrier::new(2));
    let reaped = Arc::new(AtomicBool::new(false));
    let mut application = StructureReview::new(Arc::new(|_, _, _| panic!("不应调用 Rust")));
    let (go_started, go_reaped) = (started.clone(), reaped.clone());
    application.analyzers.native.insert(
        PackId::Go,
        Arc::new(move |_, cancelled, _| {
            go_started.wait();
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !cancelled() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            go_reaped.store(cancelled(), Ordering::Release);
            Err(AnalysisError::cancelled("Go helper 已回收"))
        }),
    );
    application.analyzers.native.insert(
        PackId::Java,
        Arc::new(move |_, _, _| {
            started.wait();
            Err(AnalysisError::new("Java 原始失败"))
        }),
    );
    let mut files = SideFiles::default();
    files.push(Some(LanguageGroup::Native(PackId::Go)), "main.go".into());
    files.push(
        Some(LanguageGroup::Native(PackId::Java)),
        "Main.java".into(),
    );
    let error = analyze_side(
        &application.analyzers,
        repository.path(),
        files,
        AnalysisOptions::default(),
        &cancelled,
        RustDependencyScope::StandardLibrary,
        &stop,
    )
    .unwrap_err();
    assert_eq!(error.detail, "Java 原始失败");
    assert!(reaped.load(Ordering::Acquire));
}

/// 预留的 Native 调用通道逐档传递 Rust scope 与测试过滤，不提前注册生产 Rust 工厂。
#[test]
fn native_callback_preserves_each_rust_scope_and_test_option() {
    for scope in [
        RustDependencyScope::RepositoryOnly,
        RustDependencyScope::StandardLibrary,
        RustDependencyScope::AllDependencies,
    ] {
        let repository = tempfile::tempdir().unwrap();
        let mut application = StructureReview::new(Arc::new(|_, _, _| panic!("旧入口不应执行")));
        application.analyzers.native.insert(
            PackId::Rust,
            Arc::new(move |input, _, actual| {
                assert_eq!(actual, scope);
                assert!(!input.options.include_tests);
                assert_eq!(input.files, vec![PathBuf::from("lib.rs")]);
                Ok(single_node_graph("rust:main", "rust", vec![]))
            }),
        );
        let mut files = SideFiles::default();
        files.push(Some(LanguageGroup::Native(PackId::Rust)), "lib.rs".into());
        let cancelled: CancellationCheck = Arc::new(|| false);
        let graph = analyze_side(
            &application.analyzers,
            repository.path(),
            files,
            AnalysisOptions {
                include_tests: false,
            },
            &cancelled,
            scope,
            &CancellationToken::new(),
        )
        .unwrap();
        assert!(graph.node(&NodeId("rust:main".into())).is_some());
    }
}
