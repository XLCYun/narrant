//! Claude Code fresh process 的命令行策略。

use super::ResolvedClaudeModel;
use serde_json::Value;
use std::ffi::OsString;
use tokio::process::Command;

/// 配置一轮无会话持久化、结构化输出且仅允许只读工具的 Claude 命令。
pub(super) fn configure_round_command(
    command: &mut Command,
    prefix_args: &[OsString],
    resolved_model: &ResolvedClaudeModel,
    schema: &Value,
) {
    command
        .args(prefix_args)
        .arg("-p")
        .arg("--no-session-persistence")
        .arg("--output-format")
        .arg("json")
        .arg("--safe-mode")
        .arg("--permission-mode")
        .arg("dontAsk")
        .arg("--tools")
        .arg("Read,Glob,Grep")
        .arg("--disallowedTools")
        .arg("Bash,Write,Edit,Web,mcp__*");
    if let Some(model) = &resolved_model.model {
        command.arg("--model").arg(model);
    }
    if let Some(effort) = &resolved_model.reasoning_effort {
        command.arg("--effort").arg(effort);
    }
    command.arg("--json-schema").arg(schema.to_string());
}
