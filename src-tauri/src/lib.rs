pub mod change_content;

use change_content::ChangeContent;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};

#[derive(Clone)]
enum StoredSnapshot {
    Local {
        repository_path: String,
        snapshot: change_content::ReviewSnapshot,
    },
    GitHub(change_content::github::GitHubReviewSnapshot),
}

#[derive(Default)]
struct ChangeContentState {
    next_id: AtomicU64,
    snapshots: Mutex<HashMap<String, StoredSnapshot>>,
}

impl ChangeContentState {
    fn store(&self, snapshot: StoredSnapshot) -> Result<String, String> {
        let id = format!("snapshot-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        self.snapshots
            .lock()
            .map_err(|_| "Review Snapshot 状态不可用。".to_owned())?
            .insert(id.clone(), snapshot);
        Ok(id)
    }

    fn get(&self, id: &str) -> Result<StoredSnapshot, String> {
        self.snapshots
            .lock()
            .map_err(|_| "Review Snapshot 状态不可用。".to_owned())?
            .get(id)
            .cloned()
            .ok_or_else(|| "Review Snapshot 已失效，请重新打开。".to_owned())
    }
}

#[tauri::command]
fn compare_local_refs(
    state: tauri::State<'_, ChangeContentState>,
    repository_path: String,
    base_ref: String,
    head_ref: String,
) -> Result<change_content::ReviewSnapshot, String> {
    let mut snapshot = change_content::local::compare_refs(
        std::path::Path::new(&repository_path),
        &base_ref,
        &head_ref,
    )
    .map_err(|error| error.to_string())?;
    snapshot.snapshot_id = state.store(StoredSnapshot::Local {
        repository_path,
        snapshot: snapshot.clone(),
    })?;
    Ok(snapshot)
}

#[tauri::command]
fn list_local_refs(repository_path: String) -> Result<Vec<String>, String> {
    change_content::local::list_refs(std::path::Path::new(&repository_path))
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
    state: tauri::State<'_, ChangeContentState>,
    url: String,
) -> Result<change_content::github::GitHubReviewSnapshot, String> {
    let mut snapshot = github_change_content()
        .open_pull_request(&url)
        .map_err(|error| error.to_string())?;
    snapshot.snapshot_id = state.store(StoredSnapshot::GitHub(snapshot.clone()))?;
    Ok(snapshot)
}

#[tauri::command]
fn read_file_change(
    state: tauri::State<'_, ChangeContentState>,
    snapshot_id: String,
    path: String,
) -> Result<change_content::FileChange, String> {
    match state.get(&snapshot_id)? {
        StoredSnapshot::Local {
            repository_path,
            snapshot,
        } => {
            let adapter = change_content::local::LocalGitChangeContent::new(repository_path);
            adapter
                .read_file_change(&snapshot, &path)
                .map_err(|error| error.to_string())
        }
        StoredSnapshot::GitHub(snapshot) => github_change_content()
            .read_file_change(&snapshot, &path)
            .map_err(|error| error.to_string()),
    }
}

pub fn run() {
    let builder = tauri::Builder::default()
        .manage(ChangeContentState::default())
        .invoke_handler(tauri::generate_handler![
            compare_local_refs,
            list_local_refs,
            open_github_pull_request,
            read_file_change
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
