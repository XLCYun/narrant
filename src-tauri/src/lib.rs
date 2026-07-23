pub mod change_content;

use serde::Serialize;
use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

enum OpenedReview {
    Local(change_content::local::LocalOpenedReview),
    GitHub(change_content::github::GitHubOpenedReview),
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileChangeSummaryResponse {
    path: String,
    old_path: Option<String>,
    status: ChangeStatusResponse,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FileChangeContentResponse {
    path: String,
    old_path: Option<String>,
    status: ChangeStatusResponse,
    content_kind: ContentKindResponse,
    before_size: Option<usize>,
    after_size: Option<usize>,
    before: Option<String>,
    after: Option<String>,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum ChangeStatusResponse {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum ContentKindResponse {
    Text,
    Binary,
    Submodule,
    Symlink,
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
enum ReviewSourceResponse {
    Local,
    GitHub,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalReviewSnapshotResponse {
    source: ReviewSourceResponse,
    session_id: String,
    base_ref: String,
    head_ref: String,
    base_revision: String,
    comparison_base: String,
    head_revision: String,
    file_changes: Vec<FileChangeSummaryResponse>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GitHubReviewSnapshotResponse {
    source: ReviewSourceResponse,
    session_id: String,
    owner: String,
    repository: String,
    title: String,
    number: u64,
    author: String,
    state: String,
    draft: bool,
    base_ref: String,
    base_revision: String,
    comparison_base: String,
    head_ref: String,
    head_revision: String,
    description: String,
    changed_files: usize,
    additions: usize,
    deletions: usize,
    file_changes: Vec<FileChangeSummaryResponse>,
    github_url: String,
}

#[derive(Default)]
struct ReviewSessionStore {
    next_id: AtomicU64,
    sessions: Mutex<HashMap<String, Arc<Mutex<OpenedReview>>>>,
}

impl ReviewSessionStore {
    fn insert(&self, review: OpenedReview) -> Result<String, String> {
        let id = format!("session-{}", self.next_id.fetch_add(1, Ordering::Relaxed));
        self.sessions
            .lock()
            .map_err(|_| "Review Session 状态不可用。".to_owned())?
            .insert(id.clone(), Arc::new(Mutex::new(review)));
        Ok(id)
    }

    fn get(&self, id: &str) -> Result<Arc<Mutex<OpenedReview>>, String> {
        self.sessions
            .lock()
            .map_err(|_| "Review Session 状态不可用。".to_owned())?
            .get(id)
            .cloned()
            .ok_or_else(|| "Review Session 已关闭或失效，请重新打开。".to_owned())
    }

    fn close(&self, id: &str) -> Result<(), String> {
        self.sessions
            .lock()
            .map_err(|_| "Review Session 状态不可用。".to_owned())?
            .remove(id);
        Ok(())
    }
}

fn map_status(status: change_content::ChangeStatus) -> ChangeStatusResponse {
    match status {
        change_content::ChangeStatus::Added => ChangeStatusResponse::Added,
        change_content::ChangeStatus::Modified => ChangeStatusResponse::Modified,
        change_content::ChangeStatus::Deleted => ChangeStatusResponse::Deleted,
        change_content::ChangeStatus::Renamed => ChangeStatusResponse::Renamed,
    }
}

fn map_content_kind(kind: change_content::ContentKind) -> ContentKindResponse {
    match kind {
        change_content::ContentKind::Text => ContentKindResponse::Text,
        change_content::ContentKind::Binary => ContentKindResponse::Binary,
        change_content::ContentKind::Submodule => ContentKindResponse::Submodule,
        change_content::ContentKind::Symlink => ContentKindResponse::Symlink,
    }
}

fn map_file_content(change: change_content::LoadedFileChange) -> FileChangeContentResponse {
    FileChangeContentResponse {
        path: change.path,
        old_path: change.old_path,
        status: map_status(change.status),
        content_kind: map_content_kind(change.content_kind),
        before_size: change.before_size,
        after_size: change.after_size,
        before: change.before,
        after: change.after,
    }
}

fn map_local_snapshot(
    session_id: String,
    snapshot: &change_content::local::LocalSnapshot,
) -> LocalReviewSnapshotResponse {
    LocalReviewSnapshotResponse {
        source: ReviewSourceResponse::Local,
        session_id,
        base_ref: snapshot.base_ref.clone(),
        head_ref: snapshot.head_ref.clone(),
        base_revision: snapshot.base_revision.to_string(),
        comparison_base: snapshot.comparison_base.to_string(),
        head_revision: snapshot.head_revision.to_string(),
        file_changes: snapshot
            .file_changes
            .iter()
            .map(|change| FileChangeSummaryResponse {
                path: change.path.clone(),
                old_path: change.old_path.clone(),
                status: map_status(change.status),
            })
            .collect(),
    }
}

fn map_github_snapshot(
    session_id: String,
    snapshot: &change_content::github::GitHubSnapshot,
) -> GitHubReviewSnapshotResponse {
    GitHubReviewSnapshotResponse {
        source: ReviewSourceResponse::GitHub,
        session_id,
        owner: snapshot.owner.clone(),
        repository: snapshot.repository.clone(),
        title: snapshot.title.clone(),
        number: snapshot.number,
        author: snapshot.author.clone(),
        state: snapshot.state.clone(),
        draft: snapshot.draft,
        base_ref: snapshot.base_ref.clone(),
        base_revision: snapshot.base_revision.clone(),
        comparison_base: snapshot.comparison_base.clone(),
        head_ref: snapshot.head_ref.clone(),
        head_revision: snapshot.head_revision.clone(),
        description: snapshot.description.clone(),
        changed_files: snapshot.changed_files,
        additions: snapshot.additions,
        deletions: snapshot.deletions,
        file_changes: snapshot
            .file_changes
            .iter()
            .map(|change| FileChangeSummaryResponse {
                path: change.path.clone(),
                old_path: change.old_path.clone(),
                status: map_status(change.status),
            })
            .collect(),
        github_url: snapshot.github_url.clone(),
    }
}

#[tauri::command]
fn compare_local_refs(
    state: tauri::State<'_, ReviewSessionStore>,
    repository_path: String,
    base_ref: String,
    head_ref: String,
) -> Result<LocalReviewSnapshotResponse, String> {
    let review = change_content::local::LocalOpenedReview::open(
        std::path::Path::new(&repository_path),
        &base_ref,
        &head_ref,
    )
    .map_err(|error| error.to_string())?;
    let response = map_local_snapshot(String::new(), review.snapshot());
    let session_id = state.insert(OpenedReview::Local(review))?;
    Ok(LocalReviewSnapshotResponse {
        session_id,
        ..response
    })
}

#[tauri::command]
fn list_local_refs(repository_path: String) -> Result<Vec<String>, String> {
    change_content::local::list_refs(std::path::Path::new(&repository_path))
        .map_err(|error| error.to_string())
}

fn github_api_base_url() -> String {
    std::env::var("NARRANT_GITHUB_API_URL").unwrap_or_else(|_| "https://api.github.com".to_owned())
}

#[tauri::command]
fn open_github_pull_request(
    state: tauri::State<'_, ReviewSessionStore>,
    url: String,
) -> Result<GitHubReviewSnapshotResponse, String> {
    let review = change_content::github::GitHubOpenedReview::open(github_api_base_url(), &url)
        .map_err(|error| error.to_string())?;
    let response = map_github_snapshot(String::new(), review.snapshot());
    let session_id = state.insert(OpenedReview::GitHub(review))?;
    Ok(GitHubReviewSnapshotResponse {
        session_id,
        ..response
    })
}

#[tauri::command]
fn read_file_change(
    state: tauri::State<'_, ReviewSessionStore>,
    session_id: String,
    path: String,
) -> Result<FileChangeContentResponse, String> {
    let review = state.get(&session_id)?;
    let review = review
        .lock()
        .map_err(|_| "Review Session 状态不可用。".to_owned())?;
    match &*review {
        OpenedReview::Local(review) => review
            .read_file_change(&path)
            .map(map_file_content)
            .map_err(|error| error.to_string()),
        OpenedReview::GitHub(review) => review
            .read_file_change(&path)
            .map(map_file_content)
            .map_err(|error| error.to_string()),
    }
}

#[tauri::command]
fn close_review_session(
    state: tauri::State<'_, ReviewSessionStore>,
    session_id: String,
) -> Result<(), String> {
    state.close(&session_id)
}

pub fn run() {
    let builder = tauri::Builder::default()
        .manage(ReviewSessionStore::default())
        .invoke_handler(tauri::generate_handler![
            compare_local_refs,
            list_local_refs,
            open_github_pull_request,
            read_file_change,
            close_review_session
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_a_review_session_is_idempotent_and_keeps_in_flight_references_alive() {
        let fixture = tempfile::TempDir::new().unwrap();
        let repository = git2::Repository::init(fixture.path()).unwrap();
        let old_blob = repository.blob(b"old\n").unwrap();
        let mut tree_builder = repository.treebuilder(None).unwrap();
        tree_builder
            .insert("README.md", old_blob, 0o100644)
            .unwrap();
        let tree_id = tree_builder.write().unwrap();
        drop(tree_builder);
        let tree = repository.find_tree(tree_id).unwrap();
        let signature = git2::Signature::now("Narrant test", "test@narrant.invalid").unwrap();
        let commit_id = repository
            .commit(Some("HEAD"), &signature, &signature, "initial", &tree, &[])
            .unwrap();
        let commit = repository.find_commit(commit_id).unwrap();
        repository.branch("base", &commit, true).unwrap();
        let new_blob = repository.blob(b"new\n").unwrap();
        let mut tree_builder = repository.treebuilder(Some(&tree)).unwrap();
        tree_builder
            .insert("README.md", new_blob, 0o100644)
            .unwrap();
        let head_tree_id = tree_builder.write().unwrap();
        let head_tree = repository.find_tree(head_tree_id).unwrap();
        let head_id = repository
            .commit(None, &signature, &signature, "head", &head_tree, &[&commit])
            .unwrap();
        let head = repository.find_commit(head_id).unwrap();
        repository.branch("head", &head, true).unwrap();
        drop(head);
        drop(head_tree);
        drop(tree_builder);
        drop(commit);
        drop(tree);
        drop(repository);
        let review =
            change_content::local::LocalOpenedReview::open(fixture.path(), "base", "head").unwrap();
        let response = map_local_snapshot("session-42".to_owned(), review.snapshot());
        let json = serde_json::to_value(&response).unwrap();
        assert_eq!(json["source"], "local");
        assert_eq!(json["sessionId"], "session-42");
        assert_eq!(json["fileChanges"][0]["path"], "README.md");
        assert!(json["fileChanges"][0].get("contentKind").is_none());
        assert!(json.get("snapshotId").is_none());

        let store = ReviewSessionStore::default();
        let id = store.insert(OpenedReview::Local(review)).unwrap();
        let in_flight = store.get(&id).unwrap();

        store.close(&id).unwrap();
        store.close(&id).unwrap();

        assert_eq!(
            store.get(&id).err().unwrap(),
            "Review Session 已关闭或失效，请重新打开。"
        );
        let in_flight = in_flight.lock().unwrap();
        let OpenedReview::Local(review) = &*in_flight else {
            panic!("expected Local Review");
        };
        let change = review.read_file_change("README.md").unwrap();
        assert_eq!(change.before.as_deref(), Some("old\n"));
        assert_eq!(change.after.as_deref(), Some("new\n"));
    }

    #[test]
    fn loaded_file_change_mapper_serializes_every_status_and_content_kind() {
        let statuses = [
            (change_content::ChangeStatus::Added, "added"),
            (change_content::ChangeStatus::Modified, "modified"),
            (change_content::ChangeStatus::Deleted, "deleted"),
            (change_content::ChangeStatus::Renamed, "renamed"),
        ];
        let kinds = [
            (change_content::ContentKind::Text, "text"),
            (change_content::ContentKind::Binary, "binary"),
            (change_content::ContentKind::Submodule, "submodule"),
            (change_content::ContentKind::Symlink, "symlink"),
        ];

        for ((status, expected_status), (kind, expected_kind)) in statuses.into_iter().zip(kinds) {
            let response = map_file_content(change_content::LoadedFileChange {
                path: "new.txt".to_owned(),
                old_path: Some("old.txt".to_owned()),
                status,
                content_kind: kind,
                before_size: Some(3),
                after_size: Some(4),
                before: Some("old".to_owned()),
                after: Some("new\n".to_owned()),
            });
            let json = serde_json::to_value(response).unwrap();
            assert_eq!(json["status"], expected_status);
            assert_eq!(json["contentKind"], expected_kind);
            assert_eq!(json["oldPath"], "old.txt");
            assert_eq!(json["beforeSize"], 3);
            assert_eq!(json["afterSize"], 4);
        }
    }

    #[test]
    fn github_snapshot_mapper_produces_a_tagged_summary_without_content_placeholders() {
        let snapshot = change_content::github::GitHubSnapshot {
            owner: "octo-org".to_owned(),
            repository: "octo-repo".to_owned(),
            title: "Review".to_owned(),
            number: 42,
            author: "octocat".to_owned(),
            state: "open".to_owned(),
            draft: false,
            base_ref: "main".to_owned(),
            base_revision: "base".to_owned(),
            comparison_base: "merge-base".to_owned(),
            head_ref: "topic".to_owned(),
            head_revision: "head".to_owned(),
            description: String::new(),
            changed_files: 1,
            additions: 2,
            deletions: 1,
            file_changes: vec![change_content::github::GitHubFileChange {
                path: "README.md".to_owned(),
                old_path: None,
                status: change_content::ChangeStatus::Modified,
            }],
            github_url: "https://github.com/octo-org/octo-repo/pull/42".to_owned(),
        };

        let response = map_github_snapshot("session-7".to_owned(), &snapshot);
        let json = serde_json::to_value(response).unwrap();
        assert_eq!(json["source"], "github");
        assert_eq!(json["sessionId"], "session-7");
        assert_eq!(json["fileChanges"][0]["path"], "README.md");
        assert!(json["fileChanges"][0].get("contentKind").is_none());
        assert!(json["fileChanges"][0].get("before").is_none());
    }
}
