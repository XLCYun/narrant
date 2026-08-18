use crate::core_git::GitCommandError;
use crate::operation::OperationContext;
use crate::review::{ChangeComparison, SnapshotContentReader};
use crate::review::{FileChangeContent, FileChangeStatus, FileContent, FileContentKind};
use crate::review::{
    BranchRefCatalog, BranchRefs, BranchSelection, BranchSelectionResolutionError,
    BranchSelectionResolver,
};
use git2::{build::CheckoutBuilder, Repository, Signature};
use httpmock::prelude::*;
use std::process::Command;
use tempfile::TempDir;

fn commit_file(repository: &Repository, path: &str, contents: &str, message: &str) -> git2::Oid {
    commit_bytes(repository, path, contents.as_bytes(), message)
}

fn commit_bytes(repository: &Repository, path: &str, contents: &[u8], message: &str) -> git2::Oid {
    let workdir = repository.workdir().expect("fixture has a worktree");
    std::fs::write(workdir.join(path), contents).expect("write fixture file");
    let mut index = repository.index().expect("open index");
    index
        .add_path(std::path::Path::new(path))
        .expect("stage file");
    index.write().expect("write index");
    let tree_id = index.write_tree().expect("write tree");
    let tree = repository.find_tree(tree_id).expect("find tree");
    let signature = Signature::now("Narrant test", "test@narrant.invalid").expect("signature");
    let parents = repository
        .head()
        .ok()
        .and_then(|head| head.peel_to_commit().ok())
        .into_iter()
        .collect::<Vec<_>>();
    repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parents.iter().collect::<Vec<_>>(),
        )
        .expect("commit")
}

fn commit_tree_entry(
    repository: &Repository,
    path: &str,
    object_id: git2::Oid,
    filemode: i32,
    message: &str,
) -> git2::Oid {
    let parent = repository.head().unwrap().peel_to_commit().unwrap();
    let parent_tree = parent.tree().unwrap();
    let mut builder = repository.treebuilder(Some(&parent_tree)).unwrap();
    builder.insert(path, object_id, filemode).unwrap();
    let tree_id = builder.write().unwrap();
    let tree = repository.find_tree(tree_id).unwrap();
    let signature = Signature::now("Narrant test", "test@narrant.invalid").unwrap();
    repository
        .commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &[&parent],
        )
        .unwrap()
}

/// 测试中绑定已解析快照及其只读内容入口。
struct ResolvedComparison {
    comparison: ChangeComparison,
    reader: SnapshotContentReader,
}

impl ResolvedComparison {
    /// 从固定快照读取指定文件变更。
    fn read(&self, path: &str) -> Result<FileChangeContent, GitCommandError> {
        self.reader
            .read(path, &OperationContext::background())
    }
}

/// 在独立运行时中执行异步 Git seam。
fn block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Runtime::new().unwrap().block_on(future)
}

/// 通过分支选择 seam 解析固定比较。
fn resolve_comparison(
    repository_path: &std::path::Path,
    base_ref: &str,
    head_ref: &str,
) -> Result<ResolvedComparison, BranchSelectionResolutionError> {
    let resolved = block_on(BranchSelectionResolver.resolve(
        repository_path,
        &BranchSelection {
            base_ref: base_ref.to_owned(),
            head_ref: head_ref.to_owned(),
        },
        &OperationContext::background(),
    ))?;
    let comparison = resolved.comparison;
    let reader = SnapshotContentReader::restore(repository_path, comparison.clone());
    Ok(ResolvedComparison { comparison, reader })
}

/// 打开 fixture 的标准 base/head 比较。
fn open_comparison(repository_path: &std::path::Path) -> ResolvedComparison {
    resolve_comparison(repository_path, "refs/heads/base", "refs/heads/head").unwrap()
}

/// 通过分支目录 seam 读取仓库 refs。
fn branch_refs(repository_path: &std::path::Path) -> Result<BranchRefs, GitCommandError> {
    block_on(BranchRefCatalog.list(repository_path, &OperationContext::background()))
}
