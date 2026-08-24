//! Claude Code JSON result 协议的解析与稳定错误投影。

use super::super::{BabysitError, BabysitProviderSetupFailure};
use serde::Deserialize;
use serde_json::Value;

/// Claude JSON result envelope；未知字段由 serde 默认忽略。
#[derive(Deserialize)]
pub(super) struct ClaudeResultEnvelope {
    #[serde(default)]
    pub structured_output: Option<Value>,
    #[serde(default)]
    pub is_error: Option<bool>,
    #[serde(default)]
    pub subtype: Option<String>,
}

/// 构造不含敏感进程内容的稳定错误。
pub(super) fn redacted_provider_error(category: &str) -> BabysitError {
    BabysitError::Provider {
        detail: format!("Claude Code 进程失败（{category}）"),
    }
}

/// 将非零退出投影为稳定类别。
pub(super) fn classify_failure(stdout: &[u8]) -> BabysitError {
    serde_json::from_slice::<ClaudeResultEnvelope>(stdout)
        .ok()
        .filter(|value| value.is_error.unwrap_or(false))
        .map(|value| classify_result_error(value.subtype.as_deref()))
        .unwrap_or_else(|| redacted_provider_error("process_exit"))
}

/// 将 Claude result subtype 投影为现有 setup/provider 类别。
pub(super) fn classify_result_error(subtype: Option<&str>) -> BabysitError {
    match subtype {
        Some("authentication_failed" | "not_authenticated" | "signed_out") => {
            BabysitError::provider_setup("claude-code", BabysitProviderSetupFailure::SignedOut)
        }
        Some("incompatible" | "unsupported") => {
            BabysitError::provider_setup("claude-code", BabysitProviderSetupFailure::Incompatible)
        }
        _ => redacted_provider_error("result_error"),
    }
}
