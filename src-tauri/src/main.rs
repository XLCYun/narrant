#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

/** 启动 Narrant 桌面应用；Windows release 使用 GUI 子系统以避免额外控制台窗口。 */
fn main() {
    narrant_lib::run();
}
