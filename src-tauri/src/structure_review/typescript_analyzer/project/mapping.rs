//! 路径和 URI 映射拒绝词法跳转与符号链接逃逸，所有文件保持真实身份。
use super::PreparedProject;
use crate::structure_review::source_text::SourceText;
use crate::structure_review::typescript_analyzer::{failure, Result, LINE_BREAKS};
use std::path::{Component, Path, PathBuf};

/// 先检查词法边界，不允许通过父目录片段掩盖来源。
pub(super) fn lexical_under(root: &Path, file: &Path) -> Result<()> {
    if !file.is_absolute()
        || !file.starts_with(root)
        || file
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(failure(format!("TypeScript 路径越界：{}", file.display())));
    }
    Ok(())
}

/// 已存在文件还必须通过真实路径检查，拒绝指向仓库外的链接。
pub(super) fn checked_under(root: &Path, file: &Path) -> Result<()> {
    lexical_under(root, file)?;
    if !file.canonicalize().map_err(failure)?.starts_with(root) {
        return Err(failure(format!(
            "TypeScript 符号链接越界：{}",
            file.display()
        )));
    }
    Ok(())
}
impl PreparedProject {
    /// 只允许已快照的第一方源码或固定标准声明作为语义位置来源。
    pub fn checked_path(&self, file: &Path) -> Result<PathBuf> {
        if !file.is_absolute()
            || file
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(failure("服务返回非规范路径"));
        }
        let canonical = file.canonicalize().map_err(|error| {
            failure(format!(
                "无法读取 TypeScript 定义文件 {}：{error}",
                file.display()
            ))
        })?;
        if self.sources.contains_key(&canonical) || self.metadata.contains(&canonical) {
            return Ok(canonical);
        }
        if self
            .allowed_roots
            .iter()
            .skip(1)
            .any(|root| canonical.starts_with(root))
            && canonical.is_file()
            && canonical.to_string_lossy().ends_with(".d.ts")
        {
            return Ok(canonical);
        }
        Err(failure(format!("服务引入未授权源码：{}", file.display())))
    }

    /// 服务 URI 只能是本地 file URI，映射后仍执行同一来源范围检查。
    pub fn path_from_uri(&self, uri: &str) -> Result<PathBuf> {
        let parsed = url::Url::parse(uri).map_err(failure)?;
        if parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.host_str().is_some_and(|host| host != "localhost")
        {
            return Err(failure("服务 URI 含非本地来源或额外片段"));
        }
        self.checked_path(
            &parsed
                .to_file_path()
                .map_err(|_| failure("服务 URI 不是本地文件"))?,
        )
    }

    /// 语义位置直接使用 Git workspace 或固定标准库的真实 URI。
    pub fn source_uri(&self, file: &Path) -> Result<String> {
        crate::lsp::file_uri(&self.checked_path(file)?).map_err(failure)
    }

    /// 第一方快照和按需加载的运行时文本共享索引；每次访问仍校验真实路径。
    pub fn source(&self, file: &Path) -> Result<SourceText> {
        let checked = self.checked_path(file)?;
        if let Some(snapshot) = self.sources.get(&checked) {
            return Ok(snapshot.source.clone());
        }
        let mut loaded = self.loaded_sources.borrow_mut();
        if let Some(source) = loaded.get(&checked) {
            return Ok(source.clone());
        }
        let source = SourceText::new(
            std::fs::read_to_string(&checked).map_err(failure)?,
            LINE_BREAKS,
        );
        loaded.insert(checked, source.clone());
        Ok(source)
    }
}
