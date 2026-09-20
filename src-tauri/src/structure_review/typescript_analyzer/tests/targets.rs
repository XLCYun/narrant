use super::*;
use crate::structure_review::{
    source_text::SourceText,
    typescript_analyzer::{lsp::CheckedRange, LINE_BREAKS},
};

/// 构造真实原文坐标的证据，验证语义匹配不会按邻近所有者猜测。
fn location(path: &Path, start: u32, end: u32) -> CheckedLocation {
    CheckedLocation {
        path: path.to_path_buf(),
        span: CheckedRange {
            start_byte: start,
            end_byte: end,
        },
    }
}

/// 空、越过名字的范围和匿名回调体都不能变成确定的源声明身份。
#[test]
fn declaration_evidence_rejects_non_declaration_ranges() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("main.ts");
    let source = "function target() {} function owner() { [1].map(() => target()); } class Holder { handler = () => 1; }";
    let document = Document::parse(
        root.path(),
        path.clone(),
        SourceText::new(source, LINE_BREAKS),
        AnalysisOptions::default(),
    )
    .unwrap();
    let start = source.find("target").unwrap() as u32;
    let evidence = location(&path, start, start + 6);
    assert_eq!(
        declaration_at(&document, &evidence)
            .unwrap()
            .node
            .data()
            .name
            .as_deref(),
        Some("target")
    );
    assert!(declaration_at(&document, &location(&path, start, start)).is_none());
    assert!(declaration_at(&document, &location(&path, start, start + 10)).is_none());
    let callback = source.find("() => target()").unwrap() as u32;
    assert!(declaration_at(&document, &location(&path, callback, callback + 14)).is_none());
    let handler = source.find("() => 1").unwrap() as u32;
    assert_eq!(
        declaration_at(&document, &location(&path, handler, handler + 7))
            .unwrap()
            .node
            .data()
            .name
            .as_deref(),
        Some("handler")
    );
}
