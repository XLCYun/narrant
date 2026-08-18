use super::*;
use crate::operation::OperationContext;
use crate::review::{BranchSelection, BranchSelectionResolver};
use std::path::Path;
use std::process::Command;

#[tokio::test]
async fn restored_snapshot_reader_reads_only_the_pinned_file_change() {
    let repository = repository_with_changed_file();
    let operation = OperationContext::background();
    let resolved = BranchSelectionResolver
        .resolve(
            &operation,
            repository.path(),
            &BranchSelection {
                base_ref: "refs/heads/base".to_owned(),
                head_ref: "refs/heads/topic".to_owned(),
            },
        )
        .await
        .unwrap();
    let reader = SnapshotContentReader::restore(repository.path(), resolved.comparison);

    let change = reader.read(&operation, "README.md").unwrap();
    let error = reader.read(&operation, "not-in-snapshot.txt").unwrap_err();

    assert_eq!(
        change.before.as_ref().and_then(FileContent::text),
        Some("before\n")
    );
    assert_eq!(
        change.after.as_ref().and_then(FileContent::text),
        Some("after\n")
    );
    assert!(error
        .to_string()
        .contains("not part of the Review Snapshot"));
}

/// 创建一个包含固定文本 File Change 的真实仓库。
fn repository_with_changed_file() -> tempfile::TempDir {
    let repository = tempfile::TempDir::new().unwrap();
    run_git(repository.path(), &["init", "-q"]);
    run_git(
        repository.path(),
        &["config", "user.email", "test@narrant.invalid"],
    );
    run_git(repository.path(), &["config", "user.name", "Narrant Test"]);
    std::fs::write(repository.path().join("README.md"), "before\n").unwrap();
    run_git(repository.path(), &["add", "README.md"]);
    run_git(repository.path(), &["commit", "-q", "-m", "base"]);
    run_git(repository.path(), &["branch", "base"]);
    std::fs::write(repository.path().join("README.md"), "after\n").unwrap();
    run_git(repository.path(), &["commit", "-q", "-am", "topic"]);
    run_git(repository.path(), &["branch", "topic"]);
    repository
}

/// 执行测试必须成功的 Core Git 命令。
fn run_git(repository: &Path, arguments: &[&str]) {
    assert!(Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .status()
        .unwrap()
        .success());
}
