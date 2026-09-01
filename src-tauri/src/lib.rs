mod application_settings;
#[allow(
    dead_code,
    unused_imports,
    reason = "Task 2 只建立核心；Task 3 将把这些契约接入 Tauri 与平台适配器"
)]
mod application_update;
mod babysit;
mod child_process;
mod core_git;
mod github;
mod operation;
mod persistence;
mod repository;
mod review;
mod tauri_app;
pub(crate) mod telemetry;

/// 启动 Narrant 桌面应用。
pub fn run() {
    tauri_app::run();
}
