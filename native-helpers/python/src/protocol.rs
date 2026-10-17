//! Python 的严格单次请求；平台仅证明 helper 内嵌标准声明，不能导入解释器环境。
use anyhow::{Result, ensure};
pub use narrant_native_common::PROTOCOL;
use serde::Deserialize;
use std::{collections::BTreeSet, path::PathBuf};
pub const SOURCE_REVISION: &str = "11aaa39f84c0c44bdfb4c9cc21e4038c6f74c00a";

/// 唯一分析请求，拒绝未知或重复字段及任意命令/环境对象。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol: String,
    #[serde(rename = "type")]
    pub message_type: String,
    pub id: String,
    pub input: Input,
    pub environment: Environment,
    pub limits: Limits,
}
/// 当前固定侧选集，具体目标和源码根仍遵循仓库原有 Python 配置政策。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub repository_root: PathBuf,
    pub files: Vec<String>,
    pub include_tests: bool,
    pub profile: String,
}
/// Host 明确批准的 scratch 与 helper 内嵌标准声明平台。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    pub scratch_root: PathBuf,
    pub platforms: Vec<Platform>,
}
/// 平台不含源码搜索路径或解释器，root 只用于核验 helper 安装来源。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Platform {
    pub id: String,
    pub kind: String,
    pub version: String,
    pub root: PathBuf,
    pub metadata: Metadata,
}
/// 只接受与编译期 Ruff/ty 相同的 revision。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metadata {
    pub source_revision: String,
}
/// Host 负责期限和诊断限额，helper 在输出前执行结果限额。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub analysis_timeout_ms: u64,
    pub max_result_bytes: usize,
    pub max_diagnostic_bytes: usize,
}
impl Request {
    /// 独立启动也拒绝不规范选集、外部 scratch、错误标准声明与未知政策。
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.protocol == PROTOCOL && self.message_type == "analyze" && !self.id.is_empty(),
            "无效分析请求"
        );
        ensure!(
            self.input.profile == "python-static-v1",
            "未知 Python profile"
        );
        ensure!(
            self.input.repository_root.is_absolute() && self.environment.scratch_root.is_absolute(),
            "根路径必须为绝对路径"
        );
        let repository = self.input.repository_root.canonicalize()?;
        let scratch = self.environment.scratch_root.canonicalize()?;
        ensure!(
            repository.is_dir() && scratch.is_dir() && !scratch.starts_with(&repository),
            "scratch 必须为仓库外目录"
        );
        ensure!(
            self.limits.analysis_timeout_ms > 0
                && self.limits.max_result_bytes > 0
                && self.limits.max_diagnostic_bytes > 0,
            "无效协议限额"
        );
        ensure!(
            self.environment.platforms.len() == 1,
            "Python 必须提供唯一内嵌标准声明平台"
        );
        let platform = &self.environment.platforms[0];
        ensure!(
            !platform.id.is_empty()
                && platform.kind == "python-embedded-stdlib"
                && platform.version == "0.0.84"
                && platform.metadata.source_revision == SOURCE_REVISION
                && platform.root.is_absolute(),
            "无效 Python 标准声明平台"
        );
        let root = platform.root.canonicalize()?;
        ensure!(
            root.is_dir() && std::env::current_exe()?.canonicalize()?.starts_with(root),
            "标准声明平台不属于当前 helper 安装根"
        );
        let mut files = BTreeSet::new();
        for path in &self.input.files {
            ensure!(
                !path.contains(['\\', ':', '\0'])
                    && !path
                        .split('/')
                        .any(|part| part.is_empty() || part == "." || part == "..")
                    && files.insert(path),
                "选集含无效或重复相对路径"
            );
            ensure!(
                matches!(
                    std::path::Path::new(path)
                        .extension()
                        .and_then(|extension| extension.to_str()),
                    Some("py" | "pyi")
                ),
                "Python 分析器仅接受 .py/.pyi"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/protocol.rs"]
mod tests;
