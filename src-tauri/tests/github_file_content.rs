use httpmock::prelude::*;
use narrant_lib::change_content::github::GitHubOpenedReview;
use narrant_lib::change_content::{ChangeStatus, ContentKind};

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
            "content": "b2xkIGdyZWV0aW5nCg==", "encoding": "base64", "size": 13
        }));
    });
    let after = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/blobs/new-blob");
        then.status(200).json_body(serde_json::json!({
            "content": "bmV3IGdyZWV0aW5nCg==", "encoding": "base64", "size": 13
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/commits/immutable-comparison-base");
        then.status(200)
            .json_body(serde_json::json!({ "tree": { "sha": "comparison-tree" } }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/trees/comparison-tree");
        then.status(200).json_body(serde_json::json!({ "tree": [
            { "path": "hello world.txt", "mode": "100644", "type": "blob", "sha": "old-blob", "size": 13 }
        ]}));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/commits/immutable-head");
        then.status(200)
            .json_body(serde_json::json!({ "tree": { "sha": "head-tree" } }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/trees/head-tree");
        then.status(200).json_body(serde_json::json!({ "tree": [
            { "path": "hello world.txt", "mode": "100644", "type": "blob", "sha": "new-blob", "size": 13 }
        ]}));
    });
    let base_revision = server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/commits/immutable-base");
        then.status(500);
    });
    let review = GitHubOpenedReview::open(
        server.base_url(),
        "https://github.com/octo-org/octo-repo/pull/42",
    )
    .unwrap();

    assert_eq!(before.hits(), 0);
    assert_eq!(after.hits(), 0);
    let change = review.read_file_change("hello world.txt").unwrap();

    before.assert();
    after.assert();
    assert_eq!(base_revision.hits(), 0);
    assert_eq!(change.before.as_deref(), Some("old greeting\n"));
    assert_eq!(change.after.as_deref(), Some("new greeting\n"));
}

#[test]
fn reads_added_deleted_and_renamed_changes_by_their_snapshot_paths() {
    let server = MockServer::start();
    server.mock(|when, then| {
        when.method(GET).path("/repos/octo-org/octo-repo/pulls/42");
        then.status(200).json_body(serde_json::json!({
            "title": "All statuses", "number": 42, "user": { "login": "octocat" },
            "state": "open", "draft": false, "merged_at": null,
            "base": { "ref": "main", "sha": "base-tip" },
            "head": { "ref": "feature", "sha": "head-tip" },
            "body": "", "changed_files": 3, "additions": 2, "deletions": 1
        }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/compare/base-tip...head-tip");
        then.status(200)
            .json_body(serde_json::json!({ "merge_base_commit": { "sha": "merge-base" } }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/pulls/42/files");
        then.status(200).json_body(serde_json::json!([
            { "filename": "added.txt", "status": "added" },
            { "filename": "deleted.txt", "status": "removed" },
            { "filename": "new-name.txt", "previous_filename": "old-name.txt", "status": "renamed" }
        ]));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/commits/merge-base");
        then.status(200)
            .json_body(serde_json::json!({ "tree": { "sha": "before-tree" } }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/trees/before-tree");
        then.status(200).json_body(serde_json::json!({ "tree": [
            { "path": "deleted.txt", "mode": "100644", "type": "blob", "sha": "deleted-blob", "size": 8 },
            { "path": "old-name.txt", "mode": "100644", "type": "blob", "sha": "old-name-blob", "size": 11 }
        ]}));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/commits/head-tip");
        then.status(200)
            .json_body(serde_json::json!({ "tree": { "sha": "after-tree" } }));
    });
    server.mock(|when, then| {
        when.method(GET)
            .path("/repos/octo-org/octo-repo/git/trees/after-tree");
        then.status(200).json_body(serde_json::json!({ "tree": [
            { "path": "added.txt", "mode": "100644", "type": "blob", "sha": "added-blob", "size": 6 },
            { "path": "new-name.txt", "mode": "100644", "type": "blob", "sha": "new-name-blob", "size": 11 }
        ]}));
    });
    for (sha, content, size) in [
        ("deleted-blob", "ZGVsZXRlZAo=", 8),
        ("old-name-blob", "b2xkIGNvbnRlbnQ=", 11),
        ("added-blob", "YWRkZWQK", 6),
        ("new-name-blob", "bmV3IGNvbnRlbnQ=", 11),
    ] {
        server.mock(move |when, then| {
            when.method(GET)
                .path(format!("/repos/octo-org/octo-repo/git/blobs/{sha}"));
            then.status(200).json_body(serde_json::json!({
                "content": content, "encoding": "base64", "size": size
            }));
        });
    }
    let review = GitHubOpenedReview::open(
        server.base_url(),
        "https://github.com/octo-org/octo-repo/pull/42",
    )
    .unwrap();

    let added = review.read_file_change("added.txt").unwrap();
    assert_eq!(added.status, ChangeStatus::Added);
    assert_eq!(added.before, None);
    assert_eq!(added.after.as_deref(), Some("added\n"));

    let deleted = review.read_file_change("deleted.txt").unwrap();
    assert_eq!(deleted.status, ChangeStatus::Deleted);
    assert_eq!(deleted.before.as_deref(), Some("deleted\n"));
    assert_eq!(deleted.after, None);

    let renamed = review.read_file_change("new-name.txt").unwrap();
    assert_eq!(renamed.status, ChangeStatus::Renamed);
    assert_eq!(renamed.old_path.as_deref(), Some("old-name.txt"));
    assert_eq!(renamed.before.as_deref(), Some("old content"));
    assert_eq!(renamed.after.as_deref(), Some("new content"));

    for invalid_path in ["old-name.txt", "missing.txt"] {
        assert_eq!(
            review
                .read_file_change(invalid_path)
                .unwrap_err()
                .to_string(),
            "这个 File Change 不属于当前 Review Snapshot。"
        );
    }
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
    for revision in ["merge-base", "head"] {
        server.mock(move |when, then| {
            when.method(GET)
                .path(format!("/repos/octo-org/octo-repo/git/commits/{revision}"));
            then.status(200)
                .json_body(serde_json::json!({ "tree": { "sha": revision } }));
        });
        server.mock(move |when, then| {
            when.method(GET)
                .path(format!("/repos/octo-org/octo-repo/git/trees/{revision}"));
            then.status(200).json_body(serde_json::json!({ "tree": [
                { "path": "image.bin", "mode": "100644", "type": "blob", "sha": "binary", "size": 2 },
                { "path": "link", "mode": "120000", "type": "blob", "sha": "link-blob", "size": 9 },
                { "path": "vendor", "mode": "160000", "type": "commit", "sha": "submodule-commit", "size": null }
            ]}));
        });
    }
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
    let review = GitHubOpenedReview::open(
        server.base_url(),
        "https://github.com/octo-org/octo-repo/pull/42",
    )
    .unwrap();

    assert_eq!(
        review.read_file_change("image.bin").unwrap().content_kind,
        ContentKind::Binary
    );
    assert_eq!(
        review.read_file_change("link").unwrap().content_kind,
        ContentKind::Symlink
    );
    let submodule = review.read_file_change("vendor").unwrap();
    assert_eq!(submodule.content_kind, ContentKind::Submodule);
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
    let review = GitHubOpenedReview::open(
        server.base_url(),
        "https://github.com/octo-org/octo-repo/pull/42",
    )
    .unwrap();

    assert_eq!(
        review
            .read_file_change("missing.txt")
            .unwrap_err()
            .to_string(),
        "无法读取固定版本中的 File Change（404），请重新打开 Pull Request 后重试。"
    );
}
