use super::*;

/// 严格 schema 在验证前拒绝重复、未知字段和解释器路径注入。
#[test]
fn typed_request_rejects_unapproved_data() {
    let request = serde_json::json!({"protocol":PROTOCOL,"type":"analyze","id":"analysis-1","input":{"repository_root":"/repository","files":[],"include_tests":false,"profile":"python-static-v1"},"environment":{"scratch_root":"/scratch","platforms":[]},"limits":{"analysis_timeout_ms":1,"max_result_bytes":1,"max_diagnostic_bytes":1}});
    let _: Request = serde_json::from_value(request.clone()).unwrap();
    for pointer in ["/input/command", "/environment/python", "/limits/unknown"] {
        let mut invalid = request.clone();
        let (parent, name) = pointer.rsplit_once('/').unwrap();
        invalid
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(name.into(), serde_json::json!("injection"));
        assert!(serde_json::from_value::<Request>(invalid).is_err());
    }
    let duplicate = serde_json::to_string(&request)
        .unwrap()
        .replace("\"id\":", "\"id\":\"duplicate\",\"id\":");
    assert!(serde_json::from_str::<Request>(&duplicate).is_err());
    let mut platform_request = request;
    platform_request["environment"]["platforms"] = serde_json::json!([{"id":"stdlib","kind":"python-embedded-stdlib","version":"0.0.84","root":"/helper","metadata":{"source_revision":SOURCE_REVISION,"python":"/venv"}}]);
    assert!(serde_json::from_value::<Request>(platform_request).is_err());
    let _: fn(&str, &str) -> Result<()> = narrant_native_common::one_shot::validate_cancel;
}
