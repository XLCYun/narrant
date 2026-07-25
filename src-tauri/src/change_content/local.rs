use super::*;

pub struct LocalOpenedReview {
    repository: Repository,
    snapshot: LocalSnapshot,
}

impl LocalOpenedReview {
    pub fn open(
        repository_path: &Path,
        base_ref: &str,
        head_ref: &str,
    ) -> Result<Self, git2::Error> {
        let repository = Repository::open(repository_path)?;
        let snapshot = compare_refs(&repository, base_ref, head_ref)?;
        Ok(Self {
            repository,
            snapshot,
        })
    }

    pub fn snapshot(&self) -> &LocalSnapshot {
        &self.snapshot
    }

    pub fn read_file_change(&self, path: &str) -> Result<LoadedFileChange, git2::Error> {
        let stored = self
            .snapshot
            .file_changes
            .iter()
            .find(|change| change.path == path)
            .ok_or_else(|| {
                git2::Error::from_str("File Change is not part of the Review Snapshot")
            })?;
        let before = read_entry(&self.repository, stored.before_entry.as_ref())?;
        let after = read_entry(&self.repository, stored.after_entry.as_ref())?;
        Ok(LoadedFileChange {
            path: stored.path.clone(),
            old_path: stored.old_path.clone(),
            status: stored.status,
            content_kind: super::resolved_content_kind([
                before.as_ref().map(|content| content.kind),
                after.as_ref().map(|content| content.kind),
            ]),
            before_size: before.as_ref().and_then(|entry| entry.size),
            after_size: after.as_ref().and_then(|entry| entry.size),
            before: before.and_then(|entry| entry.text),
            after: after.and_then(|entry| entry.text),
        })
    }
}

pub struct LocalSnapshot {
    pub base_ref: String,
    pub head_ref: String,
    pub base_revision: git2::Oid,
    pub comparison_base: git2::Oid,
    pub head_revision: git2::Oid,
    pub file_changes: Vec<LocalFileChange>,
}

pub struct LocalFileChange {
    pub path: String,
    pub old_path: Option<String>,
    pub status: ChangeStatus,
    before_entry: Option<LocalEntry>,
    after_entry: Option<LocalEntry>,
}

struct LocalEntry {
    object_id: git2::Oid,
    kind: LocalEntryKind,
}

enum LocalEntryKind {
    RegularBlob,
    Symlink,
    Submodule,
}

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

/// 根据两个引用和 Repo，获取 LocalSnapshot，包含两个引用的比较结果
fn compare_refs(
    repository: &Repository,
    base_ref: &str,
    head_ref: &str,
) -> Result<LocalSnapshot, git2::Error> {
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
        file_changes.push(local_file_change(&comparison_base_tree, &head_tree, delta)?);
    }

    Ok(LocalSnapshot {
        base_ref: base_ref.to_owned(),
        head_ref: head_ref.to_owned(),
        base_revision: base_commit.id(),
        comparison_base: comparison_base_id,
        head_revision: head_commit.id(),
        file_changes,
    })
}

/// 获取某个 Delta 的 file change 信息
/// LocalFileChange 不包含实际文件内容，实际内容需要通过 read_entry(LocalEntry) 获取
fn local_file_change(
    comparison_base_tree: &git2::Tree<'_>,
    head_tree: &git2::Tree<'_>,
    delta: git2::DiffDelta<'_>,
) -> Result<LocalFileChange, git2::Error> {
    let path = delta
        .new_file()
        .path()
        .or_else(|| delta.old_file().path())
        .ok_or_else(|| git2::Error::from_str("file change has no path"))?;
    let before_entry = entry_at(comparison_base_tree, delta.old_file().path())?;
    let after_entry = entry_at(head_tree, delta.new_file().path())?;
    Ok(LocalFileChange {
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
        before_entry,
        after_entry,
    })
}

/// 根据路径返回 LocalEntry，路径不存在返回 None
fn entry_at(tree: &git2::Tree<'_>, path: Option<&Path>) -> Result<Option<LocalEntry>, git2::Error> {
    let Some(path) = path else { return Ok(None) };
    let entry = match tree.get_path(path) {
        Ok(entry) => entry,
        Err(_) => return Ok(None),
    };
    if entry.kind() == Some(git2::ObjectType::Commit) {
        return Ok(Some(LocalEntry {
            object_id: entry.id(),
            kind: LocalEntryKind::Submodule,
        }));
    }
    if entry.kind() != Some(git2::ObjectType::Blob) {
        return Ok(None);
    }
    let kind = if entry.filemode() == 0o120000 {
        LocalEntryKind::Symlink
    } else {
        LocalEntryKind::RegularBlob
    };
    Ok(Some(LocalEntry {
        object_id: entry.id(),
        kind,
    }))
}

/// 根据 LocalEntry 中的 Object ID 返回内容：LocalEntry => EntryContent
fn read_entry(
    repository: &Repository,
    entry: Option<&LocalEntry>,
) -> Result<Option<EntryContent>, git2::Error> {
    let Some(entry) = entry else { return Ok(None) };
    if matches!(entry.kind, LocalEntryKind::Submodule) {
        return Ok(Some(EntryContent {
            kind: ContentKind::Submodule,
            size: None,
            text: Some(entry.object_id.to_string()),
        }));
    }
    let blob = repository.find_blob(entry.object_id)?;
    let kind = match entry.kind {
        LocalEntryKind::Symlink => ContentKind::Symlink,
        LocalEntryKind::RegularBlob if blob.is_binary() => ContentKind::Binary,
        LocalEntryKind::RegularBlob => ContentKind::Text,
        LocalEntryKind::Submodule => unreachable!(),
    };
    Ok(Some(EntryContent {
        kind,
        size: Some(blob.size()),
        text: (kind != ContentKind::Binary)
            .then(|| std::str::from_utf8(blob.content()).ok().map(str::to_owned))
            .flatten(),
    }))
}

struct EntryContent {
    kind: ContentKind,
    size: Option<usize>,
    text: Option<String>,
}

/// git2 的底层差异类型转换成自己的 ChangeStatus 枚举
fn status(delta: Delta) -> ChangeStatus {
    match delta {
        Delta::Added => ChangeStatus::Added,
        Delta::Deleted => ChangeStatus::Deleted,
        Delta::Renamed => ChangeStatus::Renamed,
        _ => ChangeStatus::Modified,
    }
}
