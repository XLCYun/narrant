use super::{BabysitEligibility, BabysitEligibilityReason, BabysitError};
use crate::review::{FileChangeContent, FileContentKind};

/// 检查文件是否满足宝宝模式的统一后端准入规则。
pub(super) fn ensure_eligible(
    path: &str,
    content: &FileChangeContent,
    detected_language: Option<&str>,
) -> Result<(), BabysitError> {
    let result = eligibility(path, content, detected_language);
    if result.eligible {
        return Ok(());
    }
    Err(BabysitError::UnsupportedFile {
        reason: format!(
            "{:?}",
            result
                .reason
                .unwrap_or(BabysitEligibilityReason::UnsupportedLanguage)
        ),
    })
}

/// 计算稳定 eligibility code，避免 UI 与 provider 各自维护文件白名单。
pub(super) fn eligibility(
    path: &str,
    content: &FileChangeContent,
    detected_language: Option<&str>,
) -> BabysitEligibility {
    let rejected = |reason| BabysitEligibility {
        eligible: false,
        reason: Some(reason),
    };
    if content.content_kind() != FileContentKind::Text {
        return rejected(BabysitEligibilityReason::NonText);
    }
    let normalized = path.replace('\\', "/");
    let components = normalized.split('/').collect::<Vec<_>>();
    if components.iter().any(|component| {
        matches!(
            *component,
            "node_modules" | "vendor" | "dist" | "build" | "target" | "generated" | "__snapshots__"
        )
    }) {
        return rejected(BabysitEligibilityReason::GeneratedOrVendored);
    }
    let basename = components
        .last()
        .copied()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if basename.ends_with(".lock")
        || matches!(
            basename.as_str(),
            "cargo.lock" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml"
        )
        || basename.ends_with(".min.js")
        || basename.ends_with(".min.ts")
        || basename.ends_with(".snap")
        || basename.ends_with(".snapshot")
        || basename.contains(".generated.")
        || basename.ends_with("_generated.rs")
    {
        return rejected(BabysitEligibilityReason::LockMinifiedOrSnapshot);
    }
    let extension = basename.rsplit_once('.').map(|(_, extension)| extension);
    let allowed = matches!(
        extension,
        Some(
            "ts" | "tsx"
                | "mts"
                | "js"
                | "jsx"
                | "mjs"
                | "rs"
                | "py"
                | "go"
                | "java"
                | "c"
                | "cc"
                | "cpp"
                | "cxx"
                | "h"
                | "hpp"
        )
    );
    let detected_allowed = matches!(
        detected_language.map(str::to_ascii_lowercase).as_deref(),
        Some("typescript" | "javascript" | "rust" | "python" | "go" | "java" | "c" | "cpp")
    );
    if !allowed && !detected_allowed {
        return rejected(BabysitEligibilityReason::UnsupportedLanguage);
    }
    BabysitEligibility {
        eligible: true,
        reason: None,
    }
}
