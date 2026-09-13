//! 单次结构比较：借用既有 head，只准备 before，并封装分析、投影与回收。

mod projection;
mod workspace;

use super::code_graph::{AnalysisError, AnalysisInput, AnalysisOptions, CodeGraph, CodeRoot};
use super::observability::measure;
use crate::{
    lsp::CancellationCheck,
    operation::OperationContext,
    review::{ReviewLifecycle, SnapshotWorkspaceLease},
};
pub(crate) use projection::StructureComparison;
use projection::{FileCoverage, Graph, GraphDifference};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Instant,
};

/// 每次运行注入只读取消观察，生产和测试都经过同一个语言分析调用点。
pub(crate) type AnalyzeGraph =
    dyn Fn(&AnalysisInput, CancellationCheck) -> Result<CodeGraph, AnalysisError> + Send + Sync;

/// 面向应用的失败类别，取消与能力缺失不会显示为无结构变化。
#[derive(Debug, Serialize)]
pub(crate) struct StructureReviewError {
    pub code: &'static str,
    pub detail: String,
}

impl StructureReviewError {
    /// 保留原始诊断，同时让前端按稳定类别处理取消。
    fn new(code: &'static str, error: impl std::fmt::Display) -> Self {
        Self {
            code,
            detail: error.to_string(),
        }
    }
}

/// Structure Review 的小 Interface 隐藏 checkout、语言分析及传输投影。
pub(crate) struct StructureReview {
    analyze_rust: Arc<AnalyzeGraph>,
}

impl StructureReview {
    /// 接收语言分析依赖；本版覆盖 Rust，后续语言沿用同一图契约。
    pub fn new(analyze_rust: Arc<AnalyzeGraph>) -> Self {
        Self { analyze_rust }
    }

    /// 分析固定 Snapshot；真正 worker 持有 head 保护，调用方丢弃 future 也不会提前放行刷新。
    pub async fn analyze(
        &self,
        lifecycle: &ReviewLifecycle,
        operation: &OperationContext,
        review_id: &str,
        snapshot_id: &str,
        options: AnalysisOptions,
    ) -> Result<StructureComparison, StructureReviewError> {
        let started = Instant::now();
        let lease_started = Instant::now();
        tracing::info!(target: "narrant::structure_review", event = "stage_started", stage = "workspace_lease", review_id, snapshot_id, "等待结构分析工作区");
        let lease = lifecycle
            .lease_workspace(operation, review_id, snapshot_id)
            .await;
        tracing::info!(target: "narrant::structure_review", event = "stage_finished", stage = "workspace_lease", review_id, snapshot_id,
            elapsed_ms = lease_started.elapsed().as_secs_f64() * 1000.0, success = lease.is_ok(), "结构分析工作区等待结束");
        let lease =
            lease.map_err(|error| StructureReviewError::new("review-unavailable", error))?;
        let queued = Instant::now();
        let abandoned = Arc::new(AtomicBool::new(false));
        let _await_guard = AnalysisAwaitGuard(abandoned.clone());
        let context = lease
            .operation_context(operation)
            .with_revocation(abandoned);
        let analyze = self.analyze_rust.clone();
        let worker_review_id = review_id.to_owned();
        let worker_snapshot_id = snapshot_id.to_owned();
        let result = tokio::task::spawn_blocking(move || {
            let _span = tracing::info_span!(target: "narrant::structure_review", "structure_comparison", review_id = worker_review_id, snapshot_id = worker_snapshot_id).entered();
            tracing::info!(target: "narrant::structure_review", event = "stage_finished", stage = "worker_queue",
                elapsed_ms = queued.elapsed().as_secs_f64() * 1000.0, "结构分析工作线程就绪");
            measure("comparison_worker", || run(lease, context, analyze, worker_review_id, worker_snapshot_id, options))
        })
        .await
        .map_err(|error| StructureReviewError::new("analysis-failed", error))
        .and_then(|result| result);
        tracing::info!(target: "narrant::structure_review", event = "stage_finished", stage = "comparison_total", review_id, snapshot_id,
            elapsed_ms = started.elapsed().as_secs_f64() * 1000.0, success = result.is_ok(), "结构分析结束");
        result
    }
}

