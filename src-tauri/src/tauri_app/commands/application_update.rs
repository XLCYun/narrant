use crate::application_update::{ApplicationUpdateRuntime, UpdateInput, UpdateSnapshot};
use std::sync::Arc;

/// 返回 Application Update 唯一权威快照。
#[tauri::command]
pub(crate) fn application_update_snapshot(
    updater: tauri::State<'_, Arc<ApplicationUpdateRuntime>>,
) -> UpdateSnapshot {
    updater.snapshot()
}

/// 使用 Rust 固定 trust configuration 发起手动检查。
#[tauri::command]
pub(crate) fn application_update_manual_check(
    app: tauri::AppHandle,
    updater: tauri::State<'_, Arc<ApplicationUpdateRuntime>>,
) -> UpdateSnapshot {
    updater.inner().dispatch(&app, UpdateInput::ManualCheck);
    updater.snapshot()
}

/// 原子持久化自动下载偏好，并让状态机决定是否启动/取消当前候选下载。
#[tauri::command]
pub(crate) fn application_update_set_automatic_download(
    app: tauri::AppHandle,
    enabled: bool,
    updater: tauri::State<'_, Arc<ApplicationUpdateRuntime>>,
) -> UpdateSnapshot {
    updater
        .inner()
        .dispatch(&app, UpdateInput::SetAutomaticDownload(enabled));
    updater.snapshot()
}

/// 只重放状态机记录的最近一次可重试操作，不接受 URL、路径或签名。
#[tauri::command]
pub(crate) fn application_update_retry(
    app: tauri::AppHandle,
    updater: tauri::State<'_, Arc<ApplicationUpdateRuntime>>,
) -> UpdateSnapshot {
    updater.inner().dispatch(&app, UpdateInput::UserRetry);
    updater.snapshot()
}

/// 请求共享 stop boundary 在成功后安装当前 durable staging 并重启。
#[tauri::command]
pub(crate) fn application_update_restart(
    app: tauri::AppHandle,
    updater: tauri::State<'_, Arc<ApplicationUpdateRuntime>>,
) -> UpdateSnapshot {
    updater
        .inner()
        .dispatch(&app, UpdateInput::RequestUpdateRestart);
    updater.snapshot()
}
