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

pub fn run() {
    let builder = tauri::Builder::default().invoke_handler(tauri::generate_handler![
        compare_local_refs,
        list_local_refs,
        read_local_file_change
    ]);
    #[cfg(feature = "wdio")]
    let builder = builder
        .plugin(tauri_plugin_wdio::init())
        .plugin(tauri_plugin_wdio_webdriver::init());
    builder
        .run(tauri::generate_context!())
        .expect("run Narrant");
}
