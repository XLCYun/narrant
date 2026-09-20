//! 测试识别仅依赖明确的文件约定，不猜测函数名或混合文件用途。
use std::path::Path;

/// 支持的语言文件使用 TS/JS 编辑器语言标识；JavaScript 允许 JSX。
pub(crate) fn language(path: &Path) -> Option<&'static str> {
    match path.extension()?.to_str()? {
        "ts" | "tsx" | "mts" | "cts" => Some("typescript"),
        "js" | "jsx" | "mjs" | "cjs" => Some("javascript"),
        _ => None,
    }
}

/// 只接受已校验的仓库相对路径；明确测试文件在关闭测试选项时整体排除。
pub(super) fn is_test(path: &Path) -> bool {
    language(path).is_some()
        && (path.components().any(|c| c.as_os_str() == "__tests__")
            || path
                .file_stem()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.ends_with(".test") || s.ends_with(".spec")))
}
