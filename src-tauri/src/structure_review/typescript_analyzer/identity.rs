//! 节点身份只依赖仓库相对路径、词法归属与声明名。
use super::{failure, Result};
use std::path::{Component, Path};

/// 按词法相对路径编码身份；真实文件来源由项目访问层校验，内存语法解析无需磁盘访问。
pub(super) fn file_scope(repository: &Path, path: &Path, language: &str) -> Result<String> {
    if !repository.is_absolute() || !path.is_absolute() {
        return Err(failure("TypeScript 分析路径必须为绝对路径"));
    }
    let relative = path.strip_prefix(repository).map_err(failure)?;
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(failure("TypeScript 源文件路径不在仓库内"));
    }
    let relative = relative
        .to_str()
        .ok_or_else(|| failure("TypeScript 文件路径不是 UTF-8"))?;
    Ok(format!("{language}:repo:{}", component(relative)))
}

/// 转义每个身份分段，避免文件名和语言符号与层级分隔符冲突。
pub(super) fn component(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}
