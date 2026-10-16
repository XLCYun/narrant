use super::commands::{
    application_settings as application_settings_commands,
    application_update as application_update_commands, babysit as babysit_commands,
    github as github_commands, language_support as language_support_commands, repository,
    review as review_commands, structure_review as structure_review_commands,
};
use super::shutdown::{self, ApplicationShutdown, PostStopCoordinator};
use crate::{
    application_settings::ApplicationSettingsApplication,
    application_update::{ApplicationUpdateRuntime, TrustedUpdaterConfig, UpdateInput},
    babysit, github, operation, persistence, repository as repository_module, review,
};
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_window_state::StateFlags;

use github_commands::github_api_base_url;

/// 装配 Tauri state、commands 与桌面插件并运行应用。
pub fn run() {
    let context = tauri::generate_context!();
    // 两类桌面测试共用隔离边界，在任何用户目录或插件初始化前检查应用身份。
    #[cfg(feature = "test-isolation")]
    assert_eq!(
        context.config().identifier,
        if cfg!(feature = "upgrade-smoke") {
            "com.cyunlab.narrant.upgrade-test"
        } else {
            "com.cyunlab.narrant.e2e"
        },
        "Desktop tests require their isolated application identity"
    );
    #[cfg(feature = "wdio")]
    let oauth_base_url = std::env::var("NARRANT_GITHUB_OAUTH_URL")
        .unwrap_or_else(|_| "https://github.com".to_owned());
    #[cfg(not(feature = "wdio"))]
    let oauth_base_url = "https://github.com".to_owned();
    // 普通界面测试不接受正式更新包，真实安装升级由独立发布验收执行。
    let updater_config = if cfg!(feature = "wdio") {
        TrustedUpdaterConfig::parse(None, None)
    } else {
        TrustedUpdaterConfig::from_build()
    };
    let updater_public_key = updater_config
        .as_ref()
        .map(|config| config.public_key().to_owned())
        .unwrap_or_default();
    let setup_updater_config = updater_config.clone();
    let builder = tauri::Builder::default()
        // Debug 构建安装本地终端日志；Release 保留同一 no-op 生命周期 seam。
        .manage(crate::telemetry::init())
        .manage(operation::CancellableOperations::default())
        .manage(ApplicationShutdown::default())
        .manage(Arc::new(PostStopCoordinator::default()))
        .plugin(
            tauri_plugin_updater::Builder::new()
                .pubkey(updater_public_key)
                .build(),
        );
    #[cfg(feature = "wdio")]
    let builder = builder.manage(repository::E2eRepositoryPicker::default());
    let builder = builder
        .setup(move |app| {
            let application_data = app.path().app_data_dir()?;
            #[cfg(feature = "wdio")]
            let settings_path = std::env::var_os("NARRANT_E2E_SETTINGS_PATH")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| application_data.join("settings.json"));
            #[cfg(not(feature = "wdio"))]
            let settings_path = application_data.join("settings.json");
            #[cfg(feature = "wdio")]
            let database_path = std::env::var_os("NARRANT_DATABASE_PATH")
                .map(std::path::PathBuf::from)
                .unwrap_or(application_data.join("narrant.sqlite3"));
            #[cfg(not(feature = "wdio"))]
            let database_path = application_data.join("narrant.sqlite3");
            let oauth_client = tauri::async_runtime::block_on(async {
                Arc::new(
                    github::OctocrabOAuthClient::new(&oauth_base_url)
                        .await
                        .expect("build GitHub OAuth client"),
                )
            });
            let identity_provider = Arc::new(github::FileGitHubIdentityProvider::new(
                github_api_base_url(),
                application_data.join("github-identity"),
            ));
            let auth = Arc::new(github::GitHubAuth::with_identity_provider(
                option_env!("NARRANT_GITHUB_CLIENT_ID").unwrap_or_default(),
                option_env!("NARRANT_GITHUB_APP_SLUG").unwrap_or_default(),
                oauth_client,
                {
                    #[cfg(feature = "test-isolation")]
                    {
                        Arc::new(github::EphemeralCredentialStore::default())
                    }
                    #[cfg(not(feature = "test-isolation"))]
                    {
                        Arc::new(github::SystemCredentialStore)
                    }
                },
                identity_provider,
            ));
            let store = Arc::new(persistence::ReviewStore::open(&database_path)?);
            let pull_requests = Arc::new(review::PullRequestSelectionResolver::authenticated(
                github_api_base_url(),
                auth.clone(),
            ));
            let repository_application =
                Arc::new(repository_module::RepositoryApplication::new(store.clone()));
            #[cfg(feature = "wdio")]
            if let Ok(repository_path) = std::env::var("NARRANT_E2E_REPOSITORY") {
                // 非 Trust 专项 E2E 在前端首次加载前获得稳定的已授权 fixture。
                let repository = repository_application
                    .register_repository(std::path::Path::new(&repository_path))?;
                repository_application.grant_trust_and_open(repository.repository.id)?;
            }
            let lifecycle = review::ReviewLifecycle::with_repository_application(
                store.clone(),
                repository_application.clone(),
                pull_requests,
            );
            let _ = lifecycle.recover();
            app.manage(lifecycle);
            let support = crate::language_support::LanguageSupportPacks::new(
                application_data.join("language-support-packs"),
            )?;
            app.manage(
                crate::structure_review::StructureReview::new(Arc::new(|_, _, _| {
                    Err(crate::structure_review::code_graph::AnalysisError::new(
                        "Rust 语言支持包尚未准备",
                    ))
                }))
                .with_support_packs(support.clone()),
            );
            app.manage(support);
            app.manage(repository_application);
            app.manage(auth);
            let application_settings = ApplicationSettingsApplication::new(settings_path.clone());
            #[cfg(feature = "upgrade-smoke")]
            super::upgrade_smoke::prepare(app, &application_settings)?;
            app.manage(
                babysit::BabysitApplication::with_installed_provider_registry(
                    settings_path,
                    application_settings.clone(),
                ),
            );
            app.manage(application_settings);
            let updater =
                ApplicationUpdateRuntime::new(app.handle(), setup_updater_config.clone())?;
            app.manage(updater.clone());
            updater.dispatch(app.handle(), UpdateInput::Ready);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            structure_review_commands::analyze_structure_review,
            language_support_commands::language_support_snapshot,
            language_support_commands::language_support_retry,
            language_support_commands::language_support_remove,
            application_settings_commands::load_application_settings,
            application_settings_commands::load_application_custom_connection,
            application_settings_commands::save_application_custom_connection,
            application_settings_commands::update_application_theme,
            application_settings_commands::update_application_language,
            application_settings_commands::update_application_rust_dependency_scope,
            application_settings_commands::update_application_babysit_provider,
            application_settings_commands::update_application_babysit_repository_exploration,
            application_settings_commands::update_application_babysit_all_non_empty_lines,
            application_settings_commands::update_application_babysit_preferences,
            application_update_commands::application_update_snapshot,
            application_update_commands::application_update_manual_check,
            application_update_commands::application_update_set_automatic_download,
            application_update_commands::application_update_retry,
            application_update_commands::application_update_restart,
            #[cfg(feature = "wdio")]
            repository::set_e2e_repository_directory,
            #[cfg(feature = "wdio")]
            repository::choose_e2e_repository_directory,
            repository::load_repository_navigation,
            repository::open_repository_overview,
            repository::grant_repository_workspace_trust_and_open,
            repository::load_repository_reviews,
            repository::register_repository,
            review_commands::compare_local_refs,
            review_commands::check_branch_review_updates,
            review_commands::reopen_and_refresh_branch_review,
            review_commands::refresh_branch_review,
            review_commands::reopen_review,
            repository::list_local_refs,
            repository::cancel_operation,
            github_commands::discover_github_pull_requests,
            review_commands::open_pull_request_review,
            review_commands::check_pull_request_review_update,
            review_commands::refresh_pull_request_review,
            github_commands::github_auth_status,
            github_commands::refresh_github_identity,
            github_commands::start_github_authorization,
            github_commands::complete_github_authorization,
            github_commands::sign_out_github,
            review_commands::read_review_file_change,
            babysit_commands::explain_review_file_change,
            babysit_commands::check_babysit_file_eligibility,
            babysit_commands::list_babysit_models,
            babysit_commands::clear_babysit_snapshot_cache,
            babysit_commands::clear_babysit_review_cache,
            review_commands::close_review
        ])
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        // 仅恢复窗口布局；不恢复可见性，避免正常退出保存的隐藏状态导致下次启动无窗口。
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::SIZE | StateFlags::POSITION | StateFlags::MAXIMIZED)
                .build(),
        );
    #[cfg(feature = "wdio")]
    let builder = builder
        .plugin(tauri_plugin_wdio::init())
        .plugin(tauri_plugin_wdio_webdriver::init());
    let application = builder.build(context).expect("build Narrant");
    application.run(shutdown::handle_run_event);
}
