use crate::structure_review::{
    code_graph::{AnalysisInput, CodeGraph, LanguageAnalyzer, RustDependencyScope},
    codegraph_client::CodeGraphAnalyzer,
};
use std::{fs, sync::Arc};

/// 使用正常构建的真实支持包，测试不覆盖用户安装或修改真实项目。
fn analyzer() -> CodeGraphAnalyzer {
    CodeGraphAnalyzer::from_pack("rust", &super::candidate::root("rust")).unwrap()
}

/// 同一代表性多文件项目覆盖三档、cfg/test、依赖隔离及失败路径。
#[test]
#[ignore = "需要真实 Rust 优化支持包及已有 rust-src"]
fn production_profiles_keep_graph_policy_and_readonly_project() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("Cargo.toml"), "[package]\nname=\"production_fixture\"\nversion=\"0.1.0\"\nedition=\"2021\"\n[lib]\npath=\"lib.rs\"\n").unwrap();
    let source = "mod model; pub fn entry() -> u32 { model::target() } #[cfg(any())] fn inactive() {} #[test] fn check() {}";
    fs::write(root.path().join("lib.rs"), source).unwrap();
    fs::write(root.path().join("model.rs"), "pub fn target() -> u32 { 1 }").unwrap();
    let input = AnalysisInput {
        repository_root: root.path().into(),
        files: vec!["lib.rs".into(), "model.rs".into()],
        options: Default::default(),
    };
    let analyzer = analyzer();
    let mut baseline = None;
    for scope in [
        RustDependencyScope::RepositoryOnly,
        RustDependencyScope::StandardLibrary,
        RustDependencyScope::AllDependencies,
    ] {
        let now = std::time::Instant::now();
        let graph = analyzer
            .clone()
            .with_dependency_scope(scope)
            .analyze(&input)
            .unwrap();
        if let Some(baseline) = &baseline {
            let old: &CodeGraph = baseline;
            assert!(old.diff(&graph).unwrap().nodes.is_empty());
        }
        baseline = Some(graph);
        let mut included = input.clone();
        included.options.include_tests = true;
        let complete = analyzer
            .clone()
            .with_dependency_scope(scope)
            .analyze(&included)
            .unwrap();
        assert!(!baseline
            .as_ref()
            .unwrap()
            .diff(&complete)
            .unwrap()
            .nodes
            .is_empty());
        assert_eq!(
            fs::read_to_string(root.path().join("lib.rs")).unwrap(),
            source
        );
        assert!(!root.path().join("Cargo.lock").exists() && !root.path().join("target").exists());
        eprintln!(
            "production {scope:?} two test policies: {:?}",
            now.elapsed()
        );
    }
    fs::write(root.path().join("lib.rs"), "fn broken( {").unwrap();
    let error = analyzer
        .clone()
        .with_dependency_scope(RustDependencyScope::RepositoryOnly)
        .analyze(&input)
        .unwrap_err();
    assert!(
        !error.is_cancelled() && error.message.contains("analysis_failed"),
        "{error}"
    );
    let error = analyzer
        .with_cancellation(Arc::new(|| true))
        .analyze(&input)
        .unwrap_err();
    assert!(error.is_cancelled());
}
