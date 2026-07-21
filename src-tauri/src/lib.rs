pub mod change_content;

#[tauri::command]
fn compare_local_refs(repository_path: String, base_ref: String, head_ref: String) -> Result<change_content::ReviewSnapshot, String> {
    change_content::local::compare_refs(std::path::Path::new(&repository_path), &base_ref, &head_ref)
        .map_err(|error| error.to_string())
}

pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![compare_local_refs])
        .run(tauri::generate_context!())
        .expect("run Narrant");
}
