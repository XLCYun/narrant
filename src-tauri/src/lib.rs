mod babysit;
mod core_git;
mod github;
mod operation;
mod persistence;
mod repository;
mod review;
mod tauri_app;

/// 启动 Narrant 桌面应用。
pub fn run() {
    tauri_app::run();
}
