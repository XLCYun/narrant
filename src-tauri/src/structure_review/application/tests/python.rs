//! Python 从真实 Git 比较到应用 DTO 的路由、合并与生命周期验收。

use super::*;
use crate::structure_review::{code_graph::LanguageAnalyzer, codegraph_client::CodeGraphAnalyzer};

/// 用小型真实仓库建立新增、修改与仅删除场景，保留实际 Review 工作区管理。
async fn fixture(files: &[(&str, &str, Option<&str>)]) -> Fixture {
    let data = tempfile::tempdir().unwrap();
    let repository = tempfile::tempdir().unwrap();
    git(repository.path(), &["init", "-q"]);
    git(
        repository.path(),
        &["config", "user.email", "test@narrant.invalid"],
    );
    git(repository.path(), &["config", "user.name", "Narrant Test"]);
    for (path, before, _) in files {
        std::fs::write(repository.path().join(path), before).unwrap();
    }
    git(repository.path(), &["add", "."]);
    git(repository.path(), &["commit", "-qm", "before"]);
    git(repository.path(), &["branch", "base"]);
    for (path, _, after) in files {
        let path = repository.path().join(path);
        if let Some(after) = after {
            std::fs::write(path, after).unwrap();
        } else {
            std::fs::remove_file(path).unwrap();
        }
    }
    git(repository.path(), &["add", "."]);
    git(repository.path(), &["commit", "-qm", "after"]);
    git(repository.path(), &["branch", "topic"]);
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
    Fixture {
        _data: data,
        repository,
        lifecycle,
        presentation,
        head,
    }
}

