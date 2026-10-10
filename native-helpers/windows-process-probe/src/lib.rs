//! Windows 轻量验收直接编译原 LSP、进程树和测试，不复制清理实现或加载 Tauri。
#![cfg(all(test, windows))]
// 原模块包含本组测试未调用的其他公开接口；仍完整编入其全部测试。
#![allow(dead_code)]

#[path = "../../../src-tauri/src/child_process.rs"]
mod child_process;
// 原 LSP 还导出其他语言 Adapter 使用的类型；本 harness 不扩展到这些调用方。
#[allow(unused_imports)]
#[path = "../../../src-tauri/src/lsp/mod.rs"]
mod lsp;
