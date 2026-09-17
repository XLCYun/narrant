//! TypeScript / JavaScript 适配器：语法文档与后续语义查询共享稳定接口。

mod identity;
pub(crate) mod lsp;
pub(crate) mod project;
mod references;
mod runtime;
pub(crate) mod syntax;
mod test_code;
pub(crate) mod text;
pub(crate) mod tools;

use super::code_graph::{AnalysisError, AnalysisInput, CodeGraph, CodeRoot, LanguageAnalyzer};
use crate::lsp::CancellationCheck;
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, PathBuf},
    sync::Arc,
    time::Duration,
};

/// 固定工具配置可克隆；每次调用读取传入的 Git workspace 并独立管理子进程。
#[derive(Clone)]
pub struct TypeScriptLanguageAnalyzer {
    tools: tools::TypeScriptToolchain,
    timeout: Duration,
    cancelled: CancellationCheck,
}

impl Default for TypeScriptLanguageAnalyzer {
    /// 默认使用明确安装的开发工具；分析本身不下载任何依赖。
    fn default() -> Self {
        Self {
            tools: tools::TypeScriptToolchain::default(),
            timeout: Duration::from_secs(30),
            cancelled: Arc::new(|| false),
        }
    }
}

impl TypeScriptLanguageAnalyzer {
    /// 指定固定 Node 可执行文件与包含四个固定包的运行时目录。
    pub fn new(node: impl Into<PathBuf>, runtime_root: impl Into<PathBuf>) -> Self {
        Self {
            tools: tools::TypeScriptToolchain {
                node: node.into(),
                runtime_root: runtime_root.into(),
            },
            ..Self::default()
        }
    }

    /// 注入本次操作的取消观察，覆盖项目准备、查询与最终快照校验。
    pub fn with_cancellation(mut self, cancelled: CancellationCheck) -> Self {
        self.cancelled = cancelled;
        self
    }

    /// 调整单次工具或语义请求的最大等待时间，前后两侧应采用同一设置。
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// 取消是硬失败，不能返回一张成功的空图。
    fn check_cancelled(&self) -> Result<()> {
        if (self.cancelled)() {
            return Err(AnalysisError::cancelled("TypeScript 结构分析已取消"));
        }
        Ok(())
    }
}

impl LanguageAnalyzer for TypeScriptLanguageAnalyzer {
    /// 提取选集图；上下文文档只在定义命中时解析。
    fn analyze(&self, input: &AnalysisInput) -> Result<CodeGraph> {
        self.check_cancelled()?;
        let repository = input.repository_root.canonicalize().map_err(failure)?;
        if !repository.is_dir() {
            return Err(failure("仓库根路径不是目录"));
        }
        let mut selected = BTreeMap::new();
        // 所有输入先校验，测试过滤和空图短路不能掩盖越界或不支持的路径。
        for relative in &input.files {
            self.check_cancelled()?;
            if relative.as_os_str().is_empty()
                || relative.is_absolute()
                || relative
                    .components()
                    .any(|part| !matches!(part, Component::Normal(_)))
            {
                return Err(failure("文件必须是仓库内的规范相对路径"));
            }
            let path = repository.join(relative);
            if !path
                .canonicalize()
                .map_err(failure)?
                .starts_with(&repository)
            {
                return Err(failure("选中文件的符号链接指向仓库外部"));
            }
            if test_code::language(&path).is_none() {
                return Err(failure("TypeScript 分析器收到不支持的文件扩展名"));
            }
            let source = fs::read_to_string(&path).map_err(failure)?;
            if input.options.include_tests || !test_code::is_test(relative) {
                selected.insert(path, source);
            }
        }
        if selected.is_empty() {
            return CodeGraph::new(CodeRoot::default(), vec![]).map_err(failure);
        }
        let paths = selected.keys().cloned().collect::<Vec<_>>();
        let project = project::PreparedProject::prepare(
            &repository,
            &paths,
            &self.tools,
            self.timeout,
            &self.cancelled,
        )?
        .ok_or_else(|| failure("非空选集缺少项目上下文"))?;
        let mut documents = BTreeMap::new();
        for (path, source) in selected {
            let path = project.checked_path(&path)?;
            if project.source(&path)? != source {
                return Err(failure("准备期间选中源文件已改变"));
            }
            let document = syntax::Document::parse(
                &project.workspace_root,
                path.clone(),
                source,
                input.options,
            )?;
            documents.insert(path, document);
        }
        validate_snapshots(&project, &self.cancelled)?;
        let mut session = lsp::TypeScriptLsp::start(project, self.timeout, self.cancelled.clone())?;
        let graph = references::analyze(&mut session, documents, &self.cancelled)?;
        validate_snapshots(session.project(), &self.cancelled)?;
        self.check_cancelled()?;
        session.shutdown()?;
        Ok(graph)
    }
}

/// 验证 Git workspace 的全部第一方源码仍等于分析前的内存快照，不只检查输出选集。
fn validate_snapshots(
    project: &project::PreparedProject,
    cancelled: &CancellationCheck,
) -> Result<()> {
    project.check_access()?;
    for snapshot in project.sources.values() {
        if cancelled() {
            return Err(AnalysisError::cancelled("TypeScript 结构分析已取消"));
        }
        if !snapshot
            .path
            .canonicalize()
            .map_err(failure)?
            .starts_with(&project.workspace_root)
            || fs::read_to_string(&snapshot.path).map_err(failure)? != snapshot.source
        {
            return Err(failure(format!(
                "分析期间源文件已改变：{}",
                snapshot.path.display()
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "tests/references.rs"]
mod reference_tests;

/// 适配器内部统一返回公共分析错误，解析失败不能伪装为空图。
pub(crate) type Result<T> = std::result::Result<T, AnalysisError>;

/// 为各层保留统一的可读错误入口。
pub(crate) fn failure(message: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::new(message.to_string())
}

#[cfg(test)]
#[path = "tests/config_dir_fixture.rs"]
mod config_dir_fixture;
