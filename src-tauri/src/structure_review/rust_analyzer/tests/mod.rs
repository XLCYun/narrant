mod crate_roots;
mod integration;
mod repository_scope;
mod test_scope;

use std::fs;

use super::RustLanguageAnalyzer;
use crate::structure_review::code_graph::{AnalysisInput, LanguageAnalyzer};

/// 文件范围先校验；空范围无需启动工具，错误路径也不能变成成功空图。
#[test]
fn validates_sources_before_starting_the_server() {
    let directory = tempfile::tempdir().unwrap();
    let repository = directory.path().to_path_buf();
    let analyzer = RustLanguageAnalyzer::new(directory.path().join("missing-server"));
    assert!(analyzer
        .analyze(&AnalysisInput {
            options: crate::structure_review::code_graph::AnalysisOptions {
                include_tests: true
            },
            repository_root: repository.clone(),
            files: vec![]
        })
        .unwrap()
        .root()
        .contains
        .is_empty());
    for file in ["../outside.rs", "missing.rs", ""] {
        assert!(analyzer
            .analyze(&AnalysisInput {
                options: crate::structure_review::code_graph::AnalysisOptions {
                    include_tests: true
                },
                repository_root: repository.clone(),
                files: vec![file.into()]
            })
            .is_err());
    }
    fs::write(repository.join("lib.rs"), "fn f() {}").unwrap();
    let error = analyzer
        .analyze(&AnalysisInput {
            options: crate::structure_review::code_graph::AnalysisOptions {
                include_tests: true,
            },
            repository_root: repository,
            files: vec!["lib.rs".into()],
        })
        .unwrap_err();
    assert!(error.message.contains("二进制不可用"));
}
