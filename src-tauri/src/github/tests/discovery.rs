use super::discovery::{PullRequestDiscoveryQuery, PullRequestPageRequest, PullRequestStateFilter};
use super::{discover_pull_requests, GitHubRepository};
use httpmock::prelude::*;
use secrecy::SecretString;
use serde_json::{json, Value};
use std::process::Command;
use tempfile::TempDir;

/// 创建带指定 remote 的最小 Git 仓库，作为公开 discovery seam 的输入。
fn repository_with_remotes(remotes: &[(&str, &str)]) -> TempDir {
    let directory = TempDir::new().unwrap();
    assert!(Command::new("git")
        .args(["init", directory.path().to_str().unwrap()])
        .output()
        .unwrap()
        .status
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
            .output()
            .unwrap()
            .status
            .success());
    }
    directory
}

/// 生成完整的协作元数据，允许同一页包含合并与未合并的关闭条目。
fn item(number: u64, state: &str, merged: bool) -> Value {
    json!({
        "number": number, "title": format!("Pull Request {number}"), "body": "Description",
        "user": { "login": "octocat" }, "state": state,
        "merged_at": if merged { Some("2026-07-26T00:00:00Z") } else { None },
        "updated_at": "2026-07-31T08:30:00Z", "draft": true,
        "html_url": format!("https://github.com/acme/public/pull/{number}"),
        "base": { "ref": "main", "sha": "base-sha" }, "head": { "ref": "feature", "sha": "head-sha" }
    })
}

/// 首页不带任何逐来源页码，因此始终按最新 remotes 去重。
fn first(state: PullRequestStateFilter) -> PullRequestDiscoveryQuery {
    PullRequestDiscoveryQuery { state, pages: None }
}

/// 构造指定来源的后续页请求，不能携带外部 URL。
fn page(
    state: PullRequestStateFilter,
    owner: &str,
    name: &str,
    page: u32,
) -> PullRequestDiscoveryQuery {
    PullRequestDiscoveryQuery {
        state,
        pages: Some(vec![PullRequestPageRequest {
            repository: GitHubRepository {
                owner: owner.into(),
                name: name.into(),
            },
            page,
        }]),
    }
}

/// 大量历史也只请求有限首页；满页的末页不需要再探测一个空页。
#[tokio::test]
async fn large_history_has_one_request_per_action_and_link_controls_end() {
    let server = MockServer::start();
    let first_page = server.mock(|when, then| {
        when.method(GET).path("/repos/acme/public/pulls")
            .query_param("state", "open").query_param("per_page", "30")
            .query_param("sort", "updated").query_param("direction", "desc").query_param("page", "1")
            .matches(|request| request.headers.as_ref().is_none_or(|headers| headers.iter().all(|(name, _)| !name.eq_ignore_ascii_case("authorization"))));
        then.status(200)
            .header("Link", "<https://api.github.com/repos/acme/public/pulls?page=2>; rel=\"next\", <https://api.github.com/repos/acme/public/pulls?page=5000>; rel=\"last\"")
            .json_body(json!((1..=30).map(|n| item(n, "open", false)).collect::<Vec<_>>()));
    });
    let second_page = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/acme/public/pulls")
            .query_param("page", "2")
            .query_param("state", "open")
            .query_param("per_page", "30")
            .query_param("sort", "updated")
            .query_param("direction", "desc");
        then.status(200)
            .header(
                "Link",
                "<https://api.github.com/repos/acme/public/pulls?page=1>; rel=\"prev\"",
            )
            .json_body(json!((31..=60)
                .map(|n| item(n, "open", false))
                .collect::<Vec<_>>()));
    });
    let third_page = server.mock(|when, then| {
        when.method(GET).query_param("page", "3");
        then.status(500);
    });
    let repository = repository_with_remotes(&[
        ("origin", "https://github.com/ACME/public.git"),
        ("upstream", "git@github.com:acme/public.git"),
        ("ssh-host", "ssh://github.com/acme/public.git"),
        ("ssh-port", "ssh://git@github.com:22/acme/public.git"),
        ("elsewhere", "https://example.com/acme/public.git"),
    ]);
    let groups = discover_pull_requests(
        repository.path(),
        &server.base_url(),
        None,
        first(PullRequestStateFilter::Open),
    )
    .await
    .unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0].remote_names,
        vec!["origin", "ssh-host", "ssh-port", "upstream"]
    );
    assert_eq!(groups[0].pull_requests.len(), 30);
    assert!(groups[0].pull_requests[0].is_draft);
    assert_eq!(groups[0].next_page, Some(2));
    assert_eq!(first_page.hits(), 1);
    assert_eq!(second_page.hits(), 0);
    let groups = discover_pull_requests(
        repository.path(),
        &server.base_url(),
        None,
        page(PullRequestStateFilter::Open, "ACME", "PUBLIC", 2),
    )
    .await
    .unwrap();
    assert_eq!(groups[0].pull_requests.len(), 30);
    assert_eq!(groups[0].page, 2);
    assert_eq!(groups[0].next_page, None);
    assert_eq!(first_page.hits(), 1);
    assert_eq!(second_page.hits(), 1);
    assert_eq!(third_page.hits(), 0);
}

