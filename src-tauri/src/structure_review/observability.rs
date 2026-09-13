//! 结构分析的阶段耗时日志，不记录源码、LSP 参数或服务器响应正文。

use std::time::Instant;

/// 记录同步阶段的起止与结果，失败和取消也保留已花费的时间。
pub(super) fn measure<T, E>(
    stage: &'static str,
    run: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let started = Instant::now();
    tracing::info!(target: "narrant::structure_review", stage, event = "stage_started", "结构分析阶段开始");
    let result = run();
    tracing::info!(target: "narrant::structure_review", stage, event = "stage_finished",
        elapsed_ms = started.elapsed().as_secs_f64() * 1000.0, success = result.is_ok(), "结构分析阶段结束");
    result
}
