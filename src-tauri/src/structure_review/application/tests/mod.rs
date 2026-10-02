use super::*;
use crate::{
    persistence::ReviewStore,
    repository::verify_repository_location,
    review::{PresentReview, ReviewPresentation},
    structure_review::code_graph::{
        CodeNode, CodeNodeData, CodeReference, LanguageId, NodeId, ReferenceTarget,
    },
};
use std::{
    path::Path,
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    time::Duration,
};

/// 在真实 Git 仓库和 Review 生命周期上测试应用 Interface，语言提取使用受控 Adapter。
struct Fixture {
    _data: tempfile::TempDir,
    repository: tempfile::TempDir,
    lifecycle: Arc<ReviewLifecycle>,
    presentation: ReviewPresentation,
    head: PathBuf,
}

impl Fixture {
    /// base 分支在分叉后继续前进，确保 before 使用 merge-base 而非 base tip。
    async fn new() -> Self {
        let data = tempfile::tempdir().unwrap();
        let repository = tempfile::tempdir().unwrap();
        git(repository.path(), &["init", "-q"]);
        git(
            repository.path(),
            &["config", "user.email", "test@narrant.invalid"],
        );
        git(repository.path(), &["config", "user.name", "Narrant Test"]);
        std::fs::write(
            repository.path().join("lib.rs"),
            "mod removed; fn value() -> u8 { 1 }",
        )
        .unwrap();
        std::fs::write(repository.path().join("removed.rs"), "fn removed() {}").unwrap();
        std::fs::write(repository.path().join("README.md"), "before").unwrap();
        std::fs::write(
            repository.path().join("Cargo.toml"),
            r#"[package]
name = "structure_review_fixture"
version = "0.1.0"
edition = "2021"
[lib]
path = "lib.rs"
"#,
        )
        .unwrap();
        git(repository.path(), &["add", "."]);
        git(repository.path(), &["commit", "-qm", "common"]);
        git(repository.path(), &["branch", "base"]);
        std::fs::write(
            repository.path().join("lib.rs"),
            "mod added; fn value() -> u8 { 2 }",
        )
        .unwrap();
        std::fs::write(repository.path().join("added.rs"), "fn added() {}").unwrap();
        std::fs::remove_file(repository.path().join("removed.rs")).unwrap();
        std::fs::write(repository.path().join("README.md"), "after").unwrap();
        git(repository.path(), &["add", "."]);
        git(repository.path(), &["commit", "-qm", "head"]);
        git(repository.path(), &["branch", "topic"]);
        git(repository.path(), &["checkout", "-q", "base"]);
        std::fs::write(repository.path().join("base-only.rs"), "fn base_only() {}").unwrap();
        git(repository.path(), &["add", "."]);
        git(repository.path(), &["commit", "-qm", "base advanced"]);
        let store = Arc::new(ReviewStore::open(&data.path().join("store.sqlite3")).unwrap());
        let entry = store
            .upsert_repository(&verify_repository_location(repository.path()).unwrap())
            .unwrap();
        store.insert_workspace_trust(entry.id).unwrap();
        let lifecycle = Arc::new(ReviewLifecycle::new(store));
        let presentation = lifecycle
            .present(
                &OperationContext::background(),
                PresentReview::Branch {
                    repository_id: entry.id,
                    base_ref: "refs/heads/base".into(),
                    head_ref: "refs/heads/topic".into(),
                },
            )
            .await
            .unwrap();
        let head = lifecycle
            .workspace_path(&presentation.review_id, &presentation.snapshot_id)
            .unwrap();
        Self {
            _data: data,
            repository,
            lifecycle,
            presentation,
            head,
        }
    }

