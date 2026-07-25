use httpmock::prelude::*;
use narrant_lib::change_content::github::{GitHubOpenedReview, PullRequestLocation};
use narrant_lib::change_content::ChangeStatus;

#[test]
fn accepts_only_complete_standard_github_pull_request_urls() {
    let location = PullRequestLocation::parse("https://github.com/octo-org/octo-repo/pull/42")
        .expect("standard Pull Request URL");

    assert_eq!(location.owner, "octo-org");
    assert_eq!(location.repository, "octo-repo");
    assert_eq!(location.number, 42);

    for invalid in [
        "github.com/octo-org/octo-repo/pull/42",
        "https://www.github.com/octo-org/octo-repo/pull/42",
        "https://github.com/octo-org/octo-repo/pulls/42",
        "https://github.com/octo-org/octo-repo/pull/42/files",
        "https://github.com/octo-org/octo-repo/pull/not-a-number",
        "https://github.com/octo-org/octo-repo/pull/0",
        "https://evil.example/octo-org/octo-repo/pull/42",
    ] {
        assert_eq!(
            PullRequestLocation::parse(invalid).unwrap_err().to_string(),
            "请输入完整的 GitHub Pull Request URL，例如 https://github.com/owner/repo/pull/123"
        );
    }
}

#[test]
fn opens_a_pinned_public_pull_request_and_paginates_its_file_changes() {
    let server = MockServer::start();
    let metadata = server.mock(|when, then| {
        when.method(GET).path("/repos/octo-org/octo-repo/pulls/42");
        then.status(200).json_body(serde_json::json!({
            "title": "Improve greeting",
            "number": 42,
            "user": { "login": "octocat" },
            "state": "closed",
            "draft": false,
            "merged_at": "2026-07-20T10:00:00Z",
            "base": { "ref": "main", "sha": "base-sha" },
            "head": { "ref": "feature", "sha": "head-sha" },
            "body": "## Summary\nHello",
            "changed_files": 2,
            "additions": 5,
            "deletions": 1
        }));
    });
    let first_page = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/pulls/42/files")
            .query_param("per_page", "100")
            .query_param("page", "1");
        then.status(200).json_body(serde_json::json!([{
            "filename": "src/main.rs", "status": "modified", "additions": 3,
            "deletions": 1, "changes": 4
        }]));
    });
    let comparison = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/compare/base-sha...head-sha");
        then.status(200).json_body(serde_json::json!({
            "merge_base_commit": { "sha": "comparison-base-sha" }
        }));
    });
    let second_page = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/pulls/42/files")
            .query_param("per_page", "100")
            .query_param("page", "2");
        then.status(200).json_body(serde_json::json!([{
            "filename": "docs/guide.md", "previous_filename": "guide.md",
            "status": "renamed", "additions": 2, "deletions": 0, "changes": 2
        }]));
    });

    let review = GitHubOpenedReview::open(
        server.base_url(),
        "https://github.com/octo-org/octo-repo/pull/42",
    )
    .expect("open public Pull Request");
    let snapshot = review.snapshot();

    metadata.assert();
    first_page.assert();
    second_page.assert();
    comparison.assert();
    assert_eq!(snapshot.title, "Improve greeting");
    assert_eq!(snapshot.number, 42);
    assert_eq!(snapshot.author, "octocat");
    assert_eq!(snapshot.state, "merged");
    assert!(!snapshot.draft);
    assert_eq!(snapshot.base_ref, "main");
    assert_eq!(snapshot.base_revision, "base-sha");
    assert_eq!(snapshot.comparison_base, "comparison-base-sha");
    assert_eq!(snapshot.head_ref, "feature");
    assert_eq!(snapshot.head_revision, "head-sha");
    assert_eq!(snapshot.description, "## Summary\nHello");
    assert_eq!(snapshot.changed_files, 2);
    assert_eq!(snapshot.additions, 5);
    assert_eq!(snapshot.deletions, 1);
    assert_eq!(snapshot.file_changes.len(), 2);
    assert_eq!(snapshot.file_changes[0].path, "src/main.rs");
    assert_eq!(snapshot.file_changes[0].status, ChangeStatus::Modified);
    assert_eq!(
        snapshot.file_changes[1].old_path.as_deref(),
        Some("guide.md")
    );
    assert_eq!(snapshot.file_changes[1].status, ChangeStatus::Renamed);
}

#[test]
fn rejects_incomplete_reviews_above_githubs_3000_file_limit() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/repos/octo-org/octo-repo/pulls/42");
        then.status(200).json_body(serde_json::json!({
            "title": "Huge change", "number": 42, "user": { "login": "octocat" },
            "state": "open", "draft": false, "merged_at": null,
            "base": { "ref": "main", "sha": "base" }, "head": { "ref": "head", "sha": "head" },
            "body": "", "changed_files": 3001, "additions": 1, "deletions": 1
        }));
    });

    let error = GitHubOpenedReview::open(
        server.base_url(),
        "https://github.com/octo-org/octo-repo/pull/42",
    )
    .unwrap_err()
    .to_string();

    assert_eq!(error, "此 Pull Request 有 3001 个 File Change，超过 GitHub API 的 3000 个上限，无法保证审核完整。请在 GitHub 查看：https://github.com/octo-org/octo-repo/pull/42");
}

#[test]
fn maps_anonymous_github_failures_to_chinese_recovery_messages() {
    for (status, expected) in [
        (
            404,
            "找不到这个公开 Pull Request。请检查 URL，私有仓库需要登录后访问。",
        ),
        (403, "GitHub 匿名访问次数已用尽，请稍后重试或登录后继续。"),
        (500, "GitHub 服务暂时不可用，请稍后重试。"),
    ] {
        let server = MockServer::start();
        server.mock(|when, then| {
            when.method(GET).path("/repos/octo-org/octo-repo/pulls/42");
            then.status(status);
        });
        let error = GitHubOpenedReview::open(
            server.base_url(),
            "https://github.com/octo-org/octo-repo/pull/42",
        )
        .unwrap_err();
        assert_eq!(error.to_string(), expected);
    }

    let error = GitHubOpenedReview::open(
        "http://127.0.0.1:1".to_owned(),
        "https://github.com/octo-org/octo-repo/pull/42",
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "当前无法连接 GitHub，请检查网络后重试。");
}

#[test]
fn authenticated_pull_request_requests_use_bearer_token_and_explain_installation_access() {
    let server = MockServer::start();
    let request = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/private-org/private-repo/pulls/42")
            .header("authorization", "Bearer private-token");
        then.status(403);
    });

    let error = GitHubOpenedReview::open_with_token(
        server.base_url(),
        "https://github.com/private-org/private-repo/pull/42",
        Some("private-token".to_owned()),
    )
    .unwrap_err();

    request.assert();
    assert_eq!(
        error.to_string(),
        "当前账号或 GitHub App 安装范围无权访问此 Pull Request，请检查仓库授权或组织审批。"
    );
}
