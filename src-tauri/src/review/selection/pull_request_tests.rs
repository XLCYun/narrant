use super::*;
use crate::core_git::CancellationToken;
use crate::operation::OperationContext;
use httpmock::prelude::*;
use std::path::Path;
use std::process::Command;

#[tokio::test]
async fn stable_selection_resolves_current_metadata_and_local_three_dot_snapshot() {
    let fixture = PullRequestResolverFixture::new();
    let server = MockServer::start();
    let observation = server.mock(|when, then| {
        when.method(GET).path("/repos/octo-org/octo-repo/pulls/42");
        then.status(200).json_body(serde_json::json!({
            "number": 42,
            "title": "Resolve behind one seam",
            "body": "Current collaboration metadata",
            "user": { "login": "octocat" },
            "state": "open",
            "merged_at": null,
            "draft": false,
            "html_url": "https://github.com/octo-org/octo-repo/pull/42",
            "updated_at": "2026-08-06T00:00:00Z",
            "base": { "ref": "main", "sha": fixture.base_revision },
            "head": { "ref": "feature", "sha": fixture.head_revision }
        }));
    });
    let resolver = PullRequestSelectionResolver::anonymous(server.base_url());
    let operation = OperationContext::non_cancellable();

    let resolved = resolver
        .resolve(
            fixture.checkout.path(),
            &PullRequestSelection {
                repository: crate::github::GitHubRepository {
                    owner: "octo-org".to_owned(),
                    name: "octo-repo".to_owned(),
                },
                number: 42,
            },
            &operation,
        )
        .await
        .unwrap();

    observation.assert();
    assert_eq!(resolved.metadata.title, "Resolve behind one seam");
    assert_eq!(resolved.comparison.base_revision, fixture.base_revision);
    assert_eq!(resolved.comparison.head_revision, fixture.head_revision);
    assert_eq!(resolved.comparison.comparison_base, fixture.base_revision);
    assert_eq!(resolved.comparison.file_changes.len(), 1);
    assert_eq!(resolved.comparison.file_changes[0].path, "feature.txt");
    assert!(!fixture.checkout.path().join(".git/FETCH_HEAD").exists());
    assert!(!Command::new("git")
        .args(["show-ref", "--verify", "--quiet", "refs/pull/42/head"])
        .current_dir(fixture.checkout.path())
        .status()
        .unwrap()
        .success());
}

#[tokio::test]
async fn cancelled_selection_stops_before_github_observation() {
    let cancellation = CancellationToken::default();
    cancellation.cancel();
    let operation = OperationContext::from_cancellation(cancellation);
    let resolver = PullRequestSelectionResolver::anonymous("http://127.0.0.1:1");

    let error = match resolver
        .resolve(
            Path::new("/definitely/missing/repository"),
            &PullRequestSelection {
                repository: crate::github::GitHubRepository {
                    owner: "octo-org".to_owned(),
                    name: "octo-repo".to_owned(),
                },
                number: 42,
            },
            &operation,
        )
        .await
    {
        Ok(_) => panic!("已取消的 Selection 不应继续解析"),
        Err(error) => error,
    };

    assert!(matches!(error, PullRequestResolutionError::Cancelled(_)));
}