    /// 建立同时改动 TS 与 JS 的真实 Review，供固定 TLS 生命周期夹具使用。
    async fn typescript_javascript() -> Self {
        let data = tempfile::tempdir().unwrap();
        let repository = tempfile::tempdir().unwrap();
        git(repository.path(), &["init", "-q"]);
        git(
            repository.path(),
            &["config", "user.email", "test@narrant.invalid"],
        );
        git(repository.path(), &["config", "user.name", "Narrant Test"]);
        std::fs::write(
            repository.path().join("package.json"),
            r#"{"name":"structure-review-typescript-fixture","private":true,"type":"module"}"#,
        )
        .unwrap();
        std::fs::write(
            repository.path().join("tsconfig.json"),
            r#"{"compilerOptions":{"target":"ES2022","module":"NodeNext","moduleResolution":"NodeNext","allowJs":true,"checkJs":true,"strict":true},"include":["*.ts","*.js"]}"#,
        )
        .unwrap();
        std::fs::write(
            repository.path().join("format-review.js"),
            "export function formatReview(value) { return String(value); }\n",
        )
        .unwrap();
        std::fs::write(
            repository.path().join("main.ts"),
            "import type { MissingReview } from 'missing-business-package';\nimport { formatReview } from './format-review.js';\nexport function run(review: MissingReview) { return formatReview(review); }\n",
        )
        .unwrap();
        git(repository.path(), &["add", "."]);
        git(repository.path(), &["commit", "-qm", "common"]);
        git(repository.path(), &["branch", "base"]);
        std::fs::write(
            repository.path().join("format-review.js"),
            "export function formatReview(value) { return String(value).trim(); }\n",
        )
        .unwrap();
        std::fs::write(
            repository.path().join("main.ts"),
            "import type { MissingReview } from 'missing-business-package';\nimport { formatReview } from './format-review.js';\nexport function run(review: MissingReview) { return `${formatReview(review)}!`; }\n",
        )
        .unwrap();
        git(repository.path(), &["add", "."]);
        git(repository.path(), &["commit", "-qm", "head"]);
        git(repository.path(), &["branch", "topic"]);
        git(repository.path(), &["checkout", "-q", "base"]);
        let store = Arc::new(ReviewStore::open(&data.path().join("store.sqlite3")).unwrap());
        let entry = store
            .upsert_repository(&verify_repository_location(repository.path()).unwrap())
            .unwrap();
        store.insert_workspace_trust(entry.id).unwrap();
        let lifecycle = Arc::new(ReviewLifecycle::new(store));
        let presentation = lifecycle
            .present(
                &OperationContext::background(),
                PresentReview::Branch {
                    repository_id: entry.id,
                    base_ref: "refs/heads/base".into(),
                    head_ref: "refs/heads/topic".into(),
                },
            )
            .await
            .unwrap();
        let head = lifecycle
            .workspace_path(&presentation.review_id, &presentation.snapshot_id)
            .unwrap();
        Self {
            _data: data,
            repository,
            lifecycle,
            presentation,
            head,
        }
    }
}

/// 使用输出捕获避免测试日志被 Git 的工作区提示淹没。
fn git(repository: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repository)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}

/// 受控分析器用文件内容构造最小有效代码图，差异仍走生产 graph diff。
fn analyze_files(
    input: &AnalysisInput,
    _: CancellationCheck,
    _: RustDependencyScope,
) -> Result<CodeGraph, AnalysisError> {
    let nodes: Vec<_> = input
        .files
        .iter()
        .map(|path| {
            let source = std::fs::read_to_string(input.repository_root.join(path)).unwrap();
            CodeNode::Callable(CodeNodeData {
                id: NodeId(path.to_string_lossy().into()),
                language: LanguageId("rust".into()),
                native_kind: "fn".into(),
                name: Some(path.to_string_lossy().into()),
                signature: Some("fn value() -> u8".into()),
                code: Some(source.clone()),
                source: Some(source),
                contains: vec![],
                references: vec![],
                extensions: Default::default(),
            })
        })
        .collect();
    Ok(CodeGraph::new(
        CodeRoot {
            contains: nodes.iter().map(|node| node.data().id.clone()).collect(),
        },
        nodes,
    )
    .unwrap())
}