/// 三种语言使用同一侧路径；Python 的真实调用目标和删除声明通过统一投影输出。
#[tokio::test]
#[ignore = "需要真实 NARRANT_PYTHON_PACK"]
async fn native_python_mixed_review_uses_two_shared_workspaces() {
    let fixture = fixture(&[
        (
            "main.py",
            "def helper(): return 1\ndef run(): return helper()\n",
            Some("def helper(): return 2\ndef run(): return helper() + 1\n"),
        ),
        ("removed.pyi", "def removed() -> None: ...\n", None),
        ("lib.rs", "fn main() {}", Some("fn main() { changed(); }")),
        (
            "main.ts",
            "export const a = 1;",
            Some("export const a = 2;"),
        ),
    ])
    .await;
    let observations = Arc::new(Mutex::new(Vec::new()));
    let rust = observations.clone();
    let typescript = observations.clone();
    let python = observations.clone();
    let pack = std::env::var_os("NARRANT_PYTHON_PACK").expect("真实 Python 包");
    let analyzer = CodeGraphAnalyzer::from_pack("python", Path::new(&pack)).unwrap();
    let application = StructureReview::new(Arc::new(move |input, _, _| {
        rust.lock()
            .unwrap()
            .push(("rust", input.repository_root.clone()));
        Ok(single_node_graph("rust:entry", "rust", vec![]))
    }))
    .with_typescript(Arc::new(move |input, _| {
        typescript
            .lock()
            .unwrap()
            .push(("typescript", input.repository_root.clone()));
        Ok(single_node_graph("typescript:entry", "typescript", vec![]))
    }))
    .with_python(Arc::new(move |input, cancelled| {
        python
            .lock()
            .unwrap()
            .push(("python", input.repository_root.clone()));
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
    let helper = result
        .after
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("helper"))
        .unwrap();
    assert!(run.references.iter().any(
        |reference| matches!(&reference.target, projection::Target::Node { id } if id == &helper.id)
    ));
    assert!(result
        .diff
        .nodes
        .iter()
        .any(|change| change.id == run.id && change.content == Some("modified")));
    let removed = result
        .before
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some("removed"))
        .unwrap();
    assert!(result
        .diff
        .nodes
        .iter()
        .any(|change| change.id == removed.id && change.content == Some("removed")));
    assert_eq!(
        result
            .coverage
            .iter()
            .find(|file| file.path == "removed.pyi")
            .unwrap()
            .after,
        "absent"
    );
    let observations = observations.lock().unwrap();
    assert_eq!(observations.len(), 6);
    let roots: std::collections::BTreeSet<_> = observations.iter().map(|(_, root)| root).collect();
    assert_eq!(roots.len(), 2);
    for root in roots {
        assert_eq!(
            observations.iter().filter(|(_, path)| path == root).count(),
            3
        );
    }
    assert!(!fixture.head.parent().unwrap().join("before").exists());
    assert!(fixture.head.exists());
}

/// 仅删除 Python 文件仍分析 before；不存在的 after 不启动任何工具。
#[tokio::test]
async fn deleted_python_analyzes_only_before() {
    let fixture = fixture(&[("removed.py", "def removed(): pass\n", None)]).await;
    let calls = Arc::new(Mutex::new(Vec::new()));
    let observed = calls.clone();
    let application = StructureReview::new(Arc::new(|_, _, _| panic!("没有 Rust 文件")))
        .with_typescript(Arc::new(|_, _| panic!("没有 TS 文件")))
        .with_python(Arc::new(move |input, _| {
            observed.lock().unwrap().push(input.repository_root.clone());
            assert_eq!(input.files, [PathBuf::from("removed.py")]);
            assert!(input.repository_root.join("removed.py").is_file());
            Ok(single_node_graph("python:removed", "python", vec![]))
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
    assert_eq!(calls.lock().unwrap().len(), 1);
    assert!(result.after.nodes.is_empty());
    assert!(result
        .diff
        .nodes
        .iter()
        .any(|change| change.id == "python:removed" && change.content == Some("removed")));
    assert!(!fixture.head.parent().unwrap().join("before").exists());
}

/// Python 根因必须保留；返回前等待两种同侧语言观察取消并完成退出。
#[test]
fn python_failure_joins_both_language_peers() {
    let repository = tempfile::tempdir().unwrap();
    let stop = CancellationToken::new();
    let signal = stop.clone();
    let cancelled: CancellationCheck = Arc::new(move || signal.is_cancelled());
    let completed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let rust = completed.clone();
    let typescript = completed.clone();
    let analyzers = LanguageAnalyzers {
        native: BTreeMap::new(),
        rust: Arc::new(move |_, cancelled, _| {
            wait_for_cancel(cancelled);
            rust.fetch_add(1, Ordering::SeqCst);
            Err(AnalysisError::cancelled("Rust stopped"))
        }),
        typescript: Arc::new(move |_, cancelled| {
            wait_for_cancel(cancelled);
            typescript.fetch_add(1, Ordering::SeqCst);
            Err(AnalysisError::cancelled("TS stopped"))
        }),
        python: Arc::new(|_, _| Err(AnalysisError::new("Python 原始失败"))),
    };
    let error = analyze_side(
        &analyzers,
        repository.path(),
        SideFiles {
            rust: vec!["lib.rs".into()],
            typescript: vec!["main.ts".into()],
            python: vec!["main.py".into()],
            ..SideFiles::default()
        },
        AnalysisOptions::default(),
        &cancelled,
        RustDependencyScope::default(),
        &stop,
    )
    .unwrap_err();
    assert_eq!(error.detail, "Python 原始失败");
    assert_eq!(completed.load(Ordering::SeqCst), 2);
}

/// 受控语言模拟真实工作线程，仅在观察到取消后退出。
fn wait_for_cancel(cancelled: CancellationCheck) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !cancelled() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 全部改动文件被测试选项过滤时，即便本地没有 Python 包也能完成空图比较。
#[tokio::test]
async fn filtered_python_tests_do_not_prepare_a_support_pack() {
    let fixture = fixture(&[(
        "test_api.py",
        "def test_api(): pass\n",
        Some("def test_api(): return 1\n"),
    )])
    .await;
    let root = tempfile::tempdir().unwrap();
    let support =
        crate::language_support::LanguageSupportPacks::new(root.path().join("packs")).unwrap();
    let application = StructureReview::new(Arc::new(|_, _, _| panic!("没有 Rust 文件")))
        .with_python(Arc::new(|_, _| panic!("已过滤 Python 组不应分析")))
        .with_support_packs(support.clone());
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
    assert!(result.before.nodes.is_empty() && result.after.nodes.is_empty());
    support.shutdown().await;
}

/// 真实优化归档经安装工厂与固定 Git 两侧输出 DTO；语法失败和取消保留生命周期回收。
#[tokio::test]
#[ignore = "需要真实 NARRANT_CODEGRAPH_ARTIFACTS Python 优化归档"]
async fn real_python_pack_flows_through_review_lifecycle_to_dto() {
    use httpmock::prelude::*;
    let artifacts =
        PathBuf::from(std::env::var_os("NARRANT_CODEGRAPH_ARTIFACTS").expect("真实 Python 归档"));
    let target = if cfg!(windows) {
        "x86_64-pc-windows-msvc"
    } else {
        "aarch64-apple-darwin"
    };
    let server = MockServer::start();
    crate::language_support::test_support::serve_artifacts(&server, &artifacts, target);
    for invalid in [false, true] {
        let fixture = fixture(&[
            (
                "main.py",
                "def helper(): return 1\ndef run(): return helper()\n",
                Some(if invalid {
                    "def broken(\n"
                } else {
                    "def helper(): return 2\ndef run(): return helper() + len([])\n"
                }),
            ),
            ("removed.pyi", "def removed() -> None: ...\n", None),
        ])
        .await;
        let packs = crate::language_support::LanguageSupportPacks::with_test_origin(
            fixture._data.path().join("packs"),
            server.base_url(),
            target.into(),
        )
        .unwrap();
        let application = StructureReview::new(Arc::new(|_, _, _| panic!("没有 Rust 文件")))
            .with_python(Arc::new(|_, _| panic!("生产工厂必须替换 Python 路由")))
            .with_support_packs(packs.clone());
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
        if invalid {
            let error = result.unwrap_err();
            assert_eq!(error.code, "analysis-failed");
            assert!(error.detail.contains("analysis_failed"), "{}", error.detail);
        } else {
            let result = result.unwrap();
            let run = result
                .after
                .nodes
                .iter()
                .find(|node| node.name.as_deref() == Some("run"))
                .unwrap();
            let helper = result
                .after
                .nodes
                .iter()
                .find(|node| node.name.as_deref() == Some("helper"))
                .unwrap();
            assert!(run.references.iter().any(|reference| matches!(&reference.target, projection::Target::Node { id } if id == &helper.id)));
            assert!(result
                .diff
                .nodes
                .iter()
                .any(|change| change.id == run.id && change.content == Some("modified")));
            assert!(result
                .diff
                .nodes
                .iter()
                .any(|change| change.content == Some("removed")));
            assert_eq!(
                result
                    .coverage
                    .iter()
                    .find(|file| file.path == "removed.pyi")
                    .unwrap()
                    .after,
                "absent"
            );
            if let Some(output) = std::env::var_os("NARRANT_CODE_GRAPH_OUTPUT") {
                std::fs::create_dir_all(&output).unwrap();
                std::fs::write(
                    Path::new(&output).join("python-structure-comparison.json"),
                    serde_json::to_vec_pretty(&result).unwrap(),
                )
                .unwrap();
            }
            println!(
                "Python Review: before={}, after={}, changes={}",
                result.before.nodes.len(),
                result.after.nodes.len(),
                result.diff.nodes.len()
            );
        }
        assert!(!fixture.head.parent().unwrap().join("before").exists());
        assert!(fixture.head.exists());
        // 同一生产安装复用后，已取消操作仍失败且不留下 before 工作区。
        let token = CancellationToken::new();
        token.cancel();
        let error = application
            .analyze(
                &fixture.lifecycle,
                &OperationContext::new(token),
                &fixture.presentation.review_id,
                &fixture.presentation.snapshot_id,
                AnalysisOptions::default(),
                RustDependencyScope::default(),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "cancelled");
        assert!(!fixture.head.parent().unwrap().join("before").exists());
        packs
            .remove(crate::language_support::PackId::Python, "0.1.0")
            .unwrap();
        packs.shutdown().await;
    }
}
