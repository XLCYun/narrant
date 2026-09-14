use super::*;
use crate::{
    lsp::Range,
    structure_review::typescript_analyzer::{lsp::CheckedRange, text},
};

/// 构造真实原文坐标的证据，验证语义匹配不会按邻近所有者猜测。
fn location(document: &Document, start: u32, end: u32) -> CheckedLocation {
    CheckedLocation {
        view_path: document.path.clone(),
        uri: document.uri.clone(),
        unverified: false,
        span: CheckedRange {
            start_byte: start,
            end_byte: end,
            range: Range {
                start: text::position(&document.source, start).unwrap(),
                end: text::position(&document.source, end).unwrap(),
            },
        },
    }
}

/// 不可信、空、越过名字的范围和匿名回调体都不能变成确定的源声明身份。
#[test]
fn declaration_evidence_rejects_unverified_and_non_declaration_ranges() {
    let root = tempfile::tempdir().unwrap();
    let source = "function target() {} function owner() { [1].map(() => target()); } class Holder { handler = () => 1; }";
    let document = Document::parse(
        root.path(),
        root.path().join("main.ts"),
        source.into(),
        AnalysisOptions::default(),
    )
    .unwrap();
    let start = source.find("target").unwrap() as u32;
    let mut evidence = location(&document, start, start + 6);
    assert_eq!(
        declaration_at(&document, &evidence)
            .unwrap()
            .node
            .data()
            .name
            .as_deref(),
        Some("target")
    );
    evidence.unverified = true;
    assert!(declaration_at(&document, &evidence).is_none());
    assert!(declaration_at(&document, &location(&document, start, start)).is_none());
    assert!(declaration_at(&document, &location(&document, start, start + 10)).is_none());
    let callback = source.find("() => target()").unwrap() as u32;
    assert!(declaration_at(&document, &location(&document, callback, callback + 14)).is_none());
    let handler = source.find("() => 1").unwrap() as u32;
    assert_eq!(
        declaration_at(&document, &location(&document, handler, handler + 7))
            .unwrap()
            .node
            .data()
            .name
            .as_deref(),
        Some("handler")
    );
}
