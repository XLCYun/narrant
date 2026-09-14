//! 分析运行的说明与图分开传递，不进入节点内容或差异比较。
use super::code_graph::CodeGraph;
use std::path::PathBuf;

/// 一条可按侧展示的分析说明；路径始终相对于本侧仓库。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct AnalysisNote {
    pub code: String,
    pub message: String,
    pub file: Option<PathBuf>,
    pub project: Option<PathBuf>,
}

/// 一次语言分析的完整结果；说明不会改变图的语义身份。
#[derive(Clone, Debug)]
pub struct AnalysisReport {
    pub graph: CodeGraph,
    pub notes: Vec<AnalysisNote>,
}
