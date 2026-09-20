//! 项目文件访问：统一路径校验与源码的按需读取和缓存。
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
    /// 只允许清单中的第一方文件或固定标准声明，尚未读取的源码也保留其归属。
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
        if self.source_paths.contains(&canonical) || self.metadata.contains(&canonical) {
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

    /// 首次访问校验来源并读取原文；固定 workspace 会话内复用缓存，不反复访问文件系统。
    pub fn source(&self, file: &Path) -> Result<SourceText> {
        if let Some(source) = self.loaded_sources.borrow().get(file) {
            return Ok(source.clone());
        }
        let checked = self.checked_path(file)?;
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
