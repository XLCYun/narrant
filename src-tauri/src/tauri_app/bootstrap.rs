use super::commands::{
    babysit as babysit_commands, github as github_commands, repository, review as review_commands,
};
use crate::{babysit, github, operation, persistence, repository as repository_module, review};
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_window_state::StateFlags;

use github_commands::github_api_base_url;

/// 装配 Tauri state、commands 与桌面插件并运行应用。
pub fn run() {
    #[cfg(feature = "wdio")]
    let oauth_base_url = std::env::var("NARRANT_GITHUB_OAUTH_URL")
        .unwrap_or_else(|_| "https://github.com".to_owned());
    #[cfg(not(feature = "wdio"))]
    let oauth_base_url = "https://github.com".to_owned();
    let builder = tauri::Builder::default()
        .manage(operation::CancellableOperations::default())
        .setup(move |app| {
            let application_data = app.path().app_data_dir()?;
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
                Arc::new(github::SystemCredentialStore),
                identity_provider,
            ));
            let store = Arc::new(persistence::ReviewStore::open(&database_path)?);
            let pull_requests = Arc::new(review::PullRequestSelectionResolver::authenticated(
                github_api_base_url(),
                auth.clone(),
            ));
            let repository_application =
                Arc::new(repository_module::RepositoryApplication::new(store.clone()));
            let lifecycle = review::ReviewLifecycle::with_repository_application(
                store.clone(),
                repository_application.clone(),
                pull_requests,
            );
            let _ = lifecycle.recover();
            app.manage(lifecycle);
            app.manage(repository_application);
            app.manage(auth);
            app.manage(babysit::BabysitApplication::with_codex_provider_at(
                application_data.join("babysit-settings.json"),
            ));
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
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
            babysit_commands::select_babysit_codex_executable,
            babysit_commands::clear_babysit_snapshot_cache,
            babysit_commands::clear_babysit_review_cache,
            review_commands::close_review
        ])
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        // 仅恢复用户可见的桌面窗口状态，避免意外恢复全屏或其他窗口属性。
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(
                    StateFlags::SIZE
                        | StateFlags::POSITION
                        | StateFlags::MAXIMIZED
                        | StateFlags::VISIBLE,
                )
                .build(),
        );
    #[cfg(feature = "wdio")]
    let builder = builder
        .plugin(tauri_plugin_wdio::init())
        .plugin(tauri_plugin_wdio_webdriver::init());
    let application = builder
        .build(tauri::generate_context!())
        .expect("build Narrant");
    application.run(|app, event| {
        if matches!(
            event,
            tauri::RunEvent::Exit | tauri::RunEvent::ExitRequested { .. }
        ) {
            let operations = app.state::<operation::CancellableOperations>();
            let _ = operations.cancel_all();
            let babysit = app.state::<babysit::BabysitApplication>();
            let _ = tauri::async_runtime::block_on(babysit.shutdown());
            let lifecycle = app.state::<review::ReviewLifecycle>();
            let _ = lifecycle.shutdown();
        }
    });
}
