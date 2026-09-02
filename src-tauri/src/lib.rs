mod application_settings;
pub(crate) mod application_update;
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
