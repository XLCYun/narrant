use super::*;

/// 公共入口先校验选集路径，过滤测试后无需存在语义工具。
#[test]
fn excluded_and_empty_inputs_do_not_require_tools_but_validate_paths() {
    let repository = tempfile::tempdir().unwrap();
    fs::write(
        repository.path().join("hidden.test.ts"),
        "export function test() {};",
    )
    .unwrap();
    let analyzer = TypeScriptLanguageAnalyzer::new("/missing/node", "/missing/runtime");
    let mut input = AnalysisInput {
        repository_root: repository.path().into(),
        files: vec!["hidden.test.ts".into()],
        options: Default::default(),
    };
    assert!(analyzer.analyze(&input).unwrap().root().contains.is_empty());
    fs::create_dir(repository.path().join("directory.test.ts")).unwrap();
    fs::write(repository.path().join("invalid.test.ts"), [0xff]).unwrap();
    for file in [
        "../hidden.test.ts",
        "missing.test.ts",
        "directory.test.ts",
        "invalid.test.ts",
    ] {
        input.files = vec![file.into()];
        assert!(analyzer.analyze(&input).is_err(), "{file}");
    }
    #[cfg(unix)]
    {
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), repository.path().join("link.test.ts")).unwrap();
        input.files = vec!["link.test.ts".into()];
        assert!(analyzer.analyze(&input).is_err());
    }
    input.files.clear();
    assert!(analyzer.analyze(&input).unwrap().root().contains.is_empty());
}

#[path = "graph_service.rs"]
mod graph_service;

/// 取消标记和缺失工具都保持硬失败，不被空结果掩盖。
#[test]
fn public_cancellation_and_missing_tool_are_errors() {
    let repository = tempfile::tempdir().unwrap();
    fs::write(repository.path().join("main.ts"), "function caller() {}").unwrap();
    let input = AnalysisInput {
        repository_root: repository.path().into(),
        files: vec!["main.ts".into()],
        options: Default::default(),
    };
    let analyzer = TypeScriptLanguageAnalyzer::new("/missing/node", "/missing/runtime");
    assert!(!analyzer.analyze(&input).unwrap_err().is_cancelled());
    assert!(analyzer
        .with_cancellation(Arc::new(|| true))
        .analyze(&input)
        .unwrap_err()
        .is_cancelled());
}

#[cfg(unix)]
#[path = "graph_lifecycle.rs"]
mod graph_lifecycle;
