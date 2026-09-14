use super::{AnalysisNote, Graph};
use crate::structure_review::{
    analysis,
    code_graph::{CodeGraph, CodeRoot, ExternalSymbol, SymbolScope},
};

/// WebView 投影保留原始身份，以单独的名称与范围字段支持任意语言的展示。
#[test]
fn serializes_external_symbol_names_and_repository_scope() {
    let graph = CodeGraph::new(CodeRoot::default(), vec![])
        .unwrap()
        .with_external_symbols(
            [
                (
                    "opaque:%2F%25".into(),
                    ExternalSymbol {
                        name: "模块::函数".into(),
                        scope: SymbolScope::Repository,
                    },
                ),
                (
                    "dependency".into(),
                    ExternalSymbol {
                        name: "Arc<T>::new".into(),
                        scope: SymbolScope::Dependency,
                    },
                ),
            ]
            .into(),
        );
    let json = serde_json::to_value(Graph::from(&graph)).unwrap();
    assert_eq!(
        json["externalSymbols"],
        serde_json::json!({
            "opaque:%2F%25": { "name": "模块::函数", "scope": "repository" },
            "dependency": { "name": "Arc<T>::new", "scope": "dependency" },
        })
    );
    assert_eq!(json["nodes"], serde_json::json!([]));
}

/// 分析说明只投影公开字段，逐侧路径采用稳定的传输格式。
#[test]
fn projects_structured_analysis_notes_separately_from_graphs() {
    let note = analysis::AnalysisNote {
        code: "typescript.TS2307".into(),
        message: "缺少声明".into(),
        file: Some(std::path::PathBuf::from("src").join("main.ts")),
        project: Some("tsconfig.json".into()),
    };
    let json = serde_json::to_value(AnalysisNote::from(&note)).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "code": "typescript.TS2307",
            "message": "缺少声明",
            "file": "src/main.ts",
            "project": "tsconfig.json",
        })
    );
}
