//! 旧 native.rs 的业务断言迁至实际 Python helper；生产双平台安装验收仍待后续门禁。
use super::python_graph::{analyzer, named};
use crate::structure_review::code_graph::{
    AnalysisInput, AnalysisOptions, CodeGraph, LanguageAnalyzer,
};
use std::fs;
use std::path::PathBuf;

/// 使用真实二进制分析临时仓库，不依赖宿主 Python 或项目依赖安装。
fn analyze_native(files: &[(&str, &str)], selected: &[&str]) -> CodeGraph {
    analyze_with_options(files, selected, Default::default())
}

/// 在公开分析接口传递测试选项，仍通过同一个真实 CodeGraph 包。
fn analyze_with_options(
    files: &[(&str, &str)],
    selected: &[&str],
    options: AnalysisOptions,
) -> CodeGraph {
    let root = tempfile::tempdir().unwrap();
    for (name, source) in files {
        let path = root.path().join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
    analyzer()
        .analyze(&AnalysisInput {
            repository_root: root.path().into(),
            files: selected.iter().map(PathBuf::from).collect(),
            options,
        })
        .unwrap()
}

/// overload 的有效文档属于对应签名，修改文档必须修改合并后的 Callable。
#[test]
#[ignore = "需要已构建 Python helper，单独串行运行"]
fn native_overload_docstrings_participate_in_content_comparison() {
    for name in ["api.py", "api.pyi"] {
        let mut source = "from typing import overload\n@overload\ndef convert(x: int) -> int:\n    \"old doc\"\n    ...\n@overload\ndef convert(x: str) -> str: ...\n".to_owned();
        if name.ends_with(".py") {
            source.push_str("def convert(x): return x\n");
        }
        let before = analyze_native(&[(name, &source)], &[name]);
        let after = analyze_native(&[(name, &source.replace("old doc", "new doc"))], &[name]);
        let difference = before.diff(&after).unwrap();
        assert_eq!(difference.nodes.len(), 1, "{name}: {difference:?}");
        assert_eq!(difference.nodes[0].id, named(&before, "convert").data().id);
        assert_eq!(
            difference.nodes[0].content,
            Some(crate::structure_review::code_graph::ChangeKind::Modified)
        );
    }
}
