use crate::application_settings::{
    ApplicationLanguage, ApplicationSettings, ApplicationSettingsApplication,
    BabysitModelPreference, BabysitProvider, BabysitProviderSettings, CustomProviderConnection,
    ThemePreference,
};
use tauri::State;

/// 保存未来结构分析使用的 Rust 依赖范围。
#[tauri::command]
pub(crate) fn update_application_rust_dependency_scope(
    application: State<'_, ApplicationSettingsApplication>,
    scope: crate::structure_review::rust_analyzer::RustDependencyScope,
) -> Result<ApplicationSettings, String> {
    application
        .update_rust_dependency_scope(scope)
        .map_err(|error| error.to_string())
}

/// 读取当前设备级 Application Settings 快照。
#[tauri::command]
pub(crate) fn load_application_settings(
    application: State<'_, ApplicationSettingsApplication>,
) -> Result<ApplicationSettings, String> {
    application.load().map_err(|error| error.to_string())
}

/// 读取完整 Custom 连接；API key 只通过本次内存 IPC 返回给设置编辑器。
#[tauri::command]
pub(crate) fn load_application_custom_connection(
    application: State<'_, ApplicationSettingsApplication>,
) -> Result<CustomProviderConnection, String> {
    read_custom_provider_connection(&application)
}

/// 原子保存完整 Custom 连接，并由应用设置边界负责更新操作系统凭据。
#[tauri::command]
pub(crate) fn save_application_custom_connection(
    application: State<'_, ApplicationSettingsApplication>,
    babysit: State<'_, crate::babysit::BabysitApplication>,
    connection: CustomProviderConnection,
) -> Result<ApplicationSettings, String> {
    save_custom_provider_connection(&application, &babysit, connection)
}

/// typed Custom read command 的可测试应用 seam。
fn read_custom_provider_connection(
    application: &ApplicationSettingsApplication,
) -> Result<CustomProviderConnection, String> {
    application
        .load_custom_connection()
        .map_err(|error| error.to_string())
}

/// typed Custom save command 的可测试应用 seam；完整值只在本次调用内存中流转。
fn save_custom_provider_connection(
    application: &ApplicationSettingsApplication,
    babysit: &crate::babysit::BabysitApplication,
    connection: CustomProviderConnection,
) -> Result<ApplicationSettings, String> {
    let settings = application
        .save_custom_connection(connection)
        .map_err(|error| error.to_string())?;
    babysit.invalidate_provider_preferences();
    Ok(settings)
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

/// 原子更新 Application Language，并返回持久化后的完整设置快照。
#[tauri::command]
pub(crate) fn update_application_language(
    application: State<'_, ApplicationSettingsApplication>,
    language: ApplicationLanguage,
) -> Result<ApplicationSettings, String> {
    update_language(&application, language)
}

/// typed language command 的可测试应用 seam。
fn update_language(
    application: &ApplicationSettingsApplication,
    language: ApplicationLanguage,
) -> Result<ApplicationSettings, String> {
    application
        .update_language(language)
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

/// 保存解释生成范围；既有缓存与运行不受影响。
#[tauri::command]
pub(crate) fn update_application_babysit_all_non_empty_lines(
    application: State<'_, ApplicationSettingsApplication>,
    enabled: bool,
) -> Result<ApplicationSettings, String> {
    application
        .update_all_non_empty_lines(enabled)
        .map_err(|error| error.to_string())
}

/// 原子保存未来 Babysit 分析是否允许探索 Review Workspace；既有缓存与运行不受影响。
#[tauri::command]
pub(crate) fn update_application_babysit_repository_exploration(
    application: State<'_, ApplicationSettingsApplication>,
    enabled: bool,
) -> Result<ApplicationSettings, String> {
    application
        .update_repository_exploration(enabled)
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