/// 同一侧的 Rust 与 TS/JS 共用唯一 workspace；跨语言图合并并保留真实引用目标。
#[test]
fn dispatches_mixed_language_groups_and_keeps_typescript_javascript_node_targets() {
    let repository = tempfile::tempdir().unwrap();
    let rust_calls = Arc::new(Mutex::new(Vec::new()));
    let observed_rust = rust_calls.clone();
    let analyze_rust: Arc<AnalyzeGraph> = Arc::new(move |input, _, _| {
        observed_rust
            .lock()
            .unwrap()
            .push((input.repository_root.clone(), input.files.clone()));
        Ok(single_node_graph("rust:main", "rust", vec![]))
    });
    let typescript_calls = Arc::new(Mutex::new(Vec::new()));
    let observed_typescript = typescript_calls.clone();
    let analyze_typescript: Arc<AnalyzeTypeScript> = Arc::new(move |input, _| {
        observed_typescript
            .lock()
            .unwrap()
            .push((input.repository_root.clone(), input.files.clone()));
        let helper = code_node("javascript:helper", "javascript", vec![]);
        let mut entry = code_node("typescript:entry", "typescript", vec![]);
        entry.references.push(CodeReference {
            semantic: "code.calls".into(),
            target: ReferenceTarget::Node(helper.id.clone()),
        });
        Ok(CodeGraph::new(
            CodeRoot {
                contains: vec![entry.id.clone(), helper.id.clone()],
            },
            vec![CodeNode::Callable(entry), CodeNode::Callable(helper)],
        )
        .unwrap())
    });
    let analyzers = LanguageAnalyzers {
        rust: analyze_rust,
        typescript: analyze_typescript,
        python: Arc::new(|_, _| panic!("该测试不应启动 Python")),
    };
    let not_cancelled: CancellationCheck = Arc::new(|| false);
    let graph = analyze_side(
        &analyzers,
        repository.path(),
        SideFiles {
            rust: vec!["lib.rs".into()],
            typescript: vec!["main.ts".into(), "helper.js".into()],
            ..SideFiles::default()
        },
        AnalysisOptions::default(),
        &not_cancelled,
        RustDependencyScope::StandardLibrary,
        &CancellationToken::new(),
    )
    .unwrap();
    assert_eq!(
        *rust_calls.lock().unwrap(),
        [(
            repository.path().to_path_buf(),
            vec![PathBuf::from("lib.rs")]
        )]
    );
    assert_eq!(
        *typescript_calls.lock().unwrap(),
        [(
            repository.path().to_path_buf(),
            vec![PathBuf::from("main.ts"), PathBuf::from("helper.js")]
        )]
    );
    let entry = graph.node(&NodeId("typescript:entry".into())).unwrap();
    assert_eq!(
        entry.data().references[0].target,
        ReferenceTarget::Node(NodeId("javascript:helper".into()))
    );
}

/// 空语言组不触发工具，跨语言重复身份在应用合并边界失败。
#[test]
fn skips_empty_groups_and_rejects_merge_collisions() {
    let repository = tempfile::tempdir().unwrap();
    let unavailable_rust: Arc<AnalyzeGraph> = Arc::new(|_, _, _| panic!("空组不应启动 Rust"));
    let unavailable_typescript: Arc<AnalyzeTypeScript> =
        Arc::new(|_, _| panic!("空组不应启动 TypeScript"));
    let unavailable = LanguageAnalyzers {
        rust: unavailable_rust,
        typescript: unavailable_typescript,
        python: Arc::new(|_, _| panic!("空组不应启动 Python")),
    };
    let not_cancelled: CancellationCheck = Arc::new(|| false);
    let empty = analyze_side(
        &unavailable,
        repository.path(),
        SideFiles::default(),
        AnalysisOptions::default(),
        &not_cancelled,
        RustDependencyScope::default(),
        &CancellationToken::new(),
    )
    .unwrap();
    assert!(empty.root().contains.is_empty());

    let rust: Arc<AnalyzeGraph> =
        Arc::new(|_, _, _| Ok(single_node_graph("collision", "rust", vec![])));
    let typescript: Arc<AnalyzeTypeScript> =
        Arc::new(|_, _| Ok(single_node_graph("collision", "typescript", vec![])));
    let colliding = LanguageAnalyzers {
        rust,
        typescript,
        python: Arc::new(|_, _| panic!("该测试不应启动 Python")),
    };
    let error = analyze_side(
        &colliding,
        repository.path(),
        SideFiles {
            rust: vec!["lib.rs".into()],
            typescript: vec!["main.ts".into()],
            ..SideFiles::default()
        },
        AnalysisOptions::default(),
        &not_cancelled,
        RustDependencyScope::default(),
        &CancellationToken::new(),
    )
    .unwrap_err();
    assert_eq!(error.code, "analysis-failed");
    assert!(error.detail.contains("collision"));
}

