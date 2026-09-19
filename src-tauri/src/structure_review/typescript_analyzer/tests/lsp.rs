use super::*;
use crate::structure_review::source_text::SourceText;
use crate::structure_review::typescript_analyzer::LINE_BREAKS;

/// 原始 span 对齐 TS 行列语义，且拒绝代理对中点、零坐标与反向范围。
#[test]
fn raw_evidence_is_checked() {
    let source = SourceText::new("a😀b\r\nnext", LINE_BREAKS);
    let range = checked_range(
        &source,
        &json!({"start":{"line":1,"offset":2},"end":{"line":1,"offset":4}}),
    )
    .unwrap();
    assert_eq!((range.start_byte, range.end_byte), (1, 5));
    for value in [
        json!({"start":{"line":1,"offset":3},"end":{"line":1,"offset":4}}),
        json!({"start":{"line":0,"offset":1},"end":{"line":1,"offset":1}}),
        json!({"start":{"line":2,"offset":1},"end":{"line":1,"offset":1}}),
    ] {
        assert!(checked_range(&source, &value).is_err());
    }
}

/// 错误、null、缺 body 和错误命令不能作为成功的空目标。
#[test]
fn raw_errors_do_not_become_empty() {
    for response in [
        Value::Null,
        json!({"type":"response","command":"definition","success":false,"message":"failed"}),
        json!({"type":"response","command":"definition","success":true}),
        json!({"type":"response","command":"other","success":true,"body":[]}),
    ] {
        assert!(response_body("definition", response, true).is_err());
    }
    assert!(array(&Value::Null, "definition").is_err());
}

#[path = "real_service.rs"]
mod real_service;
