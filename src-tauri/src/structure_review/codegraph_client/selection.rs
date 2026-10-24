use std::path::Path;
/// TS/JS 文件测试开关与 helper 保持一致；全被排除的语言组不启动或准备工具。
pub(crate) fn typescript_needs_tools(
    path: &Path,
    options: crate::structure_review::code_graph::AnalysisOptions,
) -> bool {
    options.include_tests
        || !(path
            .components()
            .any(|part| part.as_os_str() == "__tests__")
            || path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| stem.ends_with(".test") || stem.ends_with(".spec")))
}

/// Python 文件级测试过滤与 helper 一致，全被排除的语言组不准备或启动支持包。
pub(crate) fn python_needs_tools(
    path: &Path,
    options: crate::structure_review::code_graph::AnalysisOptions,
) -> bool {
    options.include_tests
        || !(path.components().any(|part| part.as_os_str() == "tests")
            || path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .is_some_and(|stem| {
                    stem.starts_with("test_") || stem.ends_with("_test") || stem == "conftest"
                }))
}
