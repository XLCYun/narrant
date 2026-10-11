//! Rust 工厂按当前侧清单批准已有 sysroot；helper 构建工具链不进入运行环境。
mod toolchain;
use super::{failure, AnalysisError, NativeLanguageAnalyzer, Platform};
use crate::structure_review::{
    code_graph::{AnalysisInput, CodeGraph, LanguageAnalyzer, RustDependencyScope},
    native_analyzer::{rust_environment, CancellationCheck, NativeProfile},
};
use std::{collections::BTreeMap, ffi::OsString, path::Path};
type Result<T> = std::result::Result<T, AnalysisError>;

/// 冻结支持包入口及启动器环境，两侧只根据各自 manifest 探测项目工具链。
#[derive(Clone)]
pub(crate) struct RustPackAnalyzer {
    analyzer: NativeLanguageAnalyzer,
    environment: BTreeMap<OsString, OsString>,
}
impl RustPackAnalyzer {
    /// 控制文件和入口通过通用工厂验证，网络环境快照保留全部依赖档的用户政策。
    pub(crate) fn from_pack(root: &Path) -> Result<Self> {
        let mut environment = rust_environment(NativeProfile::RustAllDependencies, |key| {
            std::env::var_os(key)
        });
        if cfg!(windows) {
            if let Some(value) = std::env::var_os("SystemRoot") {
                environment.insert("SystemRoot".into(), value);
            }
        }
        Ok(Self {
            analyzer: super::from_pack(crate::language_support::PackId::Rust, root)?,
            environment,
        })
    }
    /// 空选集直接复用公共 Host；非空侧逐项目检查真实工具链和标准源码。
    pub(crate) fn analyze(
        &self,
        input: &AnalysisInput,
        cancelled: CancellationCheck,
        scope: RustDependencyScope,
    ) -> Result<CodeGraph> {
        self.analyzer.validate_input(input)?;
        if cancelled() {
            return Err(AnalysisError::cancelled("Rust 结构分析已取消"));
        }
        // 空语言组选集不探测工具链或启动 helper，不能把加载错误映射到此分支。
        if input.files.is_empty() {
            return CodeGraph::new(Default::default(), Vec::new()).map_err(failure);
        }
        let mut environment = self.environment.clone();
        let mut platforms = Vec::new();
        let toolchain = toolchain::RustToolchain::resolve(
            &input.repository_root,
            scope,
            &cancelled,
            &environment,
        )?;
        environment.insert("PATH".into(), toolchain.path);
        if let Some(root) = toolchain.sysroot {
            let sources = toolchain
                .sources
                .ok_or_else(|| failure("已解析 sysroot 缺少标准库源码"))?;
            let relative = sources
                .strip_prefix(&root)
                .map_err(failure)?
                .to_string_lossy()
                .replace('\\', "/");
            platforms.push(Platform {
                id: "project-rust".into(),
                kind: "rust-standard-library".into(),
                version: "project-toolchain".into(),
                root,
                metadata: serde_json::json!({"rust_src": relative}),
            });
        }
        self.analyzer
            .clone()
            .with_rust_project(scope, environment, platforms)
            .with_cancellation(cancelled)
            .analyze(input)
    }
}

#[cfg(test)]
#[path = "rust/tests/production.rs"]
mod tests;
