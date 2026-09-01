use rig::schemars::Schema;
use serde_json::json;

/// Rig Tool output mode 使用的稳定 synthetic tool 名称。
pub(crate) const FINAL_OUTPUT_TOOL_NAME: &str = "final_result";

/// 构造 provider-neutral before/after Line Explanation JSON Schema。
pub(crate) fn line_explanation_schema() -> Schema {
    serde_json::from_value(json!({
        "type": "object",
        "properties": {
            "before": {
                "type": "object",
                "additionalProperties": {"type": "string"}
            },
            "after": {
                "type": "object",
                "additionalProperties": {"type": "string"}
            }
        },
        "required": ["before", "after"],
        "additionalProperties": false
    }))
    .expect("Line Explanation schema 是固定的合法 JSON Schema")
}