#[tokio::test]
async fn fetched_objects_with_unverifiable_sha_report_exact_revision_failure() {
    let fixture = PullRequestResolverFixture::new();
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/repos/octo-org/octo-repo/pulls/42");
        then.status(200).json_body(serde_json::json!({
            "number": 42,
            "title": "Moved revision",
            "body": "",
            "user": { "login": "octocat" },
            "state": "open",
            "merged_at": null,
            "draft": false,
            "html_url": "https://github.com/octo-org/octo-repo/pull/42",
            "updated_at": "2026-08-06T00:00:00Z",
            "base": { "ref": "main", "sha": fixture.base_revision },
            "head": { "ref": "feature", "sha": "0000000000000000000000000000000000000000" }
        }));
    });
    let resolver = PullRequestSelectionResolver::anonymous(server.base_url());
    let operation = OperationContext::non_cancellable();

    let error = match resolver
        .resolve(
            fixture.checkout.path(),
            &PullRequestSelection {
                repository: crate::github::GitHubRepository {
                    owner: "octo-org".to_owned(),
                    name: "octo-repo".to_owned(),
                },
                number: 42,
            },
            &operation,
        )
        .await
    {
        Ok(_) => panic!("无法验证的精确 revision 不应解析成功"),
        Err(error) => error,
    };

    assert!(matches!(
        error,
        PullRequestResolutionError::ExactRevision { .. }
    ));
}

/// Resolver seam 测试所需的本地 GitHub-like remote 与 checkout。
struct PullRequestResolverFixture {
    _source: tempfile::TempDir,
    checkout: tempfile::TempDir,
    base_revision: String,
    head_revision: String,
}

impl PullRequestResolverFixture {
    /// 创建只通过 refs/pull/42/head 暴露 head commit 的远端。
    fn new() -> Self {
        let source = tempfile::TempDir::new().unwrap();
        run_git(source.path(), &["init", "-q", "-b", "main"]);
        run_git(
            source.path(),
            &["config", "user.email", "test@narrant.invalid"],
        );
        run_git(source.path(), &["config", "user.name", "Narrant Test"]);
        std::fs::write(source.path().join("README.md"), "base\n").unwrap();
        run_git(source.path(), &["add", "README.md"]);
        run_git(source.path(), &["commit", "-q", "-m", "base"]);
        let base_revision = git_text(source.path(), &["rev-parse", "HEAD"]);
        run_git(source.path(), &["checkout", "-q", "-b", "feature"]);
        std::fs::write(source.path().join("feature.txt"), "head\n").unwrap();
        run_git(source.path(), &["add", "feature.txt"]);
        run_git(source.path(), &["commit", "-q", "-m", "head"]);
        let head_revision = git_text(source.path(), &["rev-parse", "HEAD"]);
        run_git(
            source.path(),
            &["update-ref", "refs/pull/42/head", &head_revision],
        );
        run_git(source.path(), &["checkout", "-q", "main"]);
        run_git(source.path(), &["branch", "-D", "feature"]);

        let checkout = tempfile::TempDir::new().unwrap();
        let status = Command::new("git")
            .args(["clone", "-q", "--branch", "main", "--single-branch"])
            .arg(source.path())
            .arg(checkout.path())
            .status()
            .unwrap();
        assert!(status.success());
        run_git(
            checkout.path(),
            &[
                "remote",
                "set-url",
                "origin",
                "https://github.com/octo-org/octo-repo.git",
            ],
        );
        run_git(
            checkout.path(),
            &[
                "remote",
                "add",
                "aaa-broken",
                "git@github.com:octo-org/octo-repo.git",
            ],
        );
        run_git(
            checkout.path(),
            &[
                "config",
                &format!("url.{}.insteadOf", source.path().display()),
                "https://github.com/octo-org/octo-repo.git",
            ],
        );
        run_git(
            checkout.path(),
            &[
                "config",
                "url./definitely/missing/narrant-pull-request-remote.insteadOf",
                "git@github.com:octo-org/octo-repo.git",
            ],
        );
        Self {
            _source: source,
            checkout,
            base_revision,
            head_revision,
        }
    }
}

/// 在测试 repository 中执行必须成功的 Core Git 命令。
fn run_git(repository: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .args(arguments)
        .current_dir(repository)
        .status()
        .unwrap();
    assert!(status.success(), "git {arguments:?} failed");
}

/// 返回单行 Core Git 输出。
fn git_text(repository: &Path, arguments: &[&str]) -> String {
    String::from_utf8(
        Command::new("git")
            .args(arguments)
            .current_dir(repository)
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .to_owned()
}
