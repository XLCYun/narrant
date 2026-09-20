//! 单次结构比较：并行准备与分析两侧，借用既有 head，并封装投影与回收。

mod projection;
mod workspace;

use super::code_graph::{AnalysisError, AnalysisInput, AnalysisOptions, CodeGraph, CodeRoot};
use super::rust_analyzer::RustDependencyScope;
use crate::{
    lsp::CancellationCheck,
    operation::OperationContext,
    review::{ReviewLifecycle, ReviewLifecycleError, SnapshotWorkspace},
};
pub(crate) use projection::StructureComparison;
use projection::{FileCoverage, Graph, GraphDifference};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tokio_util::sync::CancellationToken;

/// 每次运行注入只读取消观察，生产和测试都经过同一个语言分析调用点。
pub(crate) type AnalyzeGraph = dyn Fn(&AnalysisInput, CancellationCheck, RustDependencyScope) -> Result<CodeGraph, AnalysisError>
    + Send
    + Sync;

/// TypeScript/JavaScript 共用一次语义项目并返回代码图。
pub(crate) type AnalyzeTypeScript =
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

/// 固定的一组语言分析入口；运行时只克隆闭包，不重新读取工具设置。
#[derive(Clone)]
struct LanguageAnalyzers {
    rust: Arc<AnalyzeGraph>,
    typescript: Arc<AnalyzeTypeScript>,
}

/// Structure Review 的小 Interface 隐藏 checkout、语言分析及传输投影。
pub(crate) struct StructureReview {
    analyzers: LanguageAnalyzers,
}

impl StructureReview {
    /// 接收固定的 Rust 分析依赖；测试和最小宿主可以保留既有单语言装配。
    pub fn new(analyze_rust: Arc<AnalyzeGraph>) -> Self {
        Self {
            analyzers: LanguageAnalyzers {
                rust: analyze_rust,
                typescript: Arc::new(|_, _| Err(AnalysisError::new("TypeScript 分析器尚未装配"))),
            },
        }
    }

