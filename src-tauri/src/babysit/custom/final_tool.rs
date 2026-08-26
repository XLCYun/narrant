use super::RepositoryTools;
use rig::completion::ToolDefinition;
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

/// 返回 Rig OutputMode::Tool 需要的 schema-bearing final tool 元数据。
pub(crate) fn line_explanation_tool_definition() -> ToolDefinition {
    ToolDefinition {
        name: FINAL_OUTPUT_TOOL_NAME.to_owned(),
        description: "提交完整的 before/after Line Explanation 结果。".to_owned(),
        parameters: line_explanation_schema().to_value(),
    }
}

/// 将 Narrant 拥有的三个 repository tool 注册与 final tool 拼成固定顺序。
pub(crate) fn custom_tool_definitions(tools: &RepositoryTools) -> Vec<ToolDefinition> {
    let mut definitions = tools
        .registrations()
        .into_iter()
        .map(|registration| ToolDefinition {
            name: registration.name,
            description: registration.description,
            parameters: registration.input_schema,
        })
        .collect::<Vec<_>>();
    definitions.push(line_explanation_tool_definition());
    definitions
}
