//! 同源 Ruff / ty 的只读 Python 分析核心；导航仅提供候选，关系裁决由调用方完成。
mod loading;
mod symbols;
mod system;
pub use symbols::StandardSymbolIdentity;

use anyhow::{Result, bail, ensure};
use ruff_db::{
    files::{File, system_path_to_file},
    parsed::{ParsedModuleRef, parsed_module},
    source::source_text,
};
use ruff_python_ast::Stmt;
use ruff_python_codegen::{Generator, Indentation, Mode};
use ruff_source_file::LineEnding;
use ruff_text_size::{Ranged, TextRange, TextSize};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};
use ty_project::{Db, ProjectDatabase};

/// 上层先解析配置，再显式给出单个项目、源码根和目标版本；路径相对于仓库。
#[derive(Debug, Clone)]
pub struct ProjectInput {
    pub repository: PathBuf,
    pub project_root: PathBuf,
    pub source_roots: Vec<PathBuf>,
    pub python_minor: u8,
}

/// 来源不包含临时 checkout 的绝对路径，标准声明保留固定 revision 的内嵌相对路径。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum SourceOrigin {
    Repository(PathBuf),
    StandardDeclaration(String),
}

/// 原生定义候选保留完整/焦点范围与原文；不代表确定的运行时调用目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionCandidate {
    pub origin: SourceOrigin,
    pub focus: TextRange,
    pub full: TextRange,
    pub source: String,
    pub standard_symbol: Option<StandardSymbolIdentity>,
}

/// 保留原生所有候选，不把缺依赖或多平台声明归约为单一目标。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionQuery {
    pub selection: Option<TextRange>,
    pub candidates: Vec<DefinitionCandidate>,
}

/// 数据库及文件集合属于单次固定侧，公共接口没有重载或文件写入入口。
pub struct PythonProject {
    db: ProjectDatabase,
    files: BTreeSet<PathBuf>,
}

impl PythonProject {
    /// 校验输入并只读冻结可达仓库源码，直接创建原生数据库。
    pub fn load(input: ProjectInput) -> Result<Self> {
        loading::load(input)
    }

    /// 读取与 AST/导航完全同源的冻结原文。
    pub fn source(&self, path: &Path) -> Result<String> {
        self.checked_source(self.file(path)?)
    }

    /// 返回同一数据库产生的 Ruff AST；语法或版本错误为显式失败。
    pub fn parse(&self, path: &Path) -> Result<ParsedModuleRef> {
        let file = self.file(path)?;
        self.checked_source(file)?;
        let program = self
            .db
            .project()
            .program(&self.db)
            .program_file(&self.db, file);
        let parsed = parsed_module(&self.db, program.python_file(&self.db)).load(&self.db);
        ensure!(
            parsed.errors().is_empty(),
            "Python 语法错误：{:?}",
            parsed.errors()
        );
        ensure!(
            parsed.unsupported_syntax_errors().is_empty(),
            "Python 目标版本不支持语法：{:?}",
            parsed.unsupported_syntax_errors()
        );
        Ok(parsed)
    }

    /// 查询 UTF-8 字节位置；仅返回候选，绑定与关系裁决留给图适配器。
    pub fn definitions(&self, path: &Path, offset: usize) -> Result<DefinitionQuery> {
        let file = self.file(path)?;
        let source = self.checked_source(file)?;
        ensure!(
            offset < source.len() && source.is_char_boundary(offset),
            "Python 查询位置越界或不是 UTF-8 边界"
        );
        self.parse(path)?;
        let program = self
            .db
            .project()
            .program(&self.db)
            .program_file(&self.db, file);
        let Some(query) = ty_ide::goto_definition(&self.db, program, TextSize::try_from(offset)?)
        else {
            return Ok(DefinitionQuery {
                selection: None,
                candidates: Vec::new(),
            });
        };
        let candidates = query
            .value
            .into_iter()
            .map(|target| {
                let origin = self.origin(target.file())?;
                let source = self.checked_source(target.file())?;
                ensure!(
                    target.full_range().end().to_usize() <= source.len(),
                    "Python 导航范围越界"
                );
                let standard_symbol = if let SourceOrigin::StandardDeclaration(path) = &origin {
                    let program = self
                        .db
                        .project()
                        .program(&self.db)
                        .program_file(&self.db, target.file());
                    let parsed =
                        parsed_module(&self.db, program.python_file(&self.db)).load(&self.db);
                    symbols::standard_symbol(path, &parsed.syntax().body, target.focus_range())
                } else {
                    None
                };
                Ok(DefinitionCandidate {
                    standard_symbol,
                    origin,
                    focus: target.focus_range(),
                    full: target.full_range(),
                    source,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(DefinitionQuery {
            selection: Some(query.range.range()),
            candidates,
        })
    }

    /// 文件查询只接受已冻结的相对源码路径，保留链接的词法身份。
    fn file(&self, path: &Path) -> Result<File> {
        ensure!(
            self.files.contains(path),
            "Python 源码未加载：{}",
            path.display()
        );
        system_path_to_file(&self.db, loading::system_path(path)?).map_err(Into::into)
    }

    /// 读取错误不能由上游的空 AST 恢复掩盖。
    fn checked_source(&self, file: File) -> Result<String> {
        let text = source_text(&self.db, file);
        ensure!(
            text.read_error().is_none(),
            "Python 原文读取失败：{:?}",
            text.read_error()
        );
        Ok(text.as_str().to_owned())
    }

    /// 将内嵌标准声明与词法仓库路径映射到可跨 checkout 使用的来源。
    fn origin(&self, file: File) -> Result<SourceOrigin> {
        let path = file.path(&self.db);
        if let Some(vendored) = path.as_vendored_path() {
            return Ok(SourceOrigin::StandardDeclaration(
                vendored.as_str().replace('\\', "/"),
            ));
        }
        if let Some(path) = path.as_system_path() {
            let relative = path.strip_prefix(loading::SNAPSHOT_ROOT)?;
            let relative = PathBuf::from(relative.as_str());
            ensure!(
                self.files.contains(&relative),
                "Python 定义不属于已加载仓库源码"
            );
            return Ok(SourceOrigin::Repository(relative));
        }
        bail!("Python 定义来源不是仓库或固定标准声明")
    }
}

/// 与旧实现相同的 AST 打印模式，后续内容投影可直接复用。
pub fn normalize_statement(statement: &Stmt) -> String {
    Generator::new(&Indentation::default(), LineEnding::Lf)
        .with_mode(Mode::AstUnparse)
        .stmt(statement)
}

#[cfg(test)]
#[path = "tests/projection.rs"]
mod projection_tests;
#[cfg(test)]
#[path = "tests/core.rs"]
mod tests;
