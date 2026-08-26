use super::*;
use tempfile::tempdir;

#[test]
fn registration_exposes_only_the_bounded_repository_tool_set() {
    let workspace = tempdir().unwrap();
    let tools = RepositoryTools::new(workspace.path()).unwrap();

    let registration = tools.registration();

    assert_eq!(
        registration.names(),
        vec!["read".to_owned(), "glob".to_owned(), "grep".to_owned()]
    );
    assert_eq!(registration.tools().len(), 3);
    for definition in registration.tools() {
        assert_eq!(definition.input_schema["type"], "object");
    }
    assert!(registration.tool("read").unwrap().input_schema["properties"]["path"].is_object());
}

#[test]
fn read_returns_text_file_with_workspace_relative_identity() {
    let workspace = tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("src")).unwrap();
    std::fs::write(workspace.path().join("src/lib.rs"), "fn main() {}\n").unwrap();
    let tools = RepositoryTools::new(workspace.path()).unwrap();

    let response = tools
        .read(
            ReadInput::new("src/lib.rs"),
            &crate::operation::OperationContext::background().cancellation(),
        )
        .unwrap();

    assert_eq!(response.path, "src/lib.rs");
    assert_eq!(response.content, "fn main() {}\n");
    assert_eq!(response.continuation, None);
    assert!(!response.truncated);
}

#[test]
fn read_accepts_absolute_and_parent_paths_only_when_the_canonical_target_is_inside() {
    let workspace = tempdir().unwrap();
    std::fs::create_dir_all(workspace.path().join("src")).unwrap();
    let file = workspace.path().join("src/lib.rs");
    std::fs::write(&file, "pub fn value() -> u8 { 7 }\n").unwrap();
    let outside = tempdir().unwrap();
    std::fs::write(outside.path().join("secret.txt"), "outside\n").unwrap();
    let tools = RepositoryTools::new(workspace.path()).unwrap();
    let cancellation = crate::operation::OperationContext::background();

    let absolute = tools
        .read(
            ReadInput::new(file.to_string_lossy()),
            cancellation.cancellation(),
        )
        .unwrap();
    let parent = tools
        .read(
            ReadInput::new("src/../src/lib.rs"),
            cancellation.cancellation(),
        )
        .unwrap();
    let escaped = tools.read(
        ReadInput::new(outside.path().join("secret.txt").to_string_lossy()),
        cancellation.cancellation(),
    );

    assert_eq!(absolute.path, "src/lib.rs");
    assert_eq!(absolute.content, parent.content);
    assert_eq!(escaped, Err(RepositoryToolError::OutsideWorkspace));
}

#[test]
fn read_reports_missing_targets_without_echoing_the_workspace_path() {
    let workspace = tempdir().unwrap();
    let tools = RepositoryTools::new(workspace.path()).unwrap();
    let cancellation = crate::operation::OperationContext::background();

    let error = tools
        .read(
            ReadInput::new("does-not-exist.txt"),
            cancellation.cancellation(),
        )
        .unwrap_err();
    let workspace_path = workspace.path().to_string_lossy().into_owned();

    assert_eq!(error, RepositoryToolError::TargetUnavailable);
    assert!(!error.to_string().contains(&workspace_path));
    assert!(!format!("{error:?}").contains(&workspace_path));
}

#[cfg(unix)]
#[test]
fn read_follows_inside_symlinks_but_rejects_symlinks_that_escape() {
    use std::os::unix::fs::symlink;

    let workspace = tempdir().unwrap();
    let outside = tempdir().unwrap();
    std::fs::write(workspace.path().join("inside.txt"), "inside\n").unwrap();
    std::fs::write(outside.path().join("outside.txt"), "secret\n").unwrap();
    symlink(
        workspace.path().join("inside.txt"),
        workspace.path().join("inside-link.txt"),
    )
    .unwrap();
    symlink(
        outside.path().join("outside.txt"),
        workspace.path().join("outside-link.txt"),
    )
    .unwrap();
    let tools = RepositoryTools::new(workspace.path()).unwrap();
    let cancellation = crate::operation::OperationContext::background();

    let inside = tools
        .read(
            ReadInput::new("inside-link.txt"),
            cancellation.cancellation(),
        )
        .unwrap();
    let outside_error = tools
        .read(
            ReadInput::new("outside-link.txt"),
            cancellation.cancellation(),
        )
        .unwrap_err();

    assert_eq!(inside.path, "inside.txt");
    assert_eq!(inside.content, "inside\n");
    assert_eq!(outside_error, RepositoryToolError::OutsideWorkspace);
}

#[test]
fn read_directory_returns_deterministically_sorted_entries() {
    let workspace = tempdir().unwrap();
    std::fs::create_dir(workspace.path().join("z-last")).unwrap();
    std::fs::write(workspace.path().join("m-middle.txt"), "middle\n").unwrap();
    std::fs::write(workspace.path().join("a-first.txt"), "first\n").unwrap();
    let tools = RepositoryTools::new(workspace.path()).unwrap();
    let cancellation = crate::operation::OperationContext::background();

    let response = tools
        .read(ReadInput::new("."), cancellation.cancellation())
        .unwrap();

    assert_eq!(response.path, ".");
    assert_eq!(response.content, "a-first.txt\nm-middle.txt\nz-last");
    assert!(!response.truncated);
}

#[test]
fn read_rejects_binary_and_non_utf8_files_as_non_text() {
    let workspace = tempdir().unwrap();
    std::fs::write(workspace.path().join("binary.dat"), b"text\0bytes").unwrap();
    std::fs::write(workspace.path().join("invalid.dat"), [0xff, 0xfe]).unwrap();
    let tools = RepositoryTools::new(workspace.path()).unwrap();
    let cancellation = crate::operation::OperationContext::background();

    let binary = tools
        .read(ReadInput::new("binary.dat"), cancellation.cancellation())
        .unwrap_err();
    let invalid = tools
        .read(ReadInput::new("invalid.dat"), cancellation.cancellation())
        .unwrap_err();

    assert_eq!(binary, RepositoryToolError::NonTextContent);
    assert_eq!(invalid, RepositoryToolError::NonTextContent);
}

