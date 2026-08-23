use crate::application_settings::{
    ApplicationSettings, ApplicationSettingsApplication, BabysitModelPreference, ThemePreference,
};
use std::path::PathBuf;
use tauri::State;

/// 读取当前设备级 Application Settings 快照。
#[tauri::command]
pub(crate) fn load_application_settings(
    application: State<'_, ApplicationSettingsApplication>,
) -> Result<ApplicationSettings, String> {
    application.load().map_err(|error| error.to_string())
}

/// 原子保存 automatic/explicit Babysit Model Preference。
#[tauri::command]
pub(crate) async fn update_application_babysit_model(
    application: State<'_, ApplicationSettingsApplication>,
    babysit: State<'_, crate::babysit::BabysitApplication>,
    preference: BabysitModelPreference,
    workspace_path: String,
) -> Result<ApplicationSettings, String> {
    let catalog = babysit
        .fetch_model_catalog(std::path::Path::new(&workspace_path))
        .await
        .map_err(|error| error.to_string())?;
    catalog.resolve(&preference).map_err(|_| {
        crate::babysit::BabysitError::ModelPreferenceInvalid.to_string()
    })?;
    application
        .update_babysit_model(preference)
        .map_err(|error| error.to_string())
}

/// 原子更新主题偏好，并返回持久化后的完整设置快照。
#[tauri::command]
pub(crate) fn update_application_theme(
    application: State<'_, ApplicationSettingsApplication>,
    theme: ThemePreference,
) -> Result<ApplicationSettings, String> {
    application
        .update_theme(theme)
        .map_err(|error| error.to_string())
}

/// 验证并原子保存显式 Codex Executable Preference。
#[tauri::command]
pub(crate) async fn update_application_codex_executable(
    application: State<'_, ApplicationSettingsApplication>,
    executable: String,
) -> Result<ApplicationSettings, String> {
    application
        .update_codex_executable(Some(PathBuf::from(executable)))
        .await
        .map_err(|error| error.to_string())
}

/// 清空显式 Codex Executable Preference，恢复自动发现。
#[tauri::command]
pub(crate) async fn clear_application_codex_executable(
    application: State<'_, ApplicationSettingsApplication>,
) -> Result<ApplicationSettings, String> {
    application
        .update_codex_executable(None)
        .await
        .map_err(|error| error.to_string())
}
