//! 一次一侧的严格 NDJSON 请求；runtime 政策只来自固定 profile 和批准的平台。
use anyhow::{Result, ensure};
use serde::Deserialize;
use std::{collections::BTreeSet, path::PathBuf};

pub const PROTOCOL: &str = "narrant.native-analyzer/1";
/// Host 发来的唯一分析请求，不接受命令或任意环境对象。
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
/// 当前固定侧的选集和语言政策。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub repository_root: PathBuf,
    pub files: Vec<String>,
    pub include_tests: bool,
    pub profile: String,
}
/// Host 授权的 scratch 和声明平台。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Environment {
    pub scratch_root: PathBuf,
    pub platforms: Vec<Platform>,
}
/// 显式 Rust 标准库路径；不推导不受信任命令。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Platform {
    pub id: String,
    pub kind: String,
    pub version: String,
    pub root: PathBuf,
    pub metadata: RustMetadata,
}
/// Rust 平台唯一允许的配置字段是 sysroot 内的源码相对路径。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustMetadata {
    pub rust_src: String,
}
/// Host 负责整个进程树的期限与诊断上限，helper 在发送前检查结果上限。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub analysis_timeout_ms: u64,
    pub max_result_bytes: usize,
    pub max_diagnostic_bytes: usize,
}
impl Request {
    /// 独立启动 helper 也必须拒绝不规范路径、重复选集、未知 profile 与多平台歧义。
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.protocol == PROTOCOL && self.message_type == "analyze" && !self.id.is_empty(),
            "无效分析请求"
        );
        ensure!(
            self.input.repository_root.is_absolute() && self.environment.scratch_root.is_absolute(),
            "根路径必须为绝对路径"
        );
        ensure!(
            matches!(
                self.input.profile.as_str(),
                "rust-repository-v1" | "rust-standard-library-v1" | "rust-all-dependencies-v1"
            ),
            "未知 Rust profile"
        );
        ensure!(
            self.environment.platforms.len()
                == usize::from(self.input.profile != "rust-repository-v1"),
            "Rust profile 的平台数量不匹配"
        );
        ensure!(
            self.limits.analysis_timeout_ms > 0
                && self.limits.max_result_bytes > 0
                && self.limits.max_diagnostic_bytes > 0,
            "无效协议限额"
        );
        for platform in &self.environment.platforms {
            ensure!(
                !platform.id.is_empty()
                    && !platform.version.is_empty()
                    && platform.kind == "rust-standard-library"
                    && platform.root.is_absolute(),
                "无效 Rust 平台"
            );
            let source = &platform.metadata.rust_src;
            ensure!(
                !source.contains(['\\', ':', '\0'])
                    && !source
                        .split('/')
                        .any(|part| part.is_empty() || part == "." || part == ".."),
                "rust_src 必须为平台内相对路径"
            );
        }
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
        }
        Ok(())
    }
}

/// 运行中仅接受严格的同请求 cancel；未知字段、重复字段和错误 ID 均为协议错误。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cancel {
    protocol: String,
    #[serde(rename = "type")]
    message_type: String,
    id: String,
}

/// 控制读取独立于原生加载，不把错误 ID 或任意 JSON 当成有效取消。
pub fn validate_cancel(line: &str, id: &str) -> Result<()> {
    let cancel: Cancel = serde_json::from_str(line)?;
    ensure!(
        cancel.protocol == PROTOCOL && cancel.message_type == "cancel" && cancel.id == id,
        "无效取消请求"
    );
    Ok(())
}

#[cfg(test)]
#[path = "tests/protocol.rs"]
mod tests;
