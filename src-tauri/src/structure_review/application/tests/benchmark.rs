//! 显式运行的真实仓库基准；与生产并行入口比较同一输入下的原串行流程。

use super::*;
use crate::structure_review::{
    code_graph::LanguageAnalyzer, rust_analyzer::RustLanguageAnalyzer,
    typescript_analyzer::TypeScriptLanguageAnalyzer,
};
use serde_json::{json, Value};
use std::time::Instant;

/// 共用对象库建立一次独立 Review，head 仅准备一次，每轮复用。
async fn fixture() -> Fixture {
    let source = PathBuf::from(std::env::var_os("NARRANT_STRUCTURE_BENCHMARK_REPOSITORY").unwrap());
    let base = std::env::var("NARRANT_STRUCTURE_BENCHMARK_BASE").unwrap();
    let head = std::env::var("NARRANT_STRUCTURE_BENCHMARK_HEAD").unwrap();
    let data = tempfile::tempdir().unwrap();
    let repository = tempfile::tempdir().unwrap();
    git(
        &source,
        &[
            "clone",
            "--quiet",
            "--shared",
            "--no-checkout",
            source.to_str().unwrap(),
            repository.path().to_str().unwrap(),
        ],
    );
    git(repository.path(), &["branch", "benchmark-base", &base]);
    git(repository.path(), &["branch", "benchmark-head", &head]);
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
                base_ref: "refs/heads/benchmark-base".into(),
                head_ref: "refs/heads/benchmark-head".into(),
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

/// 基准用串行对照保留生产的 lease、before 准备、head 校验、图比较和清理。
async fn sequential(
    fixture: &Fixture,
    analyze: Arc<AnalyzeGraph>,
    analyze_typescript: Arc<AnalyzeTypeScript>,
    dependency_scope: RustDependencyScope,
) -> StructureComparison {
    let analyzers = LanguageAnalyzers {
        rust: analyze,
        typescript: analyze_typescript,
        python: Arc::new(|_, _| panic!("该测试不应启动 Python")),
    };
    let operation = OperationContext::background();
    let review_id = fixture.presentation.review_id.clone();
    let snapshot_id = fixture.presentation.snapshot_id.clone();
    fixture
        .lifecycle
        .run_in_workspace(
            &operation,
            &fixture.presentation.review_id,
            &fixture.presentation.snapshot_id,
            move |workspace| {
                let operation = &workspace.operation;
                let (before_files, after_files, coverage) = scope(workspace);
                let checkout = (!before_files.is_empty())
                    .then(|| workspace::BeforeCheckout::prepare(workspace, operation).unwrap());
                let cancelled: CancellationCheck = Arc::new(|| false);
                let before = analyze_side(
                    &analyzers,
                    &workspace.before_path,
                    before_files,
                    AnalysisOptions::default(),
                    &cancelled,
                    dependency_scope,
                    operation.cancellation(),
                )
                .unwrap();
                let after = analyze_side(
                    &analyzers,
                    &workspace.head_path,
                    after_files,
                    AnalysisOptions::default(),
                    &cancelled,
                    dependency_scope,
                    operation.cancellation(),
                )
                .unwrap();
                let diff = before.diff(&after).unwrap();
                let result = StructureComparison {
                    review_id,
                    snapshot_id,
                    comparison_base: workspace.comparison.comparison_base.clone(),
                    head_revision: workspace.comparison.head_revision.clone(),
                    before: Graph::from(&before),
                    after: Graph::from(&after),
                    diff: GraphDifference::from(&diff),
                    coverage,
                };
                if let Some(checkout) = checkout {
                    checkout.finish().unwrap();
                }
                Ok::<_, StructureReviewError>(result)
            },
        )
        .await
        .unwrap()
}

/// 在同一 head 上交替执行串行和并行，并逐字段确认结果相同。
#[tokio::test]
#[ignore = "真实性能基准：设置 NARRANT_STRUCTURE_BENCHMARK_* 与 NARRANT_RUST_ANALYZER，直接运行测试二进制"]
async fn compares_sequential_and_parallel_repository_analysis() {
    let fixture = fixture().await;
    let executable = std::env::var_os("NARRANT_RUST_ANALYZER").unwrap();
    let analyzer = RustLanguageAnalyzer::new(executable);
    let typescript_analyzer = TypeScriptLanguageAnalyzer::default();
    let mut measurements = Vec::new();
    let scope: RustDependencyScope =
        serde_json::from_value(json!(std::env::var("NARRANT_STRUCTURE_BENCHMARK_SCOPE")
            .unwrap_or_else(|_| "standard-library".into())))
        .unwrap();
    let mut expected = None;
    for sample in 0..3 {
        for execution in ["sequential", "parallel"] {
            let start = Instant::now();
            let sides = Arc::new(Mutex::new(Vec::<Value>::new()));
            let captured = sides.clone();
            let analyzer = analyzer.clone();
            let analyze: Arc<AnalyzeGraph> = Arc::new(move |input, cancelled, scope| {
                let side_start = Instant::now();
                let graph = analyzer
                    .clone()
                    .with_cancellation(cancelled)
                    .with_dependency_scope(scope)
                    .analyze(input)?;
                captured.lock().unwrap().push(json!({
                    "side": if input.repository_root.ends_with("before") { "before" } else { "after" },
                    "started_ms": side_start.duration_since(start).as_secs_f64() * 1000.0,
                    "elapsed_ms": side_start.elapsed().as_secs_f64() * 1000.0,
                }));
                Ok(graph)
            });
            let analyzer = typescript_analyzer.clone();
            let analyze_typescript: Arc<AnalyzeTypeScript> = Arc::new(move |input, cancelled| {
                analyzer.clone().with_cancellation(cancelled).analyze(input)
            });
            let result = if execution == "sequential" {
                sequential(&fixture, analyze, analyze_typescript, scope).await
            } else {
                StructureReview::new(analyze)
                    .with_typescript(analyze_typescript)
                    .analyze(
                        &fixture.lifecycle,
                        &OperationContext::background(),
                        &fixture.presentation.review_id,
                        &fixture.presentation.snapshot_id,
                        AnalysisOptions::default(),
                        scope,
                    )
                    .await
                    .unwrap()
            };
            let elapsed = start.elapsed().as_secs_f64() * 1000.0;
            let value = serde_json::to_value(&result).unwrap();
            if let Some(expected) = &expected {
                assert_eq!(&value, expected, "并行不能改变图结果");
            } else {
                expected = Some(value);
            }
            let record = json!({
                "sample": sample, "execution": execution, "scope": scope,
                "elapsed_ms": elapsed, "sides": *sides.lock().unwrap(),
                "before_nodes": result.before.nodes.len(), "after_nodes": result.after.nodes.len(),
                "changed_nodes": result.diff.nodes.len(),
            });
            println!("{record}");
            measurements.push(record);
        }
    }
    fixture
        .lifecycle
        .close(&fixture.presentation.review_id)
        .await
        .unwrap();
    if let Some(path) = std::env::var_os("NARRANT_STRUCTURE_BENCHMARK_OUTPUT") {
        std::fs::write(path, serde_json::to_vec_pretty(&measurements).unwrap()).unwrap();
    }
}
