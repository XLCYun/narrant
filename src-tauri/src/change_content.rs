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
    pub old_path: Option<String>,
    pub status: ChangeStatus,
    pub content_kind: ContentKind,
    pub before_size: Option<usize>,
    pub after_size: Option<usize>,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ContentKind {
    Text,
    Binary,
    Submodule,
    Symlink,
}

pub mod local {
    use super::*;

    pub fn list_refs(repository_path: &Path) -> Result<Vec<String>, git2::Error> {
        let repository = Repository::open(repository_path)?;
        let mut refs = repository
            .branches(None)?
            .map(|branch| {
                let (branch, _) = branch?;
                branch
                    .name()?
                    .map(str::to_owned)
                    .ok_or_else(|| git2::Error::from_str("branch name is not UTF-8"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        refs.sort();
        Ok(refs)
    }

    pub fn compare_refs(
        repository_path: &Path,
        base_ref: &str,
        head_ref: &str,
    ) -> Result<ReviewSnapshot, git2::Error> {
        let repository = Repository::open(repository_path)?;
        let base_commit = repository.revparse_single(base_ref)?.peel_to_commit()?;
        let head_commit = repository.revparse_single(head_ref)?.peel_to_commit()?;
        let comparison_base_id = repository.merge_base(base_commit.id(), head_commit.id())?;
        let comparison_base = repository.find_commit(comparison_base_id)?;
        let comparison_base_tree = comparison_base.tree()?;
        let head_tree = head_commit.tree()?;
        let mut options = DiffOptions::new();
        let mut diff = repository.diff_tree_to_tree(
            Some(&comparison_base_tree),
            Some(&head_tree),
            Some(&mut options),
        )?;
        let mut find_options = DiffFindOptions::new();
        diff.find_similar(Some(&mut find_options))?;
        let mut file_changes = Vec::new();

        for delta in diff.deltas() {
            let path = delta
                .new_file()
                .path()
                .or_else(|| delta.old_file().path())
                .ok_or_else(|| git2::Error::from_str("file change has no path"))?;
            let before = content_at(&repository, &comparison_base_tree, delta.old_file().path())?;
            let after = content_at(&repository, &head_tree, delta.new_file().path())?;
            file_changes.push(FileChange {
                path: path.to_string_lossy().into_owned(),
                old_path: (delta.status() == Delta::Renamed)
                    .then(|| {
                        delta
                            .old_file()
                            .path()
                            .map(|path| path.to_string_lossy().into_owned())
                    })
                    .flatten(),
                status: status(delta.status()),
                content_kind: content_kind(before.as_ref(), after.as_ref()),
                before_size: before.as_ref().and_then(|content| content.size),
                after_size: after.as_ref().and_then(|content| content.size),
                before: None,
                after: None,
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

    pub fn read_file_change(
        repository_path: &Path,
        comparison_base: &str,
        head_revision: &str,
        path: &str,
    ) -> Result<FileChange, git2::Error> {
        let repository = Repository::open(repository_path)?;
        let comparison_base = repository.find_commit(git2::Oid::from_str(comparison_base)?)?;
        let head = repository.find_commit(git2::Oid::from_str(head_revision)?)?;
        let comparison_base_tree = comparison_base.tree()?;
        let head_tree = head.tree()?;
        let mut options = DiffOptions::new();
        let mut diff = repository.diff_tree_to_tree(
            Some(&comparison_base_tree),
            Some(&head_tree),
            Some(&mut options),
        )?;
        let mut find_options = DiffFindOptions::new();
        diff.find_similar(Some(&mut find_options))?;

        for delta in diff.deltas() {
            let new_path = delta.new_file().path();
            let old_path = delta.old_file().path();
            let matches = new_path.is_some_and(|candidate| candidate == Path::new(path))
                || old_path.is_some_and(|candidate| candidate == Path::new(path));
            if !matches {
                continue;
            }
            let before = content_at(&repository, &comparison_base_tree, old_path)?;
            let after = content_at(&repository, &head_tree, new_path)?;
            return Ok(FileChange {
                path: new_path
                    .or(old_path)
                    .ok_or_else(|| git2::Error::from_str("file change has no path"))?
                    .to_string_lossy()
                    .into_owned(),
                old_path: (delta.status() == Delta::Renamed)
                    .then(|| old_path.map(|path| path.to_string_lossy().into_owned()))
                    .flatten(),
                status: status(delta.status()),
                content_kind: content_kind(before.as_ref(), after.as_ref()),
                before_size: before.as_ref().and_then(|content| content.size),
                after_size: after.as_ref().and_then(|content| content.size),
                before: before.and_then(|content| content.text),
                after: after.and_then(|content| content.text),
            });
        }

        Err(git2::Error::from_str(
            "File Change is not part of the Review Snapshot",
        ))
    }

    struct EntryContent {
        kind: ContentKind,
        size: Option<usize>,
        text: Option<String>,
    }

    fn content_at(
        repository: &Repository,
        tree: &git2::Tree<'_>,
        path: Option<&Path>,
    ) -> Result<Option<EntryContent>, git2::Error> {
        let Some(path) = path else { return Ok(None) };
        let entry = match tree.get_path(path) {
            Ok(entry) => entry,
            Err(_) => return Ok(None),
        };
        if entry.kind() == Some(git2::ObjectType::Commit) {
            return Ok(Some(EntryContent {
                kind: ContentKind::Submodule,
                size: None,
                text: Some(entry.id().to_string()),
            }));
        }
        if entry.kind() != Some(git2::ObjectType::Blob) {
            return Ok(None);
        }
        let blob = repository.find_blob(entry.id())?;
        let kind = if entry.filemode() == 0o120000 {
            ContentKind::Symlink
        } else if blob.is_binary() {
            ContentKind::Binary
        } else {
            ContentKind::Text
        };
        Ok(Some(EntryContent {
            kind,
            size: Some(blob.size()),
            text: (kind != ContentKind::Binary)
                .then(|| std::str::from_utf8(blob.content()).ok().map(str::to_owned))
                .flatten(),
        }))
    }

    fn content_kind(before: Option<&EntryContent>, after: Option<&EntryContent>) -> ContentKind {
        [before, after]
            .into_iter()
            .flatten()
            .map(|content| content.kind)
            .max_by_key(|kind| match *kind {
                ContentKind::Submodule => 3,
                ContentKind::Symlink => 2,
                ContentKind::Binary => 1,
                ContentKind::Text => 0,
            })
            .unwrap_or(ContentKind::Text)
    }

    fn status(delta: Delta) -> ChangeStatus {
        match delta {
            Delta::Added => ChangeStatus::Added,
            Delta::Deleted => ChangeStatus::Deleted,
            Delta::Renamed => ChangeStatus::Renamed,
            _ => ChangeStatus::Modified,
        }
    }
}
