//! 固定外部工具链；检查只在非空准备时发生，运行分析绝不下载工具。
use super::{failure, runtime, Result};
use crate::lsp::CancellationCheck;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

/// 明确指定 Node 与完整运行时目录，不借用被分析项目的 TypeScript。
#[derive(Clone, Debug)]
pub(crate) struct TypeScriptToolchain {
    pub node: PathBuf,
    pub runtime_root: PathBuf,
}
impl Default for TypeScriptToolchain {
    /// 开发默认路径以应用仓库为基准，与被分析的临时侧无关。
    fn default() -> Self {
        let repository = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .expect("crate 位于仓库内");
        Self {
            node: std::env::var_os("NARRANT_TYPESCRIPT_NODE")
                .map(PathBuf::from)
                .unwrap_or_else(|| {
                    repository.join(if cfg!(windows) {
                        ".tools/node.exe"
                    } else {
                        ".tools/node"
                    })
                }),
            runtime_root: std::env::var_os("NARRANT_TYPESCRIPT_RUNTIME")
                .map(PathBuf::from)
                .unwrap_or_else(|| repository.join(".tools/typescript-analyzer")),
        }
    }
}
impl TypeScriptToolchain {
    /// 校验 Node 最低版本、包固定版本和声明依赖闭包，返回规范化路径。
    pub fn validate(&self, timeout: Duration, cancelled: &CancellationCheck) -> Result<Self> {
        if !self.node.is_absolute() || !self.runtime_root.is_absolute() {
            return Err(failure("TypeScript 工具路径必须是绝对路径"));
        }
        let node = self.node.canonicalize().map_err(|error| {
            failure(format!(
                "缺少 Node 工具：{error}；请运行 pnpm setup:typescript-analyzer"
            ))
        })?;
        let runtime_root = self.runtime_root.canonicalize().map_err(|error| {
            failure(format!(
                "缺少 TypeScript 运行时：{error}；请运行 pnpm setup:typescript-analyzer"
            ))
        })?;
        let toolchain = Self { node, runtime_root };
        for (name, version, required) in [
            ("typescript", "6.0.3", "lib/tsserver.js"),
            ("typescript-language-server", "6.0.0", "lib/cli.mjs"),
            ("@types/node", "22.20.1", "index.d.ts"),
            ("undici-types", "6.21.0", "index.d.ts"),
        ] {
            let package = toolchain.runtime_root.join(name);
            validate_tree(
                &package,
                &toolchain.runtime_root,
                &mut std::collections::BTreeSet::new(),
            )?;
            let metadata: Value = serde_json::from_str(
                &std::fs::read_to_string(package.join("package.json")).map_err(failure)?,
            )
            .map_err(failure)?;
            if metadata["name"] != name || metadata["version"] != version {
                return Err(failure(format!(
                    "TypeScript 运行时必须使用 {name}@{version}"
                )));
            }
            let required = package.join(required).canonicalize().map_err(failure)?;
            if !required.starts_with(&toolchain.runtime_root) {
                return Err(failure("TypeScript 运行时路径越界"));
            }
            if name == "@types/node"
                && metadata["dependencies"] != serde_json::json!({"undici-types":"~6.21.0"})
            {
                return Err(failure("固定 Node 声明的 undici 依赖闭包不完整"));
            }
            if let Some(dependencies) = metadata.get("dependencies") {
                let dependencies = dependencies
                    .as_object()
                    .ok_or_else(|| failure("工具 dependencies 不是对象"))?;
                if dependencies.iter().any(|(dependency, range)| {
                    name != "@types/node" || dependency != "undici-types" || range != "~6.21.0"
                }) {
                    return Err(failure(format!("{name} 存在不允许的运行时依赖")));
                }
            }
        }
        let mut command = toolchain.command();
        command.arg("--version");
        let version = runtime::run(command, timeout, cancelled)?;
        if !supported_node(version.trim()) {
            return Err(failure(format!(
                "TypeScript 分析需要 Node >=22.22.2，当前 {}",
                version.trim()
            )));
        }
        Ok(toolchain)
    }

    /// 清除 Node 注入变量，固定解释器；不改变应用自己的依赖或配置。
    pub(super) fn command(&self) -> Command {
        let mut command = Command::new(&self.node);
        command.env_remove("NODE_OPTIONS").env_remove("NODE_PATH");
        command
    }
}

/// 固定包树中的任意符号链接均不可逃出运行时，递归集合避免循环链接。
fn validate_tree(
    path: &Path,
    root: &Path,
    visited: &mut std::collections::BTreeSet<PathBuf>,
) -> Result<()> {
    let canonical = path.canonicalize().map_err(|error| {
        failure(format!(
            "缺少固定 TypeScript 工具 {}：{error}；请运行 pnpm setup:typescript-analyzer",
            path.display()
        ))
    })?;
    if !canonical.starts_with(root) {
        return Err(failure("TypeScript 运行时包树路径越界"));
    }
    if !visited.insert(canonical.clone()) {
        return Ok(());
    }
    if canonical.is_dir() {
        for child in std::fs::read_dir(canonical).map_err(failure)? {
            validate_tree(&child.map_err(failure)?.path(), root, visited)?;
        }
    } else if !canonical.is_file() {
        return Err(failure("TypeScript 运行时含非普通文件"));
    }
    Ok(())
}

/// 比较纯数字 Node 版本，拒绝预发布和无效版本字符串。
fn supported_node(version: &str) -> bool {
    let parts: Option<Vec<u32>> = version
        .strip_prefix('v')
        .unwrap_or(version)
        .split('.')
        .map(|value| value.parse().ok())
        .collect();
    parts.is_some_and(|parts| parts.len() == 3 && (parts[0], parts[1], parts[2]) >= (22, 22, 2))
}

#[cfg(test)]
#[path = "tests/tools.rs"]
mod tests;
