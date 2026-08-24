use crate::application_settings::{
    ApplicationSettings, ApplicationSettingsApplication, BabysitModelPreference, BabysitProvider,
    BabysitProviderSettings, ThemePreference,
};
use tauri::State;

/// 读取当前设备级 Application Settings 快照。
#[tauri::command]
pub(crate) fn load_application_settings(
    application: State<'_, ApplicationSettingsApplication>,
) -> Result<ApplicationSettings, String> {
    application.load().map_err(|error| error.to_string())
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

/// 原子切换未来 Babysit 分析使用的 provider。
#[tauri::command]
pub(crate) fn update_application_babysit_provider(
    application: State<'_, ApplicationSettingsApplication>,
    provider: BabysitProvider,
) -> Result<ApplicationSettings, String> {
    application
        .update_provider(provider)
        .map_err(|error| error.to_string())
}

/// 校验目录 revision（如适用）并原子替换一个 provider 的完整偏好组合。
#[tauri::command]
pub(crate) async fn update_application_babysit_preferences(
    application: State<'_, ApplicationSettingsApplication>,
    babysit: State<'_, crate::babysit::BabysitApplication>,
    provider: BabysitProvider,
    preference: BabysitProviderSettings,
    catalog_revision: Option<String>,
) -> Result<ApplicationSettings, crate::babysit::BabysitErrorPayload> {
    if provider == BabysitProvider::Codex {
        if let Some(revision) = catalog_revision.as_deref() {
            babysit
                .validate_model_preference(revision, &BabysitModelPreference::from(&preference))
                .map_err(crate::babysit::BabysitErrorPayload::from)?;
        }
    }
    let settings = application
        .update_provider_settings(provider, preference)
        .map_err(|_| {
            crate::babysit::BabysitErrorPayload::from(
                crate::babysit::BabysitError::SettingsPersistenceFailed,
            )
        })?;
    if provider == BabysitProvider::Codex {
        babysit.invalidate_codex_executable().await;
    }
    Ok(settings)
}
