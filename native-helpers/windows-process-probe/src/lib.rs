//! Windows 轻量验收直接编译原 LSP、进程树和测试，不复制清理实现或加载 Tauri。
#![cfg(all(test, windows))]
// 原模块包含本组测试未调用的其他公开接口；仍完整编入其全部测试。
#![allow(dead_code)]

#[path = "../../../src-tauri/src/structure_review/code_graph/error.rs"]
mod analysis_error;
#[path = "../../../src-tauri/src/child_process.rs"]
mod child_process;
// 原 LSP 还导出其他语言 Adapter 使用的类型；本 harness 不扩展到这些调用方。
#[allow(unused_imports)]
#[path = "../../../src-tauri/src/lsp/mod.rs"]
mod lsp;
#[path = "../../../src-tauri/src/structure_review/native_analyzer/process_tree.rs"]
mod process_tree;

/// 原 ProcessTree 的错误边界使用同源 AnalysisError，不引入图构建依赖。
type Result<T> = std::result::Result<T, analysis_error::AnalysisError>;

/// 仅补上原 native 模块的文字上下文；所有进程创建、终止与等待仍来自原模块。
fn failure(message: impl std::fmt::Display) -> analysis_error::AnalysisError {
    analysis_error::AnalysisError::new(format!("原生分析器：{message}"))
}

/// 现有 test-only owner seam 所需的命名空间，不加载应用或语言分析器。
mod structure_review {
    /// 将原 ProcessTree 的候选 owner 提供给原 LSP 测试选择器。
    pub(crate) mod native_analyzer {
        pub(crate) use crate::process_tree::ValidationWindowsJob;
    }
}
