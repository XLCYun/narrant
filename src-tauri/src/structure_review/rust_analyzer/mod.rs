//! Rust 代码图分析：本地语法库提取源码对象，独立 rust-analyzer 会话通过 LSP 解析语义。

mod lsp;
mod references;
mod syntax;

use std::{
    collections::BTreeMap,
    fmt::Display,
    fs,
    path::{Component, PathBuf},
};

use super::code_graph::{AnalysisError, AnalysisInput, CodeGraph, CodeRoot, LanguageAnalyzer};

type Result<T> = std::result::Result<T, AnalysisError>;

/// 已有 rust-analyzer 可执行文件的轻量配置；每次分析独立加载一侧仓库快照。
///
/// 二进制由调用方提供，例如开发仓库的 .tools/rust-analyzer。分析同步执行，可能加载
/// Cargo 依赖及运行构建脚本，应用应在其工作线程和已有 Workspace Trust 范围内调用。
#[derive(Clone, Debug)]
pub struct RustLanguageAnalyzer {
    executable: PathBuf,
}

impl RustLanguageAnalyzer {
    /// 指定已有服务器路径，暂不下载、搜索或安装工具。
    pub fn new(executable: impl Into<PathBuf>) -> Self {
        Self {
            executable: executable.into(),
        }
    }
}

impl LanguageAnalyzer for RustLanguageAnalyzer {
    /// 校验文件范围，固定文档内容，提取对象与关联后交由公共模块验证图结构。
    fn analyze(&self, input: &AnalysisInput) -> Result<CodeGraph> {
        let repository = input.repository_root.canonicalize().map_err(failure)?;
        if !repository.is_dir() {
            return Err(failure("仓库根路径不是目录"));
        }
        let mut sources = BTreeMap::new();
        for relative in &input.files {
            if relative.as_os_str().is_empty()
                || relative.is_absolute()
                || relative
                    .components()
                    .any(|part| matches!(part, Component::ParentDir))
            {
                return Err(failure(format!(
                    "文件必须是仓库内的相对路径：{}",
                    relative.display()
                )));
            }
            let path = repository
                .join(relative)
                .canonicalize()
                .map_err(|error| failure(format!("无法读取 {}：{error}", relative.display())))?;
            if !path.starts_with(&repository) {
                return Err(failure("选中文件的符号链接指向仓库外部"));
            }
            if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
                return Err(failure(format!(
                    "Rust 分析器只接受 .rs 文件：{}",
                    relative.display()
                )));
            }
            sources.insert(path.clone(), fs::read_to_string(&path).map_err(failure)?);
        }
        if sources.is_empty() {
            return CodeGraph::new(CodeRoot::default(), Vec::new()).map_err(failure);
        }
        // 保留启动器的文件名与符号链接，避免 rustup 等代理因 argv[0] 改变而失效。
        let executable = if self.executable.is_absolute() {
            self.executable.clone()
        } else {
            std::env::current_dir()
                .map_err(failure)?
                .join(&self.executable)
        };
        let metadata = fs::metadata(&executable).map_err(|error| {
            failure(format!(
                "rust-analyzer 二进制不可用 {}：{error}",
                executable.display()
            ))
        })?;
        if !metadata.is_file() {
            return Err(failure(format!(
                "rust-analyzer 二进制路径不是文件：{}",
                executable.display()
            )));
        }
        let mut documents = Vec::new();
        for (path, source) in sources {
            documents.push(syntax::Document::parse(&repository, path, source)?);
        }
        let mut client = lsp::RustLsp::start(&executable, &repository)?;
        for document in &documents {
            client.open(&document.uri, &document.source)?;
        }
        client.wait_ready()?;
        let graph = references::analyze(&mut client, &repository, &documents)?;
        for document in &documents {
            if fs::read_to_string(&document.path).map_err(failure)? != document.source {
                return Err(failure(format!(
                    "分析期间源文件已改变：{}",
                    document.path.display()
                )));
            }
        }
        client.shutdown()?;
        Ok(graph)
    }
}

/// 在 Rust 适配器内部统一添加可读错误，保持公共错误结构简单。
fn failure(message: impl Display) -> AnalysisError {
    AnalysisError {
        message: message.to_string(),
    }
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
