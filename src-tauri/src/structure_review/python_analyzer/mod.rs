//! Python 结构分析：本地语法内容与受控语义证据共用公开分析接口。

mod bindings;
mod fields;
mod known;
mod project;
mod references;
mod session;
mod syntax;

use super::code_graph::{
    AnalysisError, AnalysisInput, AnalysisOptions, CodeGraph, CodeRoot, LanguageAnalyzer,
};
use crate::lsp::{file_uri, CancellationCheck};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
    sync::Arc,
    time::Duration,
};

/// 每次分析使用独立会话，工具路径由固定支持包或显式开发配置提供。
#[derive(Clone)]
pub struct PythonLanguageAnalyzer {
    executable: PathBuf,
    timeout: Duration,
    cancelled: CancellationCheck,
}

impl Default for PythonLanguageAnalyzer {
    /// 开发入口也只读取显式工具路径，不寻找项目解释器或自动下载。
    fn default() -> Self {
        Self {
            executable: std::env::var_os("NARRANT_TY_PATH")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    PathBuf::from(if cfg!(windows) {
                        ".tools/python/ty.exe"
                    } else {
                        ".tools/python/ty"
                    })
                }),
            timeout: Duration::from_secs(30),
            cancelled: Arc::new(|| false),
        }
    }
}

impl PythonLanguageAnalyzer {
    /// 指定固定版本 ty 可执行文件，不读取用户 Python 环境。
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
            ..Self::default()
        }
    }
    /// 两侧沿用同一请求截止时间，不因进度通知延长等待。
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
    /// 应用取消贯穿源码读取、解析和语义查询。
    pub fn with_cancellation(mut self, cancelled: CancellationCheck) -> Self {
        self.cancelled = cancelled;
        self
    }
    /// 取消始终是硬失败，不能返回部分图。
    fn check_cancelled(&self) -> Result<()> {
        if (self.cancelled)() {
            return Err(AnalysisError::cancelled("Python 结构分析已取消"));
        }
        Ok(())
    }
}

impl LanguageAnalyzer for PythonLanguageAnalyzer {
    /// 原文只读取一次，规范内容与 LSP 文档始终来自同一份源码。
    fn analyze(&self, input: &AnalysisInput) -> Result<CodeGraph> {
        self.check_cancelled()?;
        let repository = input.repository_root.canonicalize().map_err(failure)?;
        if !repository.is_dir() {
            return Err(failure("仓库根路径不是目录"));
        }
        let mut selected = BTreeSet::new();
        for relative in &input.files {
            if relative.as_os_str().is_empty()
                || relative
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err(failure("源码必须是仓库内规范相对路径"));
            }
            if !matches!(
                relative.extension().and_then(|value| value.to_str()),
                Some("py" | "pyi")
            ) {
                return Err(failure("Python 分析器仅接受 .py/.pyi"));
            }
            let path = repository.join(relative);
            let actual = project::checked_path(&repository, &path)?;
            if !path.is_file() || project::environment_path(&repository, &path) {
                return Err(failure("选中的 Python 路径不是仓库源码文件"));
            }
            if input.options.include_tests
                || (!is_test(relative)
                    && !is_test(actual.strip_prefix(&repository).map_err(failure)?))
            {
                selected.insert(actual);
            }
        }
        let mut groups = BTreeMap::new();
        for path in selected {
            self.check_cancelled()?;
            let source = project::read_source(&path)?;
            let project = project::Project::for_file(&repository, &path, &source)?;
            let document = syntax::Document::parse(
                path.strip_prefix(&repository).map_err(failure)?,
                &source,
                project.minor,
            )?;
            groups
                .entry(project)
                .or_insert_with(Vec::new)
                .push((path, source, document));
        }
        let mut root = CodeRoot::default();
        let mut nodes = vec![];
        let mut externals = BTreeMap::new();
        for (project, documents) in groups {
            self.check_cancelled()?;
            let executable = self.executable.canonicalize().map_err(failure)?;
            let mut session = session::Session::start(
                &executable,
                &repository,
                &project,
                self.timeout,
                self.cancelled.clone(),
            )?;
            let mut parsed = BTreeMap::new();
            for (path, source, document) in documents {
                session
                    .client()
                    .open_document(&file_uri(&path).map_err(lsp_failure)?, "python", 1, &source)
                    .map_err(lsp_failure)?;
                parsed.insert(path.canonicalize().map_err(failure)?, document);
            }
            let (documents, symbols) = references::analyze(
                &mut session,
                &repository,
                &project,
                parsed,
                input.options.include_tests,
                &self.cancelled,
            )?;
            for document in documents {
                root.contains.extend(document.root);
                nodes.extend(document.nodes);
            }
            externals.extend(symbols);
            session.shutdown()?;
        }
        self.check_cancelled()?;
        let ids = nodes
            .iter()
            .map(|node| node.data().id.clone())
            .collect::<BTreeSet<_>>();
        for node in &mut nodes {
            for reference in &mut syntax::node_data_mut(node).references {
                if let super::code_graph::ReferenceTarget::External { symbol } = &reference.target {
                    let id = super::code_graph::NodeId(symbol.clone());
                    if ids.contains(&id) {
                        reference.target = super::code_graph::ReferenceTarget::Node(id);
                    }
                }
            }
        }
        externals.retain(|symbol, _| !ids.contains(&super::code_graph::NodeId(symbol.clone())));
        Ok(CodeGraph::new(root, nodes)
            .map_err(failure)?
            .with_external_symbols(externals))
    }
}

/// 包准备与实际提取使用同一文件级测试约定。
pub(crate) fn needs_tools(path: &str, options: AnalysisOptions) -> bool {
    options.include_tests || !is_test(Path::new(path))
}

/// 只观察仓库相对路径，不让 checkout 外祖先目录影响选择。
fn is_test(relative: &Path) -> bool {
    relative
        .components()
        .any(|part| part.as_os_str() == "tests")
        || relative
            .file_stem()
            .and_then(|value| value.to_str())
            .is_some_and(|name| {
                name.starts_with("test_") || name.ends_with("_test") || name == "conftest"
            })
}

/// 适配器在公共边界统一报告错误，不能伪装成成功空图。
type Result<T> = std::result::Result<T, AnalysisError>;

/// 保留底层可读错误与 Python 上下文。
fn failure(message: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::new(format!("Python 结构分析：{message}"))
}

/// 转换协议错误时保留取消类别，应用不会将取消误报为工具损坏。
fn lsp_failure(error: crate::lsp::LspError) -> AnalysisError {
    if error.is_cancelled() {
        AnalysisError::cancelled(error.to_string())
    } else {
        failure(error)
    }
}

#[cfg(all(test, unix))]
#[path = "tests/graph.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/native.rs"]
mod native_tests;
