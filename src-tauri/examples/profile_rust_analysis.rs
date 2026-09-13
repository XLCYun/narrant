//! 用两份已准备好的仓库和改动文件复现结构分析，避免把编译耗时算进分析耗时。

use narrant_lib::structure_review::{
    code_graph::{AnalysisInput, CodeGraph, LanguageAnalyzer},
    rust_analyzer::RustLanguageAnalyzer,
};
use std::{
    collections::{hash_map::DefaultHasher, BTreeSet},
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    time::Instant,
};

/// 输出同一构建内可比较的图摘要，不将调试哈希作为持久化格式。
fn summary(graph: &CodeGraph) -> (usize, usize, u64) {
    let mut visited = BTreeSet::new();
    let mut pending = graph.root().contains.clone();
    let mut references = 0;
    while let Some(id) = pending.pop() {
        if !visited.insert(id.clone()) {
            continue;
        }
        let node = graph.node(&id).expect("有效图中的节点必须存在");
        references += node.data().references.len();
        pending.extend(node.data().contains.clone());
    }
    let mut hash = DefaultHasher::new();
    format!("{graph:?}").hash(&mut hash);
    (visited.len(), references, hash.finish())
}

/// 只分析该侧实际存在的改动文件，测试代码沿用产品默认的排除规则。
fn analyze(
    analyzer: &RustLanguageAnalyzer,
    side: &str,
    repository: &Path,
    files: &[PathBuf],
) -> CodeGraph {
    let files: Vec<_> = files
        .iter()
        .filter(|file| repository.join(file).is_file())
        .cloned()
        .collect();
    let started = Instant::now();
    let _span =
        tracing::info_span!(target: "narrant::structure_review", "profile_side", side).entered();
    println!(
        "[rust-analysis-profile] side={side} started files={}",
        files.len()
    );
    let graph = analyzer
        .analyze(&AnalysisInput {
            repository_root: repository.to_owned(),
            files,
            options: Default::default(),
        })
        .expect("真实 Rust 分析失败");
    let elapsed = started.elapsed();
    let (nodes, references, digest) = summary(&graph);
    println!("[rust-analysis-profile] side={side} elapsed_ms={} nodes={nodes} references={references} digest={digest:016x}", elapsed.as_millis());
    graph
}

/// 示例只运行分析；Git checkout 准备和回收由调用脚本单独计时。
fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "narrant=info".into()),
        )
        .with_ansi(false)
        .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE)
        .init();
    let arguments: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    assert!(
        arguments.len() >= 3,
        "用法：profile_rust_analysis <before 仓库> <after 仓库> <改动.rs>..."
    );
    let executable = std::env::var_os("NARRANT_RUST_ANALYZER")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join(".tools/rust-analyzer")
        });
    let analyzer = RustLanguageAnalyzer::new(executable);
    let started = Instant::now();
    let before = analyze(&analyzer, "before", &arguments[0], &arguments[2..]);
    let after = analyze(&analyzer, "after", &arguments[1], &arguments[2..]);
    let diff_started = Instant::now();
    let diff = before.diff(&after).expect("图比较失败");
    println!(
        "[rust-analysis-profile] graph_diff_ms={} changed={} total_ms={}",
        diff_started.elapsed().as_millis(),
        diff.nodes.len(),
        started.elapsed().as_millis()
    );
}
