use httpmock::prelude::*;
use narrant_lib::github_discovery::discover_pull_requests;
use secrecy::SecretString;
use std::process::Command;
use tempfile::TempDir;

/// 创建带指定 remote 的最小 Git 仓库，作为公开 discovery seam 的输入。
fn repository_with_remotes(remotes: &[(&str, &str)]) -> TempDir {
    let directory = TempDir::new().unwrap();
    assert!(Command::new("git")
        .args(["init", directory.path().to_str().unwrap()])
        .status()
        .unwrap()
        .success());
    for (name, url) in remotes {
        assert!(Command::new("git")
            .args([
                "-C",
                directory.path().to_str().unwrap(),
                "remote",
                "add",
                name,
                url
            ])
            .status()
            .unwrap()
            .success());
    }
    directory
}

#[test]
fn discovers_all_remotes_deduplicates_repositories_and_paginates_metadata_only() {
    let server = MockServer::start();
    let first_page = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/pulls")
            .query_param("state", "all")
            .query_param("per_page", "100")
            .query_param("page", "1");
        then.status(200).json_body(serde_json::json!([{
            "number": 42,
            "title": "Keep metadata local",
            "body": "Description",
            "user": { "login": "octocat" },
            "state": "open",
            "draft": false,
            "html_url": "https://github.com/octo-org/octo-repo/pull/42",
            "base": { "ref": "main", "sha": "base-sha" },
            "head": { "ref": "feature", "sha": "head-sha" }
        }]));
    });
    let second_page = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/pulls")
            .query_param("page", "2");
        then.status(200).json_body(serde_json::json!([]));
    });
    let repository = repository_with_remotes(&[
        ("origin", "https://github.com/octo-org/octo-repo.git"),
        ("upstream", "git@github.com:octo-org/octo-repo.git"),
        ("ssh-host", "ssh://github.com/octo-org/octo-repo.git"),
        ("ssh-port", "ssh://git@github.com:22/octo-org/octo-repo.git"),
        ("mirror", "ssh://git@github.com/fork-owner/fork-repo.git"),
        ("elsewhere", "https://example.com/octo-org/octo-repo.git"),
    ]);
    server.mock(|when, then| {
        when.method(GET).path("/repos/fork-owner/fork-repo/pulls");
        then.status(403);
    });

    let groups = discover_pull_requests(repository.path(), &server.base_url(), None).unwrap();

    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].repository.identity(), "fork-owner/fork-repo");
    assert!(groups[0].pull_requests.is_empty());
    assert!(groups[0].error.is_some());
    assert_eq!(groups[1].repository.identity(), "octo-org/octo-repo");
    assert_eq!(
        groups[1].remote_names,
        vec!["origin", "ssh-host", "ssh-port", "upstream"]
    );
    let pull_request = &groups[1].pull_requests[0];
    assert_eq!(pull_request.number, 42);
    assert_eq!(pull_request.base.sha, "base-sha");
    assert_eq!(pull_request.head.sha, "head-sha");
    first_page.assert();
    second_page.assert();
}

#[test]
fn rejects_code_content_endpoints_by_only_listing_pull_requests() {
    let server = MockServer::start();
    let list = server.mock(|when, then| {
        when.method(GET).path("/repos/octo-org/octo-repo/pulls");
        then.status(200).json_body(serde_json::json!([]));
    });
    let forbidden = server.mock(|when, then| {
        when.path_matches(
            Regex::new(r"^/repos/.+/(contents|trees|git/blobs|compare|pulls/\d+/files)").unwrap(),
        );
        then.status(500);
    });
    let repository =
        repository_with_remotes(&[("origin", "https://github.com/octo-org/octo-repo")]);

    discover_pull_requests(repository.path(), &server.base_url(), None).unwrap();

    list.assert();
    assert_eq!(forbidden.hits(), 0);
}

#[test]
fn uses_authenticated_metadata_access_and_keeps_permission_failures_local() {
    let server = MockServer::start();
    let private = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/acme/private/pulls")
            .header("authorization", "Bearer private-token");
        then.status(404);
    });
    let public = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/acme/public/pulls")
            .header("authorization", "Bearer private-token");
        then.status(200).json_body(serde_json::json!([]));
    });
    let repository = repository_with_remotes(&[
        ("private", "git@github.com:acme/private.git"),
        ("public", "https://github.com/acme/public.git"),
    ]);

    let groups = discover_pull_requests(
        repository.path(),
        &server.base_url(),
        Some(SecretString::from("private-token")),
    )
    .unwrap();

    assert_eq!(groups.len(), 2);
    assert!(groups[0].error.as_deref().unwrap().contains("无权访问"));
    assert!(groups[1].error.is_none());
    private.assert();
    public.assert();
}

#[test]
fn reports_offline_discovery_per_repository_without_losing_groups() {
    let repository = repository_with_remotes(&[
        ("origin", "https://github.com/acme/one.git"),
        ("upstream", "https://github.com/acme/two.git"),
    ]);

    let groups = discover_pull_requests(repository.path(), "http://127.0.0.1:9", None).unwrap();

    assert_eq!(groups.len(), 2);
    assert!(groups
        .iter()
        .all(|group| group.error.as_deref().unwrap().contains("无法连接 GitHub")));
}
