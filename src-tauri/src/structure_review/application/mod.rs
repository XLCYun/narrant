//! 单次结构比较：并行准备与分析两侧，借用既有 head，并封装投影与回收。

mod projection;
mod workspace;

use super::code_graph::{AnalysisError, AnalysisInput, AnalysisOptions, CodeGraph, CodeRoot};
use super::rust_analyzer::RustDependencyScope;
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
        Arc, Mutex,
    },
};

/// 每次运行注入只读取消观察，生产和测试都经过同一个语言分析调用点。
pub(crate) type AnalyzeGraph = dyn Fn(&AnalysisInput, CancellationCheck, RustDependencyScope) -> Result<CodeGraph, AnalysisError>
    + Send
    + Sync;

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
        dependency_scope: RustDependencyScope,
    ) -> Result<StructureComparison, StructureReviewError> {
        let lease = lifecycle
            .lease_workspace(operation, review_id, snapshot_id)
            .await
            .map_err(|error| StructureReviewError::new("review-unavailable", error))?;
        let abandoned = Arc::new(AtomicBool::new(false));
        let _await_guard = AnalysisAwaitGuard(abandoned.clone());
        let context = lease
            .operation_context(operation)
            .with_revocation(abandoned);
        let analyze = self.analyze_rust.clone();
        let review_id = review_id.to_owned();
        let snapshot_id = snapshot_id.to_owned();
        tokio::task::spawn_blocking(move || {
            run(
                lease,
                context,
                analyze,
                review_id,
                snapshot_id,
                options,
                dependency_scope,
            )
        })
        .await
        .map_err(|error| StructureReviewError::new("analysis-failed", error))?
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
    dependency_scope: RustDependencyScope,
) -> Result<StructureComparison, StructureReviewError> {
    let (before_files, after_files, coverage) = scope(&lease);
    let failed = Arc::new(AtomicBool::new(false));
    let parallel_operation = operation.with_revocation(failed.clone());
    let first_error = Mutex::new(None);
    let cancelled_signal = parallel_operation.cancellation().clone();
    let cancelled: CancellationCheck = Arc::new(move || cancelled_signal.is_cancelled());
    // before 的签出和分析与 after 的完整分析重叠；scope 保证实际 worker 全部退出。
    let (before, after) = std::thread::scope(|threads| {
        let before = threads.spawn(|| {
            run_worker(&failed, &first_error, || {
                let checkout = if before_files.is_empty() {
                    None
                } else {
                    Some(workspace::BeforeCheckout::prepare(
                        &lease,
                        &parallel_operation,
                    )?)
                };
                let result = analyze_side(
                    &analyze,
                    &lease.before_path,
                    before_files,
                    options,
                    &cancelled,
                    dependency_scope,
                );
                // 目录仅由 before worker 拥有，失败或取消也必须完成回收。
                if let Some(checkout) = checkout {
                    checkout.finish()?;
                }
                result
            })
        });
        let after = run_worker(&failed, &first_error, || {
            analyze_side(
                &analyze,
                &lease.head_path,
                after_files,
                options,
                &cancelled,
                dependency_scope,
            )
        });
        (before.join().expect("worker panic 已转换为分析错误"), after)
    });
    let error = first_error
        .into_inner()
        .expect("worker 不在错误锁内执行分析");
    // 回收失败优先展示，避免一次取消掩盖残留工作副本。
    if let Some(error) = error
        .as_ref()
        .filter(|error| error.code == "cleanup-failed")
    {
        return Err(StructureReviewError::new(error.code, &error.detail));
    }
    if operation.cancellation().is_cancelled() {
        return Err(StructureReviewError::new("cancelled", "结构分析已停止。"));
    }
    if let Some(error) = error {
        return Err(error);
    }
    let (before, after) = (before.unwrap(), after.unwrap());
    workspace::verify_head(&lease, &operation)?;
    let diff = before
        .diff(&after)
        .map_err(|error| StructureReviewError::new("analysis-failed", error.message))?;
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
}

/// 一侧失败或 panic 时通知另一侧停止，保留首个根因并等待资源收敛。
fn run_worker(
    failed: &AtomicBool,
    first_error: &Mutex<Option<StructureReviewError>>,
    work: impl FnOnce() -> Result<CodeGraph, StructureReviewError>,
) -> Option<CodeGraph> {
    let result =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|_| {
            Err(StructureReviewError::new(
                "analysis-failed",
                "结构分析 worker 异常退出。",
            ))
        });
    match result {
        Ok(graph) => Some(graph),
        Err(error) => {
            let mut first = first_error.lock().expect("worker 不在错误锁内执行分析");
            if first.is_none() || error.code == "cleanup-failed" {
                *first = Some(error);
            }
            failed.store(true, Ordering::Release);
            None
        }
    }
}

/// 当前侧没有文件时直接提供空图，避免启动语言服务器或伪造目录。
fn analyze_side(
    analyze: &Arc<AnalyzeGraph>,
    repository: &std::path::Path,
    files: Vec<PathBuf>,
    options: AnalysisOptions,
    cancelled: &CancellationCheck,
    dependency_scope: RustDependencyScope,
) -> Result<CodeGraph, StructureReviewError> {
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
        dependency_scope,
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
