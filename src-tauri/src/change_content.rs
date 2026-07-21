use git2::{Delta, DiffFindOptions, DiffOptions, Repository};
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSnapshot {
    pub base_ref: String,
    pub head_ref: String,
    pub base_revision: String,
    pub comparison_base: String,
    pub head_revision: String,
    pub file_changes: Vec<FileChange>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileChange {
    pub path: String,
    pub status: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

pub mod local {
    use super::*;

    pub fn compare_refs(repository_path: &Path, base_ref: &str, head_ref: &str) -> Result<ReviewSnapshot, git2::Error> {
        let repository = Repository::open(repository_path)?;
        let base_commit = repository.revparse_single(base_ref)?.peel_to_commit()?;
        let head_commit = repository.revparse_single(head_ref)?.peel_to_commit()?;
        let comparison_base_id = repository.merge_base(base_commit.id(), head_commit.id())?;
        let comparison_base = repository.find_commit(comparison_base_id)?;
        let base_tree = comparison_base.tree()?;
        let head_tree = head_commit.tree()?;
        let mut options = DiffOptions::new();
        let mut diff = repository.diff_tree_to_tree(Some(&base_tree), Some(&head_tree), Some(&mut options))?;
        let mut find_options = DiffFindOptions::new();
        diff.find_similar(Some(&mut find_options))?;
        let mut file_changes = Vec::new();

        for delta in diff.deltas() {
            let path = delta.new_file().path().or_else(|| delta.old_file().path())
                .ok_or_else(|| git2::Error::from_str("file change has no path"))?;
            file_changes.push(FileChange {
                path: path.to_string_lossy().into_owned(),
                status: status(delta.status()).to_owned(),
                before: text_at(&repository, &base_tree, delta.old_file().path())?,
                after: text_at(&repository, &head_tree, delta.new_file().path())?,
            });
        }

        Ok(ReviewSnapshot {
            base_ref: base_ref.to_owned(),
            head_ref: head_ref.to_owned(),
            base_revision: base_commit.id().to_string(),
            comparison_base: comparison_base_id.to_string(),
            head_revision: head_commit.id().to_string(),
            file_changes,
        })
    }

    fn text_at(repository: &Repository, tree: &git2::Tree<'_>, path: Option<&Path>) -> Result<Option<String>, git2::Error> {
        let Some(path) = path else { return Ok(None) };
        let entry = match tree.get_path(path) { Ok(entry) => entry, Err(_) => return Ok(None) };
        if entry.kind() != Some(git2::ObjectType::Blob) { return Ok(None) }
        let blob = repository.find_blob(entry.id())?;
        Ok(std::str::from_utf8(blob.content()).ok().map(str::to_owned))
    }

    fn status(delta: Delta) -> &'static str {
        match delta {
            Delta::Added => "added",
            Delta::Deleted => "deleted",
            Delta::Renamed => "renamed",
            _ => "modified",
        }
    }
}
