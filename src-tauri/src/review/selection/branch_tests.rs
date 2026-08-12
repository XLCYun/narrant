use super::*;
use crate::operation::OperationContext;
use std::path::Path;
use std::process::Command;

#[tokio::test]
async fn stable_branch_selection_resolves_one_immutable_snapshot() {
    let repository = repository_with_branch_comparison();
    let base_revision = git_text(repository.path(), &["rev-parse", "refs/heads/base"]);
    let head_revision = git_text(repository.path(), &["rev-parse", "refs/heads/topic"]);

    let resolved = BranchSelectionResolver
        .resolve(
            repository.path(),
            &BranchSelection {
                base_ref: "refs/heads/base".to_owned(),
                head_ref: "refs/heads/topic".to_owned(),
            },
            &OperationContext::non_cancellable(),
        )
        .await
        .unwrap();

    assert_eq!(resolved.snapshot.base_revision, base_revision);
    assert_eq!(resolved.snapshot.head_revision, head_revision);
    assert_eq!(resolved.snapshot.comparison_base, base_revision);
    assert_eq!(resolved.snapshot.file_changes.len(), 1);
    assert_eq!(resolved.snapshot.file_changes[0].path, "topic.txt");
}

#[tokio::test]
async fn branch_ref_catalog_reports_canonical_refs_and_explicit_choices() {
    let repository = repository_with_branch_comparison();

    let branches = BranchRefCatalog
        .list(repository.path(), &OperationContext::non_cancellable())
        .await
        .unwrap();

    assert_eq!(branches.current_ref, "refs/heads/master");
    assert_eq!(branches.default_ref, "refs/heads/master");
    assert_eq!(
        branches.refs,
        vec!["refs/heads/base", "refs/heads/master", "refs/heads/topic",]
    );
}

#[tokio::test]
async fn cancelled_branch_selection_stops_before_repository_access() {
    let cancellation = crate::core_git::CancellationToken::default();
    cancellation.cancel();
    let operation = OperationContext::from_cancellation(cancellation);

    let error = match BranchSelectionResolver
        .resolve(
            std::path::Path::new("/definitely/missing/repository"),
            &BranchSelection {
                base_ref: "refs/heads/base".to_owned(),
                head_ref: "refs/heads/topic".to_owned(),
            },
            &operation,
        )
        .await
    {
        Ok(_) => panic!("已取消的 Branch Selection 不应继续解析"),
        Err(error) => error,
    };

    assert!(error.is_cancelled());
}

/// 创建 base/topic 分叉所需的真实 Git 仓库。
fn repository_with_branch_comparison() -> tempfile::TempDir {
    let repository = tempfile::TempDir::new().unwrap();
    run_git(repository.path(), &["init", "-q"]);
    run_git(
        repository.path(),
        &["config", "user.email", "test@narrant.invalid"],
    );
    run_git(repository.path(), &["config", "user.name", "Narrant Test"]);
    std::fs::write(repository.path().join("base.txt"), "base\n").unwrap();
    run_git(repository.path(), &["add", "base.txt"]);
    run_git(repository.path(), &["commit", "-q", "-m", "base"]);
    run_git(repository.path(), &["branch", "-M", "master"]);
    run_git(repository.path(), &["branch", "base"]);
    std::fs::write(repository.path().join("topic.txt"), "topic\n").unwrap();
    run_git(repository.path(), &["add", "topic.txt"]);
    run_git(repository.path(), &["commit", "-q", "-m", "topic"]);
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

/// 返回测试断言所需的单行 Core Git 输出。
fn git_text(repository: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
