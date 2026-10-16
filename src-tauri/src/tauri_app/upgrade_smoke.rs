//! 隔离升级包的数据夹具与读取证据；正式构建不包含此模块。

use crate::application_settings::{
    ApplicationLanguage, ApplicationSettingsApplication, ThemePreference,
};
use crate::repository::RepositoryApplication;
use std::sync::Arc;
use tauri::Manager;

/// 旧版通过真实应用接口初始化夹具；候选版只读取，避免重新创建数据掩盖升级丢失。
pub(super) fn prepare(
    app: &tauri::App,
    settings: &ApplicationSettingsApplication,
) -> Result<(), Box<dyn std::error::Error>> {
    let stage = std::env::var("NARRANT_UPGRADE_TEST_STAGE")?;
    let repositories = app.state::<Arc<RepositoryApplication>>();
    match stage.as_str() {
        "baseline" => {
            let repository = std::env::var_os("NARRANT_UPGRADE_TEST_REPOSITORY")
                .ok_or("upgrade test repository missing")?;
            repositories.register_repository(std::path::Path::new(&repository))?;
            settings.update_theme(ThemePreference::Dark)?;
            settings.update_language(ApplicationLanguage::English)?;
        }
        "candidate" => {}
        _ => return Err("invalid upgrade test stage".into()),
    }
    let snapshot = settings.load()?;
    let directory = app.path().app_data_dir()?;
    let observation = serde_json::json!({
        "stage": stage,
        "version": app.package_info().version.to_string(),
        "identifier": app.config().identifier,
        "dataRoot": directory,
        "theme": snapshot.theme,
        "language": snapshot.language,
        "navigation": repositories.load_navigation()?,
    });
    // 原子发布完整快照，驱动不会读到刚创建但尚未写完的 JSON。
    let temporary = directory.join("upgrade-test-observation.tmp");
    std::fs::write(&temporary, serde_json::to_vec(&observation)?)?;
    std::fs::rename(temporary, directory.join("upgrade-test-observation.json"))?;
    Ok(())
}
