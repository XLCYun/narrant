use super::*;
use crate::review::{FileChangeContent, FileChangeStatus, FileContent};

/// 构造一个不应触发 Codex runtime 的文档 File Change。
fn markdown_change() -> FileChangeContent {
    FileChangeContent {
        path: "README.md".to_owned(),
        old_path: None,
        status: FileChangeStatus::Modified,
        before: Some(FileContent::Text {
            size: 6,
            text: "# old\n".to_owned(),
        }),
        after: Some(FileContent::Text {
            size: 6,
            text: "# new\n".to_owned(),
        }),
    }
}

#[test]
fn explain_gate_rejects_ineligible_file_before_runtime_io() {
    let directory = tempfile::tempdir().unwrap();
    let settings = directory.path().join("babysit-settings.json");
    let babysit = BabysitApplication::with_codex_provider_at(&settings);

    let error =
        validate_explain_input(&babysit, "README.md", &markdown_change(), None).unwrap_err();

    assert_eq!(
        error.code,
        crate::babysit::BabysitErrorCode::UnsupportedFile
    );
    assert!(!settings.exists());
}