/// 一侧语言失败会取消同侧与另一侧的同组工作，并保留真实失败原因。
#[test]
fn language_failure_cancels_its_peer_and_preserves_original_cause() {
    let repository = tempfile::tempdir().unwrap();
    let stop = CancellationToken::new();
    let cancellation = stop.clone();
    let cancelled: CancellationCheck = Arc::new(move || cancellation.is_cancelled());
    let rust_stopped = Arc::new(AtomicBool::new(false));
    let observed = rust_stopped.clone();
    let rust: Arc<AnalyzeGraph> = Arc::new(move |_, cancelled, _| {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !cancelled() {
            assert!(
                std::time::Instant::now() < deadline,
                "语言 peer 必须收到取消"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        observed.store(true, Ordering::Release);
        Err(AnalysisError::cancelled("peer cancelled"))
    });
    let typescript: Arc<AnalyzeTypeScript> =
        Arc::new(|_, _| Err(AnalysisError::new("typescript original failure")));
    let failing = LanguageAnalyzers {
        rust,
        typescript,
        python: Arc::new(|_, _| panic!("该测试不应启动 Python")),
    };
    let error = analyze_side(
        &failing,
        repository.path(),
        SideFiles {
            rust: vec!["lib.rs".into()],
            typescript: vec!["main.ts".into()],
            ..SideFiles::default()
        },
        AnalysisOptions::default(),
        &cancelled,
        RustDependencyScope::default(),
        &stop,
    )
    .unwrap_err();
    assert_eq!(error.code, "analysis-failed");
    assert_eq!(error.detail, "typescript original failure");
    assert!(rust_stopped.load(Ordering::Acquire));
}

/// 支持扩展逐侧路由；声明文件与测试文件仍属于 TS/JS 语义组。
#[test]
fn routes_all_supported_extensions_without_excluding_tests_from_coverage() {
    let regular = crate::review::FileContentRef {
        object_id: "0".repeat(40),
        mode: 0o100644,
    };
    for path in [
        "lib.rs",
        "main.py",
        "api.pyi",
        "tests/test_api.py",
        "main.ts",
        "view.tsx",
        "helper.js",
        "legacy.jsx",
        "module.mts",
        "module.cts",
        "module.mjs",
        "module.cjs",
        "types.d.ts",
        "types.d.mts",
        "types.d.cts",
        "feature.test.ts",
        "__tests__/feature.spec.js",
    ] {
        assert_eq!(coverage_kind(path, Some(&regular)), "analyzed", "{path}");
    }
    assert_eq!(
        coverage_kind("README.md", Some(&regular)),
        "unsupported-language"
    );
    let symlink = crate::review::FileContentRef {
        object_id: "1".repeat(40),
        mode: 0o120000,
    };
    assert_eq!(coverage_kind("main.ts", Some(&symlink)), "unsupported-kind");
    assert_eq!(coverage_kind("main.ts", None), "absent");
}

/// 构造一个顶层 Callable 图，供应用分组测试组合。
fn single_node_graph(id: &str, language: &str, references: Vec<CodeReference>) -> CodeGraph {
    let mut node = code_node(id, language, vec![]);
    node.references = references;
    CodeGraph::new(
        CodeRoot {
            contains: vec![node.id.clone()],
        },
        vec![CodeNode::Callable(node)],
    )
    .unwrap()
}

/// 构造不依赖源码解析的受控代码对象。
fn code_node(id: &str, language: &str, contains: Vec<NodeId>) -> CodeNodeData {
    CodeNodeData {
        id: NodeId(id.into()),
        language: LanguageId(language.into()),
        native_kind: "function".into(),
        name: Some(id.into()),
        signature: Some(format!("fn {id}()")),
        code: Some("{}".into()),
        source: Some(format!("fn {id}() {{}}")),
        contains,
        references: vec![],
        extensions: Default::default(),
    }
}

/// 同一比较的两侧接收完全相同的通用选项，显式开启和默认关闭均向下传递。
#[tokio::test]
async fn passes_the_same_analysis_options_to_both_sides() {
    let fixture = Fixture::new().await;
    let observed = Arc::new(std::sync::Mutex::new(Vec::new()));
    let captured = observed.clone();
    let application = StructureReview::new(Arc::new(move |input, cancelled, dependency_scope| {
        captured
            .lock()
            .unwrap()
            .push((input.options, dependency_scope));
        analyze_files(input, cancelled, dependency_scope)
    }));
    for options in [
        AnalysisOptions::default(),
        AnalysisOptions {
            include_tests: true,
        },
    ] {
        for dependency_scope in [
            RustDependencyScope::RepositoryOnly,
            RustDependencyScope::StandardLibrary,
            RustDependencyScope::AllDependencies,
        ] {
            application
                .analyze(
                    &fixture.lifecycle,
                    &OperationContext::background(),
                    &fixture.presentation.review_id,
                    &fixture.presentation.snapshot_id,
                    options,
                    dependency_scope,
                )
                .await
                .unwrap();
        }
    }
    assert_eq!(
        observed
            .lock()
            .unwrap()
            .iter()
            .map(|(options, scope)| (options.include_tests, *scope))
            .collect::<Vec<_>>(),
        [false, true]
            .into_iter()
            .flat_map(|include_tests| {
                [
                    RustDependencyScope::RepositoryOnly,
                    RustDependencyScope::StandardLibrary,
                    RustDependencyScope::AllDependencies,
                ]
                .into_iter()
                .flat_map(move |scope| [(include_tests, scope); 2])
            })
            .collect::<Vec<_>>()
    );
}

/// 确认只增加 before，并使用固定分叉提交和各侧真实存在的文件。
#[tokio::test]
async fn reuses_head_and_compares_exact_merge_base_with_side_specific_files() {
    let fixture = Fixture::new().await;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = calls.clone();
    let application = StructureReview::new(Arc::new(move |input, cancelled, dependency_scope| {
        observed
            .lock()
            .unwrap()
            .push((input.repository_root.clone(), input.files.clone()));
        assert!(!input.repository_root.join("base-only.rs").exists());
        let count = git(&input.repository_root, &["worktree", "list", "--porcelain"])
            .matches("worktree ")
            .count();
        assert!((2..=3).contains(&count));
        analyze_files(input, cancelled, dependency_scope)
    }));
    let result = application
        .analyze(
            &fixture.lifecycle,
            &OperationContext::background(),
            &fixture.presentation.review_id,
            &fixture.presentation.snapshot_id,
            AnalysisOptions::default(),
            RustDependencyScope::default(),
        )
        .await
        .unwrap();
    let mut calls = calls.lock().unwrap();
    calls.sort_by_key(|call| call.0 == fixture.head);
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].0, fixture.head.parent().unwrap().join("before"));
    assert_eq!(calls[1].0, fixture.head);
    assert!(calls[0].1.contains(&PathBuf::from("removed.rs")));
    assert!(!calls[0].1.contains(&PathBuf::from("added.rs")));
    assert!(calls[1].1.contains(&PathBuf::from("added.rs")));
    assert!(!calls[1].1.contains(&PathBuf::from("removed.rs")));
    assert!(!calls[0].0.exists());
    assert!(fixture.head.exists());
    // before 已回收，传输投影仍持有两侧原始源码，详情无需再次 checkout 或读取文件。
    let serialized = serde_json::to_value(&result).unwrap();
    for (side, expected) in [
        ("before", "mod removed; fn value() -> u8 { 1 }"),
        ("after", "mod added; fn value() -> u8 { 2 }"),
    ] {
        let node = serialized[side]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|node| node["id"] == "lib.rs")
            .unwrap();
        assert_eq!(node["source"], expected);
    }
    assert_eq!(
        git(
            fixture.repository.path(),
            &["worktree", "list", "--porcelain"]
        )
        .matches("worktree ")
        .count(),
        2
    );
    let body = result
        .diff
        .nodes
        .iter()
        .find(|node| node.id == "lib.rs")
        .unwrap();
    assert_eq!(body.content, Some("modified"));
    assert_eq!(
        result
            .coverage
            .iter()
            .find(|file| file.path == "README.md")
            .unwrap()
            .after,
        "unsupported-language"
    );
    assert_ne!(
        result.comparison_base,
        fixture.presentation.snapshot.base_revision
    );
    let json = serde_json::to_value(result).unwrap();
    assert!(json.get("snapshotId").is_some());
    assert!(json["before"]["nodes"][0].get("nativeKind").is_some());
}

