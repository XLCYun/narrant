pub mod change_content;

#[tauri::command]
fn compare_local_refs(
    repository_path: String,
    base_ref: String,
    head_ref: String,
) -> Result<change_content::ReviewSnapshot, String> {
    change_content::local::compare_refs(
        std::path::Path::new(&repository_path),
        &base_ref,
        &head_ref,
    )
    .map_err(|error| error.to_string())
}

#[tauri::command]
fn list_local_refs(repository_path: String) -> Result<Vec<String>, String> {
    change_content::local::list_refs(std::path::Path::new(&repository_path))
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn read_local_file_change(
    repository_path: String,
    comparison_base: String,
    head_revision: String,
    path: String,
) -> Result<change_content::FileChange, String> {
    change_content::local::read_file_change(
        std::path::Path::new(&repository_path),
        &comparison_base,
        &head_revision,
        &path,
    )
    .map_err(|error| error.to_string())
}

fn github_change_content() -> change_content::github::GitHubChangeContent {
    change_content::github::GitHubChangeContent::new(
        std::env::var("NARRANT_GITHUB_API_URL")
            .unwrap_or_else(|_| "https://api.github.com".to_owned()),
    )
}

#[tauri::command]
fn open_github_pull_request(
    url: String,
) -> Result<change_content::github::GitHubReviewSnapshot, String> {
    github_change_content()
        .open_pull_request(&url)
        .map_err(|error| error.to_string())
}

#[tauri::command]
fn read_github_file_change(
    snapshot: change_content::github::GitHubReviewSnapshot,
    path: String,
) -> Result<change_content::FileChange, String> {
    github_change_content()
        .read_file_change(&snapshot, &path)
        .map_err(|error| error.to_string())
}

pub fn run() {
    let builder = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            compare_local_refs,
            list_local_refs,
            read_local_file_change,
            open_github_pull_request,
            read_github_file_change
        ])
        .plugin(tauri_plugin_opener::init());
    #[cfg(feature = "wdio")]
    let builder = builder
        .plugin(tauri_plugin_wdio::init())
        .plugin(tauri_plugin_wdio_webdriver::init());
    builder
        .run(tauri::generate_context!())
        .expect("run Narrant");
}
