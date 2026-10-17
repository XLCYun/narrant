use super::*;

/// cancel 只接受同一请求及精确 schema，重复/未知字段不能被宽松 Value 解码吞掉。
#[test]
fn cancellation_control_is_strict() {
    let valid = r#"{"protocol":"narrant.native-analyzer/1","type":"cancel","id":"analysis-1"}"#;
    validate_cancel(valid, "analysis-1").unwrap();
    assert!(validate_cancel(valid, "other").is_err());
    for invalid in [
        valid.replace("cancel", "analyze"),
        valid.replace("/1", "/2"),
        valid.replace("\"id\":", "\"unknown\":true,\"id\":"),
        valid.replace("\"id\":", "\"id\":\"first\",\"id\":"),
    ] {
        assert!(validate_cancel(&invalid, "analysis-1").is_err());
    }
}
