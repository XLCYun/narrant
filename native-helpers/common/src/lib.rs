//! Rust/Python 已迁移调用者共用的序列化 DTO 与单次控制循环；不包含分析引擎或图校验。
pub mod model;
pub mod one_shot;
pub const PROTOCOL: &str = "narrant.native-analyzer/1";

// 仅复用已有进程所有权原语，保持宿主源码自身的格式边界。
#[allow(dead_code)]
#[rustfmt::skip]
#[path = "../../../src-tauri/src/child_process.rs"]
mod child_process;
