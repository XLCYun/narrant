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
    babysit: State<'_, crate::babysit::BabysitApplication>,
    provider: BabysitProvider,
) -> Result<ApplicationSettings, String> {
    update_babysit_provider(&application, &babysit, provider)
}

/// typed provider command 的可测试应用 seam；保存成功后才清理后续分析状态。
fn update_babysit_provider(
    application: &ApplicationSettingsApplication,
    babysit: &crate::babysit::BabysitApplication,
    provider: BabysitProvider,
) -> Result<ApplicationSettings, String> {
    let settings = application
        .update_provider(provider)
        .map_err(|error| error.to_string())?;
    babysit.invalidate_provider_preferences();
    Ok(settings)
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
    update_babysit_preferences(
        &application,
        &babysit,
        provider,
        preference,
        catalog_revision.as_deref(),
    )
    .await
}

/// typed command 的可测试应用 seam：全部校验成功后才执行唯一一次原子写入。
async fn update_babysit_preferences(
    application: &ApplicationSettingsApplication,
    babysit: &crate::babysit::BabysitApplication,
    provider: BabysitProvider,
    mut preference: BabysitProviderSettings,
    catalog_revision: Option<&str>,
) -> Result<ApplicationSettings, crate::babysit::BabysitErrorPayload> {
    if let Some(executable) = preference.executable.as_deref() {
        preference.executable = Some(
            babysit
                .validate_provider_executable(provider, executable)
                .await
                .map_err(crate::babysit::BabysitErrorPayload::from)?,
        );
    }
    match (provider, preference.model.as_ref()) {
        (BabysitProvider::Codex, Some(_)) => {
            let revision = catalog_revision.ok_or_else(|| {
                crate::babysit::BabysitErrorPayload::from(
                    crate::babysit::BabysitError::ModelPreferenceInvalid,
                )
            })?;
            babysit
                .validate_model_preference(
                    provider,
                    revision,
                    &BabysitModelPreference::from(&preference),
                )
                .map_err(crate::babysit::BabysitErrorPayload::from)?;
        }
        (BabysitProvider::ClaudeCode, _) if catalog_revision.is_some() => {
            return Err(crate::babysit::BabysitErrorPayload::from(
                crate::babysit::BabysitError::ModelPreferenceInvalid,
            ));
        }
        _ => {}
    }
    let settings = application
        .update_provider_settings(provider, preference)
        .map_err(|error| {
            let error = match error {
                crate::application_settings::ApplicationSettingsError::InvalidProviderPreference => {
                    crate::babysit::BabysitError::ModelPreferenceInvalid
                }
                _ => crate::babysit::BabysitError::SettingsPersistenceFailed,
            };
            crate::babysit::BabysitErrorPayload::from(error)
        })?;
    babysit.invalidate_provider_preferences();
    Ok(settings)
}

#[cfg(test)]
#[path = "tests/application_settings.rs"]
mod tests;