    /// 装配共享的 TypeScript/JavaScript 分析入口；非空语言组才会调用它。
    pub fn with_typescript(mut self, analyze_typescript: Arc<AnalyzeTypeScript>) -> Self {
        self.analyzers.typescript = analyze_typescript;
        self
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
        let analyzers = self.analyzers.clone();
        let result_review_id = review_id.to_owned();
        let result_snapshot_id = snapshot_id.to_owned();
        lifecycle
            .run_in_workspace(operation, review_id, snapshot_id, move |workspace| {
                run(
                    workspace,
                    analyzers,
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
    analyzers: LanguageAnalyzers,
    review_id: String,
    snapshot_id: String,
    options: AnalysisOptions,
    dependency_scope: RustDependencyScope,
) -> Result<StructureComparison, StructureReviewError> {
    let (before_files, after_files, coverage) = scope(workspace);
    let parallel_operation = workspace.operation.child();
    let stop = parallel_operation.cancellation();
    let errors = Mutex::new(WorkerErrors::default());
    let cancelled_signal = parallel_operation.cancellation().clone();
    let cancelled: CancellationCheck = Arc::new(move || cancelled_signal.is_cancelled());
    // before 的准备与 after 分析重叠；所有实际子工作和清理都在 scope 内结束。
    let (before, after) = std::thread::scope(|threads| {
        let before = threads.spawn(|| {
            let checkout = if before_files.is_empty() {
                None
            } else {
                Some(run_worker(stop, &errors, || {
                    workspace::BeforeCheckout::prepare(workspace, &parallel_operation)
                })?)
            };
            // 在 checkout 仍存活时捕获分析器 panic，随后显式清理并收集两个错误。
            let result = run_worker(stop, &errors, || {
                analyze_side(
                    &analyzers,
                    &workspace.before_path,
                    before_files,
                    options,
                    &cancelled,
                    dependency_scope,
                    stop,
                )
            });
            if let Some(checkout) = checkout {
                run_worker(stop, &errors, || checkout.finish());
            }
            result
        });
        let after = run_worker(stop, &errors, || {
            analyze_side(
                &analyzers,
                &workspace.head_path,
                after_files,
                options,
                &cancelled,
                dependency_scope,
                stop,
            )
        });
        let before = run_worker(stop, &errors, || before.join().map_err(panic_error)).flatten();
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
    stop: &CancellationToken,
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
    analyzers: &LanguageAnalyzers,
    repository: &std::path::Path,
    files: SideFiles,
    options: AnalysisOptions,
    cancelled: &CancellationCheck,
    dependency_scope: RustDependencyScope,
    stop: &CancellationToken,
) -> Result<CodeGraph, StructureReviewError> {
    if cancelled() {
        return Err(StructureReviewError::new("cancelled", "结构分析已停止。"));
    }
    if files.is_empty() {
        return empty_graph();
    }
    let rust_input = (!files.rust.is_empty()).then(|| AnalysisInput {
        repository_root: repository.to_owned(),
        files: files.rust,
        options,
    });
    let typescript_input = (!files.typescript.is_empty()).then(|| AnalysisInput {
        repository_root: repository.to_owned(),
        files: files.typescript,
        options,
    });
    let errors = Mutex::new(WorkerErrors::default());
    let (rust, typescript) = std::thread::scope(|threads| {
        let rust = rust_input.map(|input| {
            let errors = &errors;
            threads.spawn(move || {
                run_worker(stop, errors, || {
                    (analyzers.rust)(&input, cancelled.clone(), dependency_scope)
                        .map_err(analysis_error)
                })
            })
        });
        let typescript = typescript_input.and_then(|input| {
            run_worker(stop, &errors, || {
                (analyzers.typescript)(&input, cancelled.clone()).map_err(analysis_error)
            })
        });
        let rust = rust.and_then(|worker| {
            run_worker(stop, &errors, || worker.join().map_err(panic_error)).flatten()
        });
        (rust, typescript)
    });
    errors
        .into_inner()
        .expect("语言 worker 不在错误锁内执行分析")
        .finish()?;
    let mut graph = empty_graph()?;
    for next in [rust, typescript].into_iter().flatten() {
        graph = graph
            .merge(next)
            .map_err(|error| StructureReviewError::new("analysis-failed", error.message))?;
    }
    Ok(graph)
}

/// 将语言分析错误映射到稳定应用类别，并保留适配器提供的原始原因。
fn analysis_error(error: AnalysisError) -> StructureReviewError {
    StructureReviewError::new(
        if error.is_cancelled() {
            "cancelled"
        } else {
            "analysis-failed"
        },
        error.message,
    )
}

/// 构造不启动任何语言工具的空图。
fn empty_graph() -> Result<CodeGraph, StructureReviewError> {
    CodeGraph::new(CodeRoot::default(), Vec::new())
        .map_err(|error| StructureReviewError::new("analysis-failed", error.message))
}

/// 一侧的文件按 Rust 和共享 TS/JS 语义项目分组。
#[derive(Default)]
struct SideFiles {
    rust: Vec<PathBuf>,
    typescript: Vec<PathBuf>,
}

impl SideFiles {
    /// 没有支持文件时跳过 checkout 与全部语言工具。
    fn is_empty(&self) -> bool {
        self.rust.is_empty() && self.typescript.is_empty()
    }

    /// 按路径对应的语言组加入输入，未知扩展保持在 coverage 中。
    fn push(&mut self, language: Option<LanguageGroup>, path: PathBuf) {
        match language {
            Some(LanguageGroup::Rust) => self.rust.push(path),
            Some(LanguageGroup::TypeScript) => self.typescript.push(path),
            None => {}
        }
    }

    /// 为分析器提供稳定且去重的文件次序。
    fn sort_and_dedup(&mut self) {
        self.rust.sort();
        self.rust.dedup();
        self.typescript.sort();
        self.typescript.dedup();
    }
}

/// 应用层只需要两个明确语言组，不引入通用插件注册表。
#[derive(Clone, Copy, PartialEq, Eq)]
enum LanguageGroup {
    Rust,
    TypeScript,
}

/// 分侧选择支持的源码文件，并保留所有改动文件的覆盖说明。
fn scope(workspace: &SnapshotWorkspace) -> (SideFiles, SideFiles, Vec<FileCoverage>) {
    let mut before = SideFiles::default();
    let mut after = SideFiles::default();
    let mut coverage = Vec::new();
    for change in &workspace.comparison.file_changes {
        let before_path = change.old_path.as_deref().unwrap_or(&change.path);
        let old = coverage_kind(before_path, change.before.as_ref());
        let new = coverage_kind(&change.path, change.after.as_ref());
        before.push(
            language_group(before_path, change.before.as_ref()),
            before_path.into(),
        );
        after.push(
            language_group(&change.path, change.after.as_ref()),
            (&change.path).into(),
        );
        coverage.push(FileCoverage {
            path: change.path.clone(),
            old_path: change.old_path.clone(),
            before: old,
            after: new,
        });
    }
    before.sort_and_dedup();
    after.sort_and_dedup();
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
    language_group(path, Some(reference)).map_or("unsupported-language", |_| "analyzed")
}

/// 普通文件按支持扩展路由；测试文件仍进入分析器，由 include_tests 决定是否产出对象。
fn language_group(
    path: &str,
    reference: Option<&crate::review::FileContentRef>,
) -> Option<LanguageGroup> {
    if !matches!(reference?.mode, 0o100644 | 0o100755) {
        return None;
    }
    match std::path::Path::new(path).extension()?.to_str()? {
        "rs" => Some(LanguageGroup::Rust),
        "ts" | "tsx" | "js" | "jsx" | "mts" | "cts" | "mjs" | "cjs" => {
            Some(LanguageGroup::TypeScript)
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
