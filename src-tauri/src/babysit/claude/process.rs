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
    repository_exploration: bool,
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
        .arg(if repository_exploration {
            "Read,Glob,Grep"
        } else {
            ""
        });
    if repository_exploration {
        command
            .arg("--disallowedTools")
            .arg("Bash,Write,Edit,Web,mcp__*");
    }
    if let Some(model) = &resolved_model.model {
        command.arg("--model").arg(model);
    }
    if let Some(effort) = &resolved_model.reasoning_effort {
        command.arg("--effort").arg(effort);
    }
    command.arg("--json-schema");
    let schema = schema.to_string();
    #[cfg(all(test, windows))]
    if !prefix_args.is_empty() {
        // Git Bash 夹具采用 MSYS 转义：整体加引号，并转义每个反斜杠和双引号。
        // 生产 Claude 使用原生 Windows argv，仍由 Command::arg 负责转义。
        command.raw_arg(format!(
            "\"{}\"",
            schema.replace('\\', "\\\\").replace('"', "\\\"")
        ));
        return;
    }
    command.arg(schema);
}
