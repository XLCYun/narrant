use std::path::PathBuf;

use super::{AnalysisError, CodeGraph};

/// Rust 语义分析允许加载的依赖范围；不改变 Code Graph 的文件范围。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RustDependencyScope {
    /// 只加载仓库项目，跳过标准库和第三方依赖。
    RepositoryOnly,
    /// 允许加载标准库及其内部依赖，同时跳过项目依赖元数据。
    #[default]
    StandardLibrary,
    /// 允许加载标准库和项目依赖。
    AllDependencies,
}

/// 各语言共用的分析范围选项；默认只提取未被明确判定为测试专用的代码。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AnalysisOptions {
    /// 启用后也分析测试代码；关闭时由语言实现保守识别，无法确定的代码继续保留。
    pub include_tests: bool,
}

/// 一侧已准备好的仓库目录，以及本次需要提取对象的文件范围。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AnalysisInput {
    /// 对应确定代码状态的仓库根目录，分析期间保持内容一致。
    pub repository_root: PathBuf,
    /// 仓库内的相对路径，仅列出当前侧存在、需要分析的文件。
    pub files: Vec<PathBuf>,
    /// 前后两侧必须使用相同选项，避免将分析范围变化误报为代码变化。
    pub options: AnalysisOptions,
}

/// 各语言实现此接口，提取指定文件和分析选项范围中的全部支持对象。
pub trait LanguageAnalyzer {
    /// 基于指定仓库状态分析文件，可以使用 LSP 并读取其它项目文件作为上下文。
    ///
    /// 实现负责校验相对路径不越出仓库，并将请求文件的读取或提取失败返回为错误。
    /// 不存在于当前侧的文件不列入输入；选中的空文件仍需列入。
    /// 前后两侧须使用相同分析约定，节点身份不得依赖 checkout 的绝对路径。
    fn analyze(&self, input: &AnalysisInput) -> Result<CodeGraph, AnalysisError>;
}