/// 关闭先撤销分析，再等实际 worker 释放目录。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn close_revokes_analysis_and_waits_for_actual_worker_before_removing_head() {
    assert_workspace_drain("close").await;
}

/// 刷新前排空分析，发布的新 head 不会被旧 worker 读取。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refresh_revokes_analysis_before_replacing_head() {
    assert_workspace_drain("refresh").await;
}

/// 丢弃调用方等待时，目录保护仍由实际 worker 拥有。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abandoned_await_keeps_head_lease_until_worker_exits() {
    assert_workspace_drain("abandon").await;
}

/// 全局退出先撤销正在执行的分析，等待两侧和 before 清理后再删除 head。
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn shutdown_revokes_analysis_and_waits_for_actual_workers() {
    assert_workspace_drain("shutdown").await;
}

/// 用真实目录和完成屏障验证异步等待者与实际 worker 的不同寿命。
async fn assert_workspace_drain(action: &str) {
    let fixture = Fixture::new().await;
    let confirmation = if action == "refresh" {
        git(fixture.repository.path(), &["checkout", "-q", "topic"]);
        std::fs::write(
            fixture.repository.path().join("lib.rs"),
            "mod added; fn value() -> u8 { 3 }",
        )
        .unwrap();
        git(fixture.repository.path(), &["commit", "-qam", "next head"]);
        let update = fixture
            .lifecycle
            .check_update(
                &OperationContext::background(),
                &fixture.presentation.review_id,
            )
            .await
            .unwrap();
        Some(crate::review::ConfirmedReviewUpdate {
            current_base_revision: update.current_base_revision,
            current_head_revision: update.current_head_revision,
            available_base_revision: update.available_base_revision,
            available_head_revision: update.available_head_revision,
        })
    } else {
        None
    };
    let started = Arc::new(tokio::sync::Semaphore::new(0));
    let cancelled_seen = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(AtomicBool::new(false));
    let (worker_started, worker_cancelled, worker_release) =
        (started.clone(), cancelled_seen.clone(), release.clone());
    let application = StructureReview::new(Arc::new(move |_, cancelled, _| {
        worker_started.add_permits(1);
        while !cancelled() {
            std::thread::sleep(Duration::from_millis(5));
        }
        worker_cancelled.notify_one();
        while !worker_release.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(5));
        }
        Err(AnalysisError::cancelled("cancelled"))
    }));
    let lifecycle = fixture.lifecycle.clone();
    let (review, snapshot) = (
        fixture.presentation.review_id.clone(),
        fixture.presentation.snapshot_id.clone(),
    );
    let running = tokio::spawn(async move {
        application
            .analyze(
                &lifecycle,
                &OperationContext::background(),
                &review,
                &snapshot,
                AnalysisOptions::default(),
                RustDependencyScope::default(),
            )
            .await
    });
    tokio::time::timeout(Duration::from_secs(10), started.acquire_many(2))
        .await
        .unwrap()
        .unwrap()
        .forget();
    let lifecycle = fixture.lifecycle.clone();
    let review = fixture.presentation.review_id.clone();
    if action == "abandon" {
        running.abort();
    }
    let shutdown = action == "shutdown";
    let closing = tokio::spawn(async move {
        if let Some(confirmation) = confirmation {
            lifecycle
                .refresh(&OperationContext::background(), &review, confirmation)
                .await
                .map(|_| ())
        } else if shutdown {
            lifecycle.shutdown().await.map(|_| ())
        } else {
            lifecycle.close(&review).await.map(|_| ())
        }
    });
    tokio::time::timeout(Duration::from_secs(10), cancelled_seen.notified())
        .await
        .unwrap();
    assert!(!closing.is_finished());
    assert!(fixture.head.exists());
    assert!(fixture.head.parent().unwrap().join("before").exists());
    release.store(true, Ordering::Release);
    if action == "abandon" {
        assert!(running.await.unwrap_err().is_cancelled());
    } else {
        assert_eq!(running.await.unwrap().unwrap_err().code, "cancelled");
    }
    closing.await.unwrap().unwrap();
    assert_eq!(fixture.head.exists(), action == "refresh");
    if action == "refresh" {
        assert!(std::fs::read_to_string(fixture.head.join("lib.rs"))
            .unwrap()
            .contains("{ 3 }"));
    }
    assert!(!fixture.head.parent().unwrap().join("before").exists());
}

