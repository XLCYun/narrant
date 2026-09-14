//! 单次结构比较：并行准备与分析两侧，借用既有 head，并封装投影与回收。

mod projection;
mod workspace;

use super::code_graph::{AnalysisError, AnalysisInput, AnalysisOptions, CodeGraph, CodeRoot};
use super::rust_analyzer::RustDependencyScope;
use crate::{
    lsp::CancellationCheck,
    operation::{CancellationSource, OperationContext},
    review::{ReviewLifecycle, ReviewLifecycleError, SnapshotWorkspace},
};
pub(crate) use projection::StructureComparison;
use projection::{FileCoverage, Graph, GraphDifference};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
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
        let analyze = self.analyze_rust.clone();
        let result_review_id = review_id.to_owned();
        let result_snapshot_id = snapshot_id.to_owned();
        lifecycle
            .run_in_workspace(operation, review_id, snapshot_id, move |workspace| {
                run(
                    workspace,
                    analyze,
                    result_review_id,
                    result_snapshot_id,
                    options,
                    dependency_scope,
                )
            })
            .await
    }
}

impl From<ReviewLifecycleError> for StructureReviewError {
    /// 取消有独立类别；其余生命周期诊断原样保留。
    fn from(error: ReviewLifecycleError) -> Self {
        let code = if matches!(error, ReviewLifecycleError::Cancelled(_)) {
            "cancelled"
        } else {
            "review-unavailable"
        };
        Self::new(code, error)
    }
}

/// 生命周期执行范围保护目录；两侧 worker 在返回前完成分析与 before 回收。
fn run(
    workspace: &SnapshotWorkspace,
    analyze: Arc<AnalyzeGraph>,
    review_id: String,
    snapshot_id: String,
    options: AnalysisOptions,
    dependency_scope: RustDependencyScope,
) -> Result<StructureComparison, StructureReviewError> {
    let (before_files, after_files, coverage) = scope(workspace);
    let (stop, parallel_operation) = workspace.operation.child();
    let errors = Mutex::new(WorkerErrors::default());
    let cancelled_signal = parallel_operation.cancellation().clone();
    let cancelled: CancellationCheck = Arc::new(move || cancelled_signal.is_cancelled());
    // before 的准备与 after 分析重叠；所有实际子工作和清理都在 scope 内结束。
    let (before, after) = std::thread::scope(|threads| {
        let before = threads.spawn(|| {
            let checkout = if before_files.is_empty() {
                None
            } else {
                Some(run_worker(&stop, &errors, || {
                    workspace::BeforeCheckout::prepare(workspace, &parallel_operation)
                })?)
            };
            // 在 checkout 仍存活时捕获分析器 panic，随后显式清理并收集两个错误。
            let result = run_worker(&stop, &errors, || {
                analyze_side(
                    &analyze,
                    &workspace.before_path,
                    before_files,
                    options,
                    &cancelled,
                    dependency_scope,
                )
            });
            if let Some(checkout) = checkout {
                run_worker(&stop, &errors, || checkout.finish());
            }
            result
        });
        let after = run_worker(&stop, &errors, || {
            analyze_side(
                &analyze,
                &workspace.head_path,
                after_files,
                options,
                &cancelled,
                dependency_scope,
            )
        });
        let before = run_worker(&stop, &errors, || before.join().map_err(panic_error)).flatten();
        (before, after)
    });
    errors
        .into_inner()
        .expect("worker 不在错误锁内执行分析")
        .finish()?;
    if workspace.operation.cancellation().is_cancelled() {
        return Err(StructureReviewError::new("cancelled", "结构分析已停止。"));
    }
    let (before, after) = (before.unwrap(), after.unwrap());
    let diff = before
        .diff(&after)
        .map_err(|error| StructureReviewError::new("analysis-failed", error.message))?;
    Ok(StructureComparison {
        review_id,
        snapshot_id,
        comparison_base: workspace.comparison.comparison_base.clone(),
        head_revision: workspace.comparison.head_revision.clone(),
        before: Graph::from(&before),
        after: Graph::from(&after),
        diff: GraphDifference::from(&diff),
        coverage,
    })
}

/// 诊断与停止信号分离：记录首个真实根因及每次补偿清理错误。
#[derive(Default)]
struct WorkerErrors {
    first_error: Option<StructureReviewError>,
    cleanup_errors: Vec<String>,
}

impl WorkerErrors {
    /// 真正失败优先于伴随取消；清理错误单独积累，不能覆盖根因。
    fn record(&mut self, error: StructureReviewError) {
        if error.code == "cleanup-failed" {
            self.cleanup_errors.push(error.detail);
        } else if self
            .first_error
            .as_ref()
            .is_none_or(|first| first.code == "cancelled" && error.code != "cancelled")
        {
            self.first_error = Some(error);
        }
    }

    /// 清理失败优先报告，附带原始分析失败；单纯取消保留其稳定类别。
    fn finish(self) -> Result<(), StructureReviewError> {
        if !self.cleanup_errors.is_empty() {
            let mut detail = self.cleanup_errors.join("；");
            if let Some(error) = self.first_error {
                detail.push_str(&format!("；原始错误：{}", error.detail));
            }
            // 等待者可能已经消失，仍留下包含根因的清理诊断供排查和恢复。
            tracing::warn!(%detail, "结构分析 before checkout 清理失败");
            return Err(StructureReviewError::new("cleanup-failed", detail));
        }
        self.first_error.map_or(Ok(()), Err)
    }
}

/// 捕获同步阶段的失败或 panic，先记录根因再停止同组工作。
fn run_worker<T>(
    stop: &CancellationSource,
    errors: &Mutex<WorkerErrors>,
    work: impl FnOnce() -> Result<T, StructureReviewError>,
) -> Option<T> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
        .unwrap_or_else(|panic| Err(panic_error(panic)));
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            errors
                .lock()
                .expect("worker 不在错误锁内执行分析")
                .record(error);
            stop.cancel();
            None
        }
    }
}

/// 保留 panic 的文字根因，避免仅报告一个无法定位的 worker 退出。
fn panic_error(panic: Box<dyn std::any::Any + Send>) -> StructureReviewError {
    let detail = panic
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| panic.downcast_ref::<&str>().copied())
        .unwrap_or("无文字诊断");
    StructureReviewError::new(
        "analysis-failed",
        format!("结构分析 worker 异常退出：{detail}"),
    )
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
    .map_err(|error| {
        StructureReviewError::new(
            if error.is_cancelled() {
                "cancelled"
            } else {
                "analysis-failed"
            },
            error.message,
        )
    })
}

/// 分侧选择普通 Rust 源文件，并保留所有改动文件的覆盖说明。
fn scope(workspace: &SnapshotWorkspace) -> (Vec<PathBuf>, Vec<PathBuf>, Vec<FileCoverage>) {
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut coverage = Vec::new();
    for change in &workspace.comparison.file_changes {
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