/// Closed 与 Merged 不为补满结果扫描历史，保留相反状态以校正已保存 Review 的元数据。
#[tokio::test]
async fn closed_and_merged_scan_only_one_closed_page_even_without_matching_rows() {
    let server = MockServer::start();
    let list = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/acme/public/pulls")
            .query_param("state", "closed")
            .query_param("page", "1")
            .query_param("per_page", "30")
            .query_param("sort", "updated")
            .query_param("direction", "desc");
        then.status(200)
            .header(
                "Link",
                "<https://elsewhere.invalid/pulls?page=2>; rel=\"next\"",
            )
            .json_body(json!([item(42, "closed", true)]));
    });
    let later = server.mock(|when, then| {
        when.query_param("page", "2");
        then.status(500);
    });
    let repository = repository_with_remotes(&[("origin", "https://github.com/acme/public")]);
    for state in [
        PullRequestStateFilter::Closed,
        PullRequestStateFilter::Merged,
    ] {
        let groups =
            discover_pull_requests(repository.path(), &server.base_url(), None, first(state))
                .await
                .unwrap();
        assert_eq!(groups[0].pull_requests[0].state, "merged");
        assert_eq!(groups[0].next_page, Some(2));
        assert!(groups[0].error.is_none());
    }
    assert_eq!(list.hits(), 2);
    assert_eq!(later.hits(), 0);
}

/// All 查询保留三种状态，同页重复 identity 只留下较新的元数据。
#[tokio::test]
async fn all_preserves_states_and_deduplicates_page_identities() {
    let server = MockServer::start();
    let mut newer = item(1, "closed", false);
    newer["updated_at"] = json!("2026-08-01T00:00:00Z");
    let list = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/acme/public/pulls")
            .query_param("state", "all")
            .query_param("page", "1")
            .query_param("sort", "updated")
            .query_param("direction", "desc")
            .query_param("per_page", "30");
        then.status(200).json_body(json!([
            newer,
            item(1, "open", false),
            item(2, "closed", true),
            item(3, "open", false)
        ]));
    });
    let repository = repository_with_remotes(&[("origin", "https://github.com/acme/public")]);
    let groups = discover_pull_requests(
        repository.path(),
        &server.base_url(),
        None,
        first(PullRequestStateFilter::All),
    )
    .await
    .unwrap();
    assert_eq!(
        groups[0]
            .pull_requests
            .iter()
            .map(|pr| pr.state.as_str())
            .collect::<Vec<_>>(),
        vec!["closed", "merged", "open"]
    );
    assert_eq!(groups[0].next_page, None);
    assert_eq!(list.hits(), 1);
}

