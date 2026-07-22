use httpmock::prelude::*;
use narrant_lib::change_content::github::{GitHubChangeContent, PullRequestLocation};
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

    let adapter = GitHubChangeContent::new(server.base_url());
    let snapshot = adapter
        .open_pull_request("https://github.com/octo-org/octo-repo/pull/42")
        .expect("open public Pull Request");

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
    assert_eq!(snapshot.file_changes[0].before, None);
    assert_eq!(snapshot.file_changes[0].after, None);
}

#[test]
fn loads_one_file_change_on_demand_from_the_snapshots_pinned_revisions() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/repos/octo-org/octo-repo/pulls/42");
        then.status(200).json_body(serde_json::json!({
            "title": "Update greeting", "number": 42, "user": { "login": "octocat" },
            "state": "open", "draft": true, "merged_at": null,
            "base": { "ref": "main", "sha": "immutable-base" },
            "head": { "ref": "feature", "sha": "immutable-head" },
            "body": null, "changed_files": 1, "additions": 1, "deletions": 1
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/compare/immutable-base...immutable-head");
        then.status(200).json_body(serde_json::json!({
            "merge_base_commit": { "sha": "immutable-comparison-base" }
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/pulls/42/files");
        then.status(200).json_body(serde_json::json!([{
            "filename": "hello world.txt", "status": "modified",
            "additions": 1, "deletions": 1, "changes": 2
        }]));
    });
    let before = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/blobs/old-blob");
        then.status(200).json_body(serde_json::json!({
            "type": "file", "content": "b2xkIGdyZWV0aW5nCg==", "encoding": "base64",
            "size": 13, "sha": "old-blob", "submodule_git_url": null
        }));
    });
    let after = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/blobs/new-blob");
        then.status(200).json_body(serde_json::json!({
            "type": "file", "content": "bmV3IGdyZWV0aW5nCg==", "encoding": "base64",
            "size": 13, "sha": "new-blob", "submodule_git_url": null
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/commits/immutable-comparison-base");
        then.status(200)
            .json_body(serde_json::json!({ "tree": { "sha": "immutable-comparison-base" } }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/repos/octo-org/octo-repo/git/trees/immutable-comparison-base");
        then.status(200).json_body(serde_json::json!({ "truncated": false, "tree": [
            { "path": "hello world.txt", "mode": "100644", "type": "blob", "sha": "old-blob", "size": 13 }
        ]}));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/commits/immutable-head");
        then.status(200)
            .json_body(serde_json::json!({ "tree": { "sha": "immutable-head" } }));
    });
    server.mock(|when, then| {
        when.method(GET).path("/repos/octo-org/octo-repo/git/trees/immutable-head");
        then.status(200).json_body(serde_json::json!({ "truncated": false, "tree": [
            { "path": "hello world.txt", "mode": "100644", "type": "blob", "sha": "new-blob", "size": 13 }
        ]}));
    });
    let adapter = GitHubChangeContent::new(server.base_url());
    let snapshot = adapter
        .open_pull_request("https://github.com/octo-org/octo-repo/pull/42")
        .unwrap();

    assert_eq!(before.hits(), 0);
    assert_eq!(after.hits(), 0);
    let change = adapter
        .read_file_change(&snapshot, "hello world.txt")
        .unwrap();

    before.assert();
    after.assert();
    assert_eq!(change.before.as_deref(), Some("old greeting\n"));
    assert_eq!(change.after.as_deref(), Some("new greeting\n"));
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

    let error = GitHubChangeContent::new(server.base_url())
        .open_pull_request("https://github.com/octo-org/octo-repo/pull/42")
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
        let error = GitHubChangeContent::new(server.base_url())
            .open_pull_request("https://github.com/octo-org/octo-repo/pull/42")
            .unwrap_err();
        assert_eq!(error.to_string(), expected);
    }

    let error = GitHubChangeContent::new("http://127.0.0.1:1".to_owned())
        .open_pull_request("https://github.com/octo-org/octo-repo/pull/42")
        .unwrap_err();
    assert_eq!(error.to_string(), "当前无法连接 GitHub，请检查网络后重试。");
}

#[test]
fn preserves_binary_symlink_and_submodule_content_kinds() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/repos/octo-org/octo-repo/pulls/42");
        then.status(200).json_body(serde_json::json!({
            "title": "Special entries", "number": 42, "user": { "login": "octocat" },
            "state": "open", "draft": false, "merged_at": null,
            "base": { "ref": "main", "sha": "base" }, "head": { "ref": "head", "sha": "head" },
            "body": "", "changed_files": 3, "additions": 1, "deletions": 1
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/compare/base...head");
        then.status(200)
            .json_body(serde_json::json!({ "merge_base_commit": { "sha": "merge-base" } }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/pulls/42/files");
        then.status(200).json_body(serde_json::json!([
            { "filename": "image.bin", "status": "modified" },
            { "filename": "link", "status": "modified" },
            { "filename": "vendor", "status": "modified" }
        ]));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/commits/merge-base");
        then.status(200)
            .json_body(serde_json::json!({ "tree": { "sha": "merge-base" } }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/trees/merge-base");
        then.status(200).json_body(serde_json::json!({
            "truncated": false, "tree": [
                { "path": "image.bin", "mode": "100644", "type": "blob", "sha": "binary", "size": 2 },
                { "path": "link", "mode": "120000", "type": "blob", "sha": "link-blob", "size": 9 },
                { "path": "vendor", "mode": "160000", "type": "commit", "sha": "submodule-commit", "size": null }
            ]
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/commits/head");
        then.status(200)
            .json_body(serde_json::json!({ "tree": { "sha": "head" } }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/trees/head");
        then.status(200).json_body(serde_json::json!({
            "truncated": false, "tree": [
                { "path": "image.bin", "mode": "100644", "type": "blob", "sha": "binary", "size": 2 },
                { "path": "link", "mode": "120000", "type": "blob", "sha": "link-blob", "size": 9 },
                { "path": "vendor", "mode": "160000", "type": "commit", "sha": "submodule-commit", "size": null }
            ]
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/blobs/binary");
        then.status(200).json_body(serde_json::json!({
            "content": "/wE=", "encoding": "base64", "size": 2
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/blobs/link-blob");
        then.status(200).json_body(serde_json::json!({
            "content": "UkVBRE1FLm1k", "encoding": "base64", "size": 9
        }));
    });
    let adapter = GitHubChangeContent::new(server.base_url());
    let snapshot = adapter
        .open_pull_request("https://github.com/octo-org/octo-repo/pull/42")
        .unwrap();

    assert_eq!(
        adapter
            .read_file_change(&snapshot, "image.bin")
            .unwrap()
            .content_kind,
        narrant_lib::change_content::ContentKind::Binary
    );
    assert_eq!(
        adapter
            .read_file_change(&snapshot, "link")
            .unwrap()
            .content_kind,
        narrant_lib::change_content::ContentKind::Symlink
    );
    let submodule = adapter.read_file_change(&snapshot, "vendor").unwrap();
    assert_eq!(
        submodule.content_kind,
        narrant_lib::change_content::ContentKind::Submodule
    );
    assert_eq!(submodule.after.as_deref(), Some("submodule-commit"));
}

#[test]
fn reports_a_404_when_pinned_file_content_is_missing() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/repos/octo-org/octo-repo/pulls/42");
        then.status(200).json_body(serde_json::json!({
            "title": "Missing content", "number": 42, "user": { "login": "octocat" },
            "state": "open", "draft": false, "merged_at": null,
            "base": { "ref": "main", "sha": "base" }, "head": { "ref": "head", "sha": "head" },
            "body": "", "changed_files": 1, "additions": 1, "deletions": 1
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/compare/base...head");
        then.status(200)
            .json_body(serde_json::json!({ "merge_base_commit": { "sha": "merge-base" } }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/pulls/42/files");
        then.status(200)
            .json_body(serde_json::json!([{ "filename": "missing.txt", "status": "modified" }]));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/commits/merge-base");
        then.status(404);
    });
    let adapter = GitHubChangeContent::new(server.base_url());
    let snapshot = adapter
        .open_pull_request("https://github.com/octo-org/octo-repo/pull/42")
        .unwrap();

    assert_eq!(
        adapter
            .read_file_change(&snapshot, "missing.txt")
            .unwrap_err()
            .to_string(),
        "无法读取固定版本中的 File Change（404），请重新打开 Pull Request 后重试。"
    );
}