/// 分析失败只回收附加目录，当前 Review 仍可继续使用。
#[tokio::test]
async fn analysis_failure_cleans_before_without_closing_review() {
    let fixture = Fixture::new().await;
    let application = StructureReview::new(Arc::new(|_, _, _| {
        Err(AnalysisError::new("server missing"))
    }));
    let result = application
        .analyze(
            &fixture.lifecycle,
            &OperationContext::background(),
            &fixture.presentation.review_id,
            &fixture.presentation.snapshot_id,
            AnalysisOptions::default(),
            RustDependencyScope::default(),
        )
        .await;
    assert_eq!(result.unwrap_err().code, "analysis-failed");
    assert!(fixture
        .lifecycle
        .workspace_path(
            &fixture.presentation.review_id,
            &fixture.presentation.snapshot_id
        )
        .is_ok());
    assert!(fixture.head.exists());
    assert!(!fixture.head.parent().unwrap().join("before").exists());
}

/// 不属于受管 worktree 的 before 路径必须保留，准备失败不能影响已有 head。
#[tokio::test]
async fn refuses_foreign_before_directory_and_stale_snapshot() {
    let fixture = Fixture::new().await;
    let before = fixture.head.parent().unwrap().join("before");
    std::fs::create_dir(&before).unwrap();
    std::fs::write(before.join("keep"), "unrelated").unwrap();
    let application = StructureReview::new(Arc::new(analyze_files));
    let result = application
        .analyze(
            &fixture.lifecycle,
            &OperationContext::background(),
            &fixture.presentation.review_id,
            &fixture.presentation.snapshot_id,
            AnalysisOptions::default(),
            RustDependencyScope::default(),
        )
        .await;
    assert_eq!(result.unwrap_err().code, "cleanup-failed");
    assert_eq!(
        std::fs::read_to_string(before.join("keep")).unwrap(),
        "unrelated"
    );
    assert!(fixture.head.exists());
    let result = application
        .analyze(
            &fixture.lifecycle,
            &OperationContext::background(),
            &fixture.presentation.review_id,
            "stale",
            AnalysisOptions::default(),
            RustDependencyScope::default(),
        )
        .await;
    assert_eq!(result.unwrap_err().code, "review-unavailable");
}

