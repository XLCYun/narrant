//! Windows 轻量验收直接编译生产进程树及其全部平台回归，不复制清理实现或加载 Tauri。
#![cfg(all(test, windows))]
// 本组仅调用所有者生命周期接口；完整生产模块的其他调用入口仍编入。
#![allow(dead_code)]

#[path = "../../../src-tauri/src/child_process.rs"]
mod child_process;
