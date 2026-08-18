use super::*;
use crate::review::PullRequestState;
use std::process::Command;

/// 创建一个带初始提交的真实 working-tree repository。
fn create_repository() -> tempfile::TempDir {
    let fixture = tempfile::TempDir::new().unwrap();
    for arguments in [
        vec!["init", "-q"],
        vec!["config", "user.email", "test@narrant.invalid"],
        vec!["config", "user.name", "Narrant Test"],
    ] {
        assert!(Command::new("git")
            .args(arguments)
            .current_dir(fixture.path())
            .status()
            .unwrap()
            .success());
    }
    std::fs::write(fixture.path().join("README.md"), "fixture").unwrap();
    assert!(Command::new("git")
        .args(["add", "README.md"])
        .current_dir(fixture.path())
        .status()
        .unwrap()
        .success());
    assert!(Command::new("git")
        .args(["commit", "-q", "-m", "initial"])
        .current_dir(fixture.path())
        .status()
        .unwrap()
        .success());
    fixture
}