/// 崩溃后由既有 Review 清理目标恢复 before 的准确归属，无需另建持久化图记录。
#[tokio::test]
async fn recovery_removes_interrupted_before_worktree() {
    let fixture = Fixture::new().await;
    let before = fixture.head.parent().unwrap().join("before");
    git(
        fixture.repository.path(),
        &[
            "worktree",
            "add",
            "--detach",
            before.to_str().unwrap(),
            &fixture.presentation.snapshot.comparison_base,
        ],
    );
    let report = fixture.lifecycle.recover().unwrap();
    assert!(report.cleanup_warnings.is_empty());
    assert!(!before.exists());
    assert_eq!(
        git(
            fixture.repository.path(),
            &["worktree", "list", "--porcelain"]
        )
        .matches("worktree ")
        .count(),
        1
    );
}

/// 从 Review 固定提交到真实 Rust LSP，再到前端 DTO 的完整生产调用链。
#[tokio::test]
#[ignore = "需要显式配置 rust-analyzer，串行运行且避免测试进程占用 Cargo 锁"]
async fn real_server_builds_review_structure_comparison() {
    use crate::structure_review::{
        code_graph::LanguageAnalyzer, rust_analyzer::RustLanguageAnalyzer,
    };
    let fixture = Fixture::new().await;
    let executable = std::env::var_os("NARRANT_RUST_ANALYZER").expect("需要提供 rust-analyzer");
    let analyzer = RustLanguageAnalyzer::new(executable);
    let application = StructureReview::new(Arc::new(move |input, cancelled, dependency_scope| {
        analyzer
            .clone()
            .with_cancellation(cancelled)
            .with_dependency_scope(dependency_scope)
            .analyze(input)
    }));
    let result = application
        .analyze(
            &fixture.lifecycle,
            &OperationContext::background(),
            &fixture.presentation.review_id,
            &fixture.presentation.snapshot_id,
            AnalysisOptions::default(),
            RustDependencyScope::default(),
        )
        .await
        .unwrap();
    let function = result
        .after
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("value"))
        .unwrap();
    assert_eq!(
        result
            .diff
            .nodes
            .iter()
            .find(|change| change.id == function.id)
            .unwrap()
            .content,
        Some("modified")
    );
    assert!(function.code.as_ref().unwrap().contains('2'));
    assert!(result
        .diff
        .nodes
        .iter()
        .any(|change| change.content == Some("added")));
    assert!(result
        .diff
        .nodes
        .iter()
        .any(|change| change.content == Some("removed")));
    assert!(!fixture.head.parent().unwrap().join("before").exists());
    assert!(fixture.head.exists());
    if let Some(output) = std::env::var_os("NARRANT_CODE_GRAPH_OUTPUT") {
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(
            Path::new(&output).join("structure-comparison.json"),
            serde_json::to_vec_pretty(&result).unwrap(),
        )
        .unwrap();
    }
    println!(
        "Structure Review: before={}, after={}, changed={}",
        result.before.nodes.len(),
        result.after.nodes.len(),
        result.diff.nodes.len()
    );
}