#[test]
fn new_rejects_missing_or_non_directory_workspaces_with_stable_errors() {
    let workspace = tempdir().unwrap();
    let file = workspace.path().join("workspace-file");
    std::fs::write(&file, "not a directory").unwrap();

    assert_eq!(
        RepositoryTools::new(workspace.path().join("missing"))
            .map(|_| ())
            .unwrap_err(),
        RepositoryToolError::WorkspaceUnavailable
    );
    assert_eq!(
        RepositoryTools::new(file).map(|_| ()).unwrap_err(),
        RepositoryToolError::WorkspaceUnavailable
    );
}

#[test]
fn read_honors_the_64_kib_budget_and_returns_a_byte_continuation() {
    let workspace = tempdir().unwrap();
    let content = "x".repeat(READ_OUTPUT_LIMIT + 17);
    std::fs::write(workspace.path().join("large.txt"), &content).unwrap();
    let tools = RepositoryTools::new(workspace.path()).unwrap();
    let cancellation = crate::operation::OperationContext::background();

    let first = tools
        .read(ReadInput::new("large.txt"), cancellation.cancellation())
        .unwrap();
    let next_offset = first.continuation.as_ref().unwrap().offset;
    let second = tools
        .read(
            ReadInput::new("large.txt").with_offset(next_offset),
            cancellation.cancellation(),
        )
        .unwrap();

    assert_eq!(first.content.len(), READ_OUTPUT_LIMIT);
    assert!(first.truncated);
    assert_eq!(next_offset, READ_OUTPUT_LIMIT);
    assert_eq!(second.content, "x".repeat(17));
    assert_eq!(second.continuation, None);
    assert!(!second.truncated);
}

#[test]
fn read_limit_is_bounded_even_when_the_caller_requests_a_larger_value() {
    let workspace = tempdir().unwrap();
    std::fs::write(
        workspace.path().join("large.txt"),
        "y".repeat(READ_OUTPUT_LIMIT + 1),
    )
    .unwrap();
    let tools = RepositoryTools::new(workspace.path()).unwrap();
    let cancellation = crate::operation::OperationContext::background();

    let response = tools
        .read(
            ReadInput::new("large.txt").with_limit(READ_OUTPUT_LIMIT * 2),
            cancellation.cancellation(),
        )
        .unwrap();

    assert_eq!(response.content.len(), READ_OUTPUT_LIMIT);
    assert!(response.truncated);
}

#[test]
fn read_continuation_keeps_utf8_boundaries_when_a_byte_limit_cuts_a_character() {
    let workspace = tempdir().unwrap();
    std::fs::write(workspace.path().join("unicode.txt"), "aéz").unwrap();
    let tools = RepositoryTools::new(workspace.path()).unwrap();
    let cancellation = crate::operation::OperationContext::background();

    let first = tools
        .read(
            ReadInput::new("unicode.txt").with_limit(2),
            cancellation.cancellation(),
        )
        .unwrap();
    let second = tools
        .read(
            ReadInput::new("unicode.txt").with_offset(first.continuation.as_ref().unwrap().offset),
            cancellation.cancellation(),
        )
        .unwrap();

    assert_eq!(first.content, "a");
    assert_eq!(first.continuation, Some(ReadContinuation { offset: 1 }));
    assert_eq!(second.content, "éz");
    assert!(!second.truncated);
}

#[test]
fn read_response_serializes_identity_content_continuation_and_truncation() {
    let workspace = tempdir().unwrap();
    std::fs::write(workspace.path().join("notes.txt"), "abcdef").unwrap();
    let tools = RepositoryTools::new(workspace.path()).unwrap();
    let cancellation = crate::operation::OperationContext::background();

    let response = tools
        .read(
            ReadInput::new("notes.txt").with_limit(3),
            cancellation.cancellation(),
        )
        .unwrap();
    let value = serde_json::to_value(response).unwrap();

    assert_eq!(
        value,
        serde_json::json!({
            "path": "notes.txt",
            "content": "abc",
            "continuation": {"offset": 3},
            "truncated": true
        })
    );
}

#[test]
fn read_stops_before_filesystem_work_when_the_signal_is_already_cancelled() {
    let workspace = tempdir().unwrap();
    std::fs::write(workspace.path().join("notes.txt"), "should not be read").unwrap();
    let tools = RepositoryTools::new(workspace.path()).unwrap();
    let cancellation = crate::operation::TestCancellation::default();
    cancellation.cancel();

    let result = tools.read(ReadInput::new("notes.txt"), &cancellation);

    assert_eq!(result, Err(RepositoryToolError::Cancelled));
}

#[test]
fn execute_dispatches_typed_read_requests_through_the_shared_tool_seam() {
    let workspace = tempdir().unwrap();
    std::fs::write(workspace.path().join("notes.txt"), "typed\n").unwrap();
    let tools = RepositoryTools::new(workspace.path()).unwrap();
    let cancellation = crate::operation::OperationContext::background();

    let result = tools
        .execute(
            RepositoryToolRequest::Read(ReadInput::new("notes.txt")),
            cancellation.cancellation(),
        )
        .unwrap();

    assert_eq!(
        result,
        RepositoryToolResponse::Read(ReadResponse {
            path: "notes.txt".to_owned(),
            content: "typed\n".to_owned(),
            continuation: None,
            truncated: false,
        })
    );
}
