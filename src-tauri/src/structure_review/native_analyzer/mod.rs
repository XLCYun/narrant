//! 原生语言 helper 的公共宿主：只解释受限协议，不参与各语言 AST 映射。

mod graph;
mod input;
mod process;
mod profiles;
mod protocol;

pub use profiles::NativeProfile;

use std::{collections::BTreeMap, ffi::OsString, path::PathBuf, sync::Arc, time::Duration};

use super::code_graph::{AnalysisError, AnalysisInput, CodeGraph, LanguageAnalyzer};

/// 只观察操作取消状态；所有进程和读写线程仍由当前同步调用回收。
pub type CancellationCheck = Arc<dyn Fn() -> bool + Send + Sync>;

/// 由支持包入口提供的固定命令，不从分析项目读取可执行文件或命令行。
#[derive(Clone, Debug)]
pub struct HelperCommand {
    pub executable: PathBuf,
    pub args: Vec<OsString>,
    /// 清空继承环境后，仅传入经支持包验证的 runtime 必要值。
    pub environment: BTreeMap<OsString, OsString>,
    /// 可执行文件所属的受信安装根；不得与当前仓库重叠。
    pub trusted_roots: Vec<PathBuf>,
}

/// 前后侧必须冻结同一组实现、投影和语言政策身份。
#[derive(Clone, Debug)]
pub struct HelperIdentity {
    pub helper_id: String,
    pub helper_version: String,
    pub projection_revision: String,
    pub languages: Vec<String>,
    pub profile: String,
}

/// 已验证的目标声明授权锚点；内嵌声明使用 helper 安装根，不代表磁盘源码目录。
#[derive(Clone, Debug, serde::Serialize)]
pub struct Platform {
    pub id: String,
    pub kind: String,
    pub version: String,
    pub root: PathBuf,
    pub metadata: serde_json::Value,
}

/// 固定阶段期限和内存上限；日志或分片不会延长任何期限。
#[derive(Clone, Debug)]
pub struct AnalysisLimits {
    pub startup_timeout: Duration,
    pub analysis_timeout: Duration,
    pub cancellation_grace: Duration,
    pub exit_timeout: Duration,
    pub max_result_bytes: usize,
    pub max_diagnostic_bytes: usize,
}

impl Default for AnalysisLimits {
    /// 启动与分析容纳慢机器和大项目；取消与退出仍使用短期限。
    fn default() -> Self {
        Self {
            startup_timeout: Duration::from_secs(60),
            analysis_timeout: Duration::from_secs(30 * 60),
            cancellation_grace: Duration::from_secs(2),
            exit_timeout: Duration::from_secs(5),
            max_result_bytes: 128 * 1024 * 1024,
            max_diagnostic_bytes: 256 * 1024,
        }
    }
}

/// 一次一侧的固定 helper 配置；每次分析建立独立 scratch 和进程所有权。
#[derive(Clone)]
pub struct NativeLanguageAnalyzer {
    command: HelperCommand,
    identity: HelperIdentity,
    platforms: Vec<Platform>,
    limits: AnalysisLimits,
    cancelled: CancellationCheck,
}

impl NativeLanguageAnalyzer {
    /// 接受受信命令与目标 catalog；Rust 只快照固定工具链/网络变量，不安装依赖。
    pub fn new(
        mut command: HelperCommand,
        identity: HelperIdentity,
        platforms: Vec<Platform>,
    ) -> Self {
        if let Some(
            profile @ (NativeProfile::RustRepository
            | NativeProfile::RustStandardLibrary
            | NativeProfile::RustAllDependencies),
        ) = NativeProfile::parse(&identity.profile)
        {
            command
                .environment
                .extend(profiles::rust_environment(profile, |key| {
                    std::env::var_os(key)
                }));
            command
                .environment
                .remove(std::ffi::OsStr::new("RUSTUP_TOOLCHAIN"));
        }
        Self {
            command,
            identity,
            platforms,
            limits: AnalysisLimits::default(),
            cancelled: Arc::new(|| false),
        }
    }

    /// 设置本次分析的有界资源政策，前后侧应使用同一组参数。
    pub fn with_limits(mut self, limits: AnalysisLimits) -> Self {
        self.limits = limits;
        self
    }

    /// 绑定工作区或用户取消观察；取消不会跳过进程回收。
    pub fn with_cancellation(mut self, cancelled: CancellationCheck) -> Self {
        self.cancelled = cancelled;
        self
    }
}

impl LanguageAnalyzer for NativeLanguageAnalyzer {
    /// 输入、协议、来源、图和退出均通过后才返回；工作区最终一致性由调用方租约检查。
    fn analyze(&self, input: &AnalysisInput) -> Result<CodeGraph> {
        let checked = input::validate(self, input)?;
        if (self.cancelled)() {
            return Err(cancelled());
        }
        let temporary_root = std::env::temp_dir().canonicalize().map_err(failure)?;
        if temporary_root.starts_with(&checked.repository) {
            return Err(failure("helper scratch 根不能位于分析仓库中"));
        }
        let scratch = tempfile::Builder::new()
            .prefix("narrant-native-")
            .tempdir_in(temporary_root)
            .map_err(failure)?;
        if scratch.path().starts_with(&checked.repository) {
            return Err(failure("helper scratch 不能位于分析仓库中"));
        }
        let request = checked.request(self, scratch.path())?;
        let graph = process::run(self, &checked, &request, scratch.path())?;
        if (self.cancelled)() {
            return Err(cancelled());
        }
        Ok(graph)
    }
}

/// 公共层只输出既有 AnalysisError，不把失败转换为空图。
type Result<T> = std::result::Result<T, AnalysisError>;

/// 保留底层诊断文本，统一添加本模块上下文。
fn failure(message: impl std::fmt::Display) -> AnalysisError {
    AnalysisError::new(format!("原生分析器：{message}"))
}

/// 明确记录取消原因，避免此后取消覆盖真实分析失败。
fn cancelled() -> AnalysisError {
    AnalysisError::cancelled("原生语言分析已取消")
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