/// 从真实 Review 生命周期经固定 TS/JS 服务到 DTO，保留本地 Node 目标、原文与结构差异。
#[tokio::test]
#[ignore = "需要固定 NARRANT_TYPESCRIPT_NODE/NARRANT_TYPESCRIPT_RUNTIME；直接运行测试二进制"]
async fn real_typescript_javascript_graphs_flow_through_review_lifecycle_to_dto() {
    use crate::structure_review::{
        code_graph::LanguageAnalyzer, typescript_analyzer::TypeScriptLanguageAnalyzer,
    };
    let fixture = Fixture::typescript_javascript().await;
    let analyzer = TypeScriptLanguageAnalyzer::default();
    let application = StructureReview::new(Arc::new(|_, _, _| {
        panic!("TS/JS-only Review 不应启动 rust-analyzer")
    }))
    .with_typescript(Arc::new(move |input, cancelled| {
        analyzer.clone().with_cancellation(cancelled).analyze(input)
    }));
    let result = application
        .analyze(
            &fixture.lifecycle,
            &OperationContext::background(),
            &fixture.presentation.review_id,
            &fixture.presentation.snapshot_id,
            AnalysisOptions::default(),
            RustDependencyScope::default(),
        )
        .await
        .unwrap();
    let run = result
        .after
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("run"))
        .unwrap();
    let target = run
        .references
        .iter()
        .find(|reference| reference.semantic == "code.calls")
        .and_then(|reference| match &reference.target {
            projection::Target::Node { id } => Some(id),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        result
            .after
            .nodes
            .iter()
            .find(|node| &node.id == target)
            .and_then(|node| node.name.as_deref()),
        Some("formatReview")
    );
    assert!(run
        .source
        .as_deref()
        .unwrap()
        .contains("formatReview(review)"));
    assert_eq!(
        result
            .diff
            .nodes
            .iter()
            .find(|change| change.id == run.id)
            .and_then(|change| change.content),
        Some("modified")
    );
    assert!(result
        .coverage
        .iter()
        .all(|file| { file.path == "main.ts" || file.path == "format-review.js" }));
    assert!(!fixture.head.parent().unwrap().join("before").exists());
    assert!(fixture.head.exists());
}

#[path = "parallel.rs"]
mod parallel;

#[path = "workspace_execution.rs"]
mod workspace_execution;

#[path = "benchmark.rs"]
mod benchmark;

#[path = "python.rs"]
mod python;