/// 失败来源返回原重试页，私有鉴权与其他来源的成功结果互不影响。
#[tokio::test]
async fn authenticated_partial_failure_retains_retry_page_and_only_requests_selected_sources() {
    let server = MockServer::start();
    let private = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/acme/private/pulls")
            .header("authorization", "Bearer private-token")
            .query_param("page", "2");
        then.status(404);
    });
    let public = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/acme/public/pulls")
            .header("authorization", "Bearer private-token");
        then.status(200).json_body(json!([]));
    });
    let repository = repository_with_remotes(&[
        ("private", "git@github.com:acme/private.git"),
        ("public", "https://github.com/acme/public.git"),
    ]);
    let query = PullRequestDiscoveryQuery {
        state: PullRequestStateFilter::Open,
        pages: Some(vec![
            PullRequestPageRequest {
                repository: GitHubRepository {
                    owner: "acme".into(),
                    name: "private".into(),
                },
                page: 2,
            },
            PullRequestPageRequest {
                repository: GitHubRepository {
                    owner: "acme".into(),
                    name: "public".into(),
                },
                page: 1,
            },
        ]),
    };
    let groups = discover_pull_requests(
        repository.path(),
        &server.base_url(),
        Some(SecretString::from("private-token")),
        query,
    )
    .await
    .unwrap();
    assert!(groups[0].error.as_deref().unwrap().contains("无权访问"));
    assert_eq!(groups[0].next_page, Some(2));
    assert!(groups[1].error.is_none());
    assert_eq!(groups[1].next_page, None);
    discover_pull_requests(
        repository.path(),
        &server.base_url(),
        Some(SecretString::from("private-token")),
        page(PullRequestStateFilter::Open, "acme", "private", 2),
    )
    .await
    .unwrap();
    assert_eq!(private.hits(), 2);
    assert_eq!(public.hits(), 1);
}

/// 只访问 remotes 内的协作元数据，不能用页码列表扩展到陌生来源或代码 endpoint。
#[tokio::test]
async fn only_lists_configured_metadata_sources_and_empty_selection_makes_no_request() {
    let server = MockServer::start();
    let list = server.mock(|when, then| {
        when.method(GET).path("/repos/acme/public/pulls");
        then.status(200).json_body(json!([]));
    });
    let forbidden = server.mock(|when, then| {
        when.path_matches(
            Regex::new(r"^/repos/.+/(contents|trees|git/blobs|compare|pulls/\d+/files)").unwrap(),
        );
        then.status(500);
    });
    let stranger = server.mock(|when, then| {
        when.path("/repos/stranger/secret/pulls");
        then.status(500);
    });
    let repository = repository_with_remotes(&[("origin", "https://github.com/acme/public")]);
    discover_pull_requests(
        repository.path(),
        &server.base_url(),
        None,
        first(PullRequestStateFilter::Open),
    )
    .await
    .unwrap();
    for query in [
        PullRequestDiscoveryQuery {
            state: PullRequestStateFilter::Open,
            pages: Some(vec![]),
        },
        page(PullRequestStateFilter::All, "stranger", "secret", 1),
    ] {
        assert!(
            discover_pull_requests(repository.path(), &server.base_url(), None, query)
                .await
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(list.hits(), 1);
    assert_eq!(forbidden.hits(), 0);
    assert_eq!(stranger.hits(), 0);
}

/// 无效或重复来源请求在网络前拒绝，保证每来源最多一次请求。
#[tokio::test]
async fn rejects_invalid_or_duplicate_pages_before_network() {
    let server = MockServer::start();
    let unexpected = server.mock(|when, then| {
        when.method(GET);
        then.status(500);
    });
    let repository = repository_with_remotes(&[("origin", "https://github.com/acme/public")]);
    let mut duplicate = page(PullRequestStateFilter::Open, "acme", "public", 1);
    duplicate
        .pages
        .as_mut()
        .unwrap()
        .push(PullRequestPageRequest {
            repository: GitHubRepository {
                owner: "ACME".into(),
                name: "PUBLIC".into(),
            },
            page: 2,
        });
    for query in [
        duplicate,
        page(PullRequestStateFilter::Open, "acme", "public", 0),
    ] {
        assert!(
            discover_pull_requests(repository.path(), &server.base_url(), None, query)
                .await
                .is_err()
        );
    }
    assert_eq!(unexpected.hits(), 0);
}

/// 网络故障按来源返回，未知结果不能被当作完整末页。
#[tokio::test]
async fn transport_failure_is_local_and_retryable() {
    let repository = repository_with_remotes(&[
        ("origin", "https://github.com/acme/one.git"),
        ("upstream", "https://github.com/acme/two.git"),
    ]);
    let groups = discover_pull_requests(
        repository.path(),
        "://invalid-github-api-base",
        None,
        first(PullRequestStateFilter::Open),
    )
    .await
    .unwrap();
    assert_eq!(groups.len(), 2);
    assert!(groups.iter().all(
        |group| group.error.as_deref().unwrap().contains("无法连接 GitHub")
            && group.next_page == Some(1)
    ));
}
