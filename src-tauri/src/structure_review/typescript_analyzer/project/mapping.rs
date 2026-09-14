//! 路径和 URI 映射拒绝词法跳转与符号链接逃逸，运行时声明保持外部身份。
use super::PreparedProject;
use crate::structure_review::typescript_analyzer::{failure, Result};
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
        let canonical = file.canonicalize().map_err(failure)?;
        if self.sources.contains_key(&canonical)
            || self.metadata.contains_key(&canonical)
            || self.runtime_original(&canonical).is_some()
        {
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

    /// 将原始侧已选中或上下文文件映射到受控视图。
    pub fn view_path(&self, original: &Path) -> Result<PathBuf> {
        lexical_under(&self.original_root, original)?;
        let view = self.view_root.join(
            original
                .strip_prefix(&self.original_root)
                .map_err(failure)?,
        );
        if !self.sources.contains_key(&view) {
            return Err(failure("原始路径不属于源码快照"));
        }
        Ok(view)
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

    /// 返回第一方的原始 URI；标准库保留固定运行时 URI，由语义层识别为外部对象。
    pub fn original_uri(&self, view: &Path) -> Result<String> {
        let checked = self.checked_path(view)?;
        let identity = self
            .sources
            .get(&checked)
            .map(|source| source.original.clone())
            .or_else(|| self.metadata.get(&checked).cloned())
            .or_else(|| self.runtime_original(&checked))
            .unwrap_or(checked);
        crate::lsp::file_uri(&identity).map_err(failure)
    }

    /// 视图中的固定 Node 闭包映射回工具身份；业务 node_modules 永远不能使用此入口。
    fn runtime_original(&self, file: &Path) -> Option<PathBuf> {
        if !file.to_string_lossy().ends_with(".d.ts") {
            return None;
        }
        for name in ["@types/node", "undici-types"] {
            let root = self.view_root.join("node_modules").join(name);
            if let Ok(relative) = file.strip_prefix(root) {
                let original = self.toolchain.runtime_root.join(name).join(relative);
                if original.is_file() {
                    return Some(original);
                }
            }
        }
        None
    }

    /// 从固定快照读取第一方文本；运行时声明只能从已验证目录读取。
    pub fn source(&self, view: &Path) -> Result<String> {
        let checked = self.checked_path(view)?;
        self.sources
            .get(&checked)
            .map(|source| Ok(source.source.clone()))
            .unwrap_or_else(|| std::fs::read_to_string(checked).map_err(failure))
    }
}
