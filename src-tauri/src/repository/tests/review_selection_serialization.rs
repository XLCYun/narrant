use super::*;
use crate::review::PullRequestState;
use serde_json::json;

/// 验证 Branch selection 的 IPC 字段名与前端 camelCase 契约完全一致。
#[test]
fn branch_selection_serializes_with_camel_case_fields() {
    let selection = ReviewSelectionSummary::Branch {
        base_ref: "refs/heads/main".to_owned(),
        head_ref: "refs/heads/topic".to_owned(),
    };

    assert_eq!(
        serde_json::to_value(selection).unwrap(),
        json!({
            "kind": "branch",
            "baseRef": "refs/heads/main",
            "headRef": "refs/heads/topic",
        })
    );
}

/// 验证 Pull Request selection 及其嵌套元数据保持既有 camelCase IPC 契约。
#[test]
fn pull_request_selection_serializes_with_camel_case_fields() {
    let selection = ReviewSelectionSummary::PullRequest {
        base_ref: "refs/heads/main".to_owned(),
        head_ref: "refs/pull/42/head".to_owned(),
        pull_request: Box::new(PullRequestMetadata {
            owner: "octo-org".to_owned(),
            repository: "octo-repo".to_owned(),
            number: 42,
            title: "Fix selection serialization".to_owned(),
            description: "Keep the IPC contract stable.".to_owned(),
            author_username: "octocat".to_owned(),
            state: PullRequestState::Open,
            is_draft: false,
            web_url: "https://github.com/octo-org/octo-repo/pull/42".to_owned(),
            base_ref: "main".to_owned(),
            head_ref: "topic".to_owned(),
            github_updated_at: Some("2026-08-19T08:00:00Z".to_owned()),
            synchronized_at_millis: 1_755_590_400_000,
        }),
    };

    assert_eq!(
        serde_json::to_value(selection).unwrap(),
        json!({
            "kind": "pull-request",
            "baseRef": "refs/heads/main",
            "headRef": "refs/pull/42/head",
            "pullRequest": {
                "owner": "octo-org",
                "repository": "octo-repo",
                "number": 42,
                "title": "Fix selection serialization",
                "description": "Keep the IPC contract stable.",
                "authorUsername": "octocat",
                "state": "open",
                "isDraft": false,
                "webUrl": "https://github.com/octo-org/octo-repo/pull/42",
                "baseRef": "main",
                "headRef": "topic",
                "githubUpdatedAt": "2026-08-19T08:00:00Z",
                "synchronizedAtMillis": 1_755_590_400_000_i64,
            },
        })
    );
}