/// 外层 future 消失时通知实际 worker 停止，而不是仅丢弃 JoinHandle。
struct AnalysisAwaitGuard(Arc<AtomicBool>);
impl Drop for AnalysisAwaitGuard {
    /// 正常结束时 worker 已完成；异常结束时该标记传播到 Git 和 LSP。
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

/// 同一 Review 的 lease 使固定 before 路径一次仅有一个使用者。
fn run(
    lease: SnapshotWorkspaceLease,
    operation: OperationContext,
    analyze: Arc<AnalyzeGraph>,
    review_id: String,
    snapshot_id: String,
    options: AnalysisOptions,
) -> Result<StructureComparison, StructureReviewError> {
    let (before_files, after_files, coverage) = scope(&lease);
    tracing::info!(target: "narrant::structure_review", event = "comparison_scope", before_files = before_files.len(), after_files = after_files.len(), "结构分析文件范围");
    let mut before_checkout = None;
    let result = (|| {
        if !before_files.is_empty() {
            before_checkout = Some(measure("before_checkout", || {
                workspace::BeforeCheckout::prepare(&lease, &operation)
            })?);
        }
        let cancelled_signal = operation.cancellation().clone();
        let cancelled: CancellationCheck = Arc::new(move || cancelled_signal.is_cancelled());
        let before = analyze_side(
            "before",
            &analyze,
            &lease.before_path,
            before_files,
            options,
            &cancelled,
        )?;
        let after = analyze_side(
            "after",
            &analyze,
            &lease.head_path,
            after_files,
            options,
            &cancelled,
        )?;
        measure("verify_head", || workspace::verify_head(&lease, &operation))?;
        let diff = measure("graph_diff", || before.diff(&after))
            .map_err(|error| StructureReviewError::new("analysis-failed", error.message))?;
        measure("project_graph", || {
            Ok(StructureComparison {
                review_id,
                snapshot_id,
                comparison_base: lease.comparison.comparison_base.clone(),
                head_revision: lease.comparison.head_revision.clone(),
                before: Graph::from(&before),
                after: Graph::from(&after),
                diff: GraphDifference::from(&diff),
                coverage,
            })
        })
    })();
    let cleanup = measure("cleanup_before", || {
        before_checkout.map_or(Ok(()), |checkout| checkout.finish())
    });
    if operation.cancellation().is_cancelled() {
        cleanup?;
        return Err(StructureReviewError::new("cancelled", "结构分析已停止。"));
    }
    cleanup?;
    result
}

/// 当前侧没有文件时直接提供空图，避免启动语言服务器或伪造目录。
fn analyze_side(
    side: &'static str,
    analyze: &Arc<AnalyzeGraph>,
    repository: &std::path::Path,
    files: Vec<PathBuf>,
    options: AnalysisOptions,
    cancelled: &CancellationCheck,
) -> Result<CodeGraph, StructureReviewError> {
    let _span =
        tracing::info_span!(target: "narrant::structure_review", "analysis_side", side).entered();
    if cancelled() {
        return Err(StructureReviewError::new("cancelled", "结构分析已停止。"));
    }
    if files.is_empty() {
        return CodeGraph::new(CodeRoot::default(), Vec::new())
            .map_err(|error| StructureReviewError::new("analysis-failed", error.message));
    }
    analyze(
        &AnalysisInput {
            repository_root: repository.to_owned(),
            files,
            options,
        },
        cancelled.clone(),
    )
    .map_err(|error| StructureReviewError::new("analysis-failed", error.message))
}

/// 分侧选择普通 Rust 源文件，并保留所有改动文件的覆盖说明。
fn scope(lease: &SnapshotWorkspaceLease) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<FileCoverage>) {
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut coverage = Vec::new();
    for change in &lease.comparison.file_changes {
        let before_path = change.old_path.as_deref().unwrap_or(&change.path);
        let old = coverage_kind(before_path, change.before.as_ref());
        let new = coverage_kind(&change.path, change.after.as_ref());
        if old == "analyzed" {
            before.push(before_path.into());
        }
        if new == "analyzed" {
            after.push((&change.path).into());
        }
        coverage.push(FileCoverage {
            path: change.path.clone(),
            old_path: change.old_path.clone(),
            before: old,
            after: new,
        });
    }
    before.sort();
    before.dedup();
    after.sort();
    after.dedup();
    (before, after, coverage)
}

/// 缺失的一侧与不支持的一侧分别报告，不把符号链接或 gitlink 当作源码。
fn coverage_kind(path: &str, reference: Option<&crate::review::FileContentRef>) -> &'static str {
    let Some(reference) = reference else {
        return "absent";
    };
    if !matches!(reference.mode, 0o100644 | 0o100755) {
        return "unsupported-kind";
    }
    if std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        != Some("rs")
    {
        return "unsupported-language";
    }
    "analyzed"
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
