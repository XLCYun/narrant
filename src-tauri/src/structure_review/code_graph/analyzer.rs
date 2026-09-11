use std::path::PathBuf;

use super::{AnalysisError, CodeGraph};

/// 一侧已准备好的仓库目录，以及本次需要提取对象的文件范围。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalysisInput {
    /// 对应确定代码状态的仓库根目录，分析期间保持内容一致。
    pub repository_root: PathBuf,
    /// 仓库内的相对路径，仅列出当前侧存在、需要分析的文件。
    pub files: Vec<PathBuf>,
}

/// 各语言实现此接口，提取指定文件范围中的全部支持对象。
pub trait LanguageAnalyzer {
    /// 基于指定仓库状态分析文件，可以使用 LSP 并读取其它项目文件作为上下文。
    ///
    /// 实现负责校验相对路径不越出仓库，并将请求文件的读取或提取失败返回为错误。
    /// 不存在于当前侧的文件不列入输入；选中的空文件仍需列入。
    /// 前后两侧须使用相同分析约定，节点身份不得依赖 checkout 的绝对路径。
    fn analyze(&self, input: &AnalysisInput) -> Result<CodeGraph, AnalysisError>;
}
