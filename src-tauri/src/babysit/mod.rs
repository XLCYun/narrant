use crate::operation::CancellationSignal;
use crate::review::FileChangeContent;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
#[cfg(test)]
use std::sync::atomic::{AtomicU8, AtomicUsize};
use std::sync::{Arc, Mutex};

mod application;
mod claude;
mod codex;
mod eligibility;
mod error;
mod model_catalog;
mod repository_tools;
mod validation;

use application::codex_setup_error;
#[cfg(test)]
use application::CacheKey;
pub use application::{
    BabysitApplication, BabysitEligibility, BabysitEligibilityReason, BabysitFileRequest,
    BabysitGenerationPolicy, ReviewIdentity,
};
use eligibility::{eligibility, ensure_eligible};
#[allow(unused_imports)]
pub use error::{BabysitError, BabysitErrorCode, BabysitErrorPayload, BabysitRecoveryAction};
pub(crate) use model_catalog::{
    BabysitModelCatalog, BabysitProviderModelCatalog, BabysitResolvedModel,
};
use validation::{merge_repair_patch, normalize_and_collect_repairs};
pub use validation::{
    parse_provider_response, validate_response, BabysitFileExplanations, ValidationFailure,
};

pub use claude::ClaudeCodeProvider;
pub use codex::CodexAppServerProvider;
#[allow(unused_imports)]
pub use repository_tools::{
    GlobRequest, GlobResponse, GrepMatch, GrepRequest, GrepResponse, ReadContinuation, ReadInput,
    ReadRequest, ReadResponse, RepositoryToolDefinition, RepositoryToolError,
    RepositoryToolRegistration, RepositoryToolRegistrationView, RepositoryToolRequest,
    RepositoryToolResponse, RepositoryTools,
};
// Provider-neutral model catalog types live in their own deep module.
/// 当前首个 Babysit 契约版本；它参与缓存 identity，避免契约变更复用旧结果。
pub const ANALYSIS_CONTRACT_VERSION: &str = "babysit-line-explanations-v2";
/// 同一个 provider 输出失败后允许的最大修复次数。
pub const MAX_REPAIR_ATTEMPTS: u8 = 3;

/// 把源码渲染为供 provider 阅读的带行号视图；编号仅属于 prompt，不改变 Review Snapshot。
pub(crate) fn numbered_content(content: &str) -> String {
    content
        .split('\n')
        .enumerate()
        .map(|(index, line)| format!("{:04}|{line}", index + 1))
        .collect::<Vec<_>>()
        .join("\n")
}
/// 为 provider 私有运行状态分配进程内唯一身份，避免相同 File Change 的并发分析碰撞。
static NEXT_ANALYSIS_RUN_ID: AtomicU64 = AtomicU64::new(1);
/// Codex App Server 在 provider-independent 错误契约中的稳定 identity。
const CODEX_PROVIDER_ID: &str = "codex";

/// Installed provider 初始化失败的 provider-independent 分类。
///
/// 分类只描述恢复方向，不包含 provider 协议、版本输出或 executable 路径；
/// provider identity 单独通过 `provider_id` 传递，因而新增 provider 不需要新增
/// 一组 Codex-shaped enum variant。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
// expand-first seam：其余分类由后续 installed provider adapter 构造。
#[allow(dead_code)]
pub enum BabysitProviderSetupFailure {
    NotFound,
    ExecutableInvalid,
    SignedOut,
    Incompatible,
    InitializationFailed,
}

impl fmt::Display for BabysitProviderSetupFailure {
    /// 将通用 setup 分类投影为不含路径和原始进程输出的稳定短文本。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::NotFound => "未找到可用的 provider 可执行文件",
            Self::ExecutableInvalid => "provider 可执行文件不可用",
            Self::SignedOut => "provider 尚未登录",
            Self::Incompatible => "provider 缺少宝宝模式所需能力",
            Self::InitializationFailed => "provider 初始化失败",
        })
    }
}

/// Installed provider runtime 的 provider-independent readiness 状态。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
// expand-first seam：后续 provider-aware command 会把 readiness 投影到 IPC。
#[allow(dead_code)]
pub enum BabysitProviderRuntimeStatus {
    #[default]
    Unknown,
    Probing,
    Ready,
    Missing,
    SignedOut,
    Incompatible,
    InitFailed,
}

/// Installed Coding Agent provider runtime 的唯一 application lifecycle seam。
#[async_trait]
pub trait InstalledProviderRuntime: Send + Sync {
    /// 返回当前 runtime readiness，但不暴露 executable 路径或原始进程信息。
    #[allow(dead_code)]
    fn status(&self) -> BabysitProviderRuntimeStatus;
    /// 懒执行 discovery、认证检查、能力探针和 provider 进程初始化。
    async fn ensure_ready(&self, workspace_path: &Path) -> Result<(), BabysitError>;
    /// 应用退出时释放 provider 进程、线程和未完成的 runtime ownership。
    async fn shutdown(&self);
}

/// Registry 中一个 provider 拥有的完整能力集合。
#[async_trait]
pub trait BabysitProviderCapabilities: Send + Sync {
    /// 懒准备本次分析，并冻结 provider、模型和进程 generation。
    async fn prepare_analysis(
        &self,
        workspace_path: &Path,
        preference: Option<&crate::application_settings::BabysitProviderSettings>,
    ) -> Result<PreparedBabysitAnalysis, BabysitError>;
    /// 解析该 provider 当前可见的模型目录。
    async fn model_catalog(&self) -> Result<BabysitProviderModelCatalog, BabysitError>;
    /// 只验证 executable，不持久化设置。
    async fn validate_executable(&self, path: &Path) -> Result<std::path::PathBuf, BabysitError>;
    /// 关闭该 bundle 拥有的运行资源。
    async fn shutdown(&self);
}

/// 一次分析开始时冻结的 provider 能力快照。
pub struct PreparedBabysitAnalysis {
    provider: Arc<dyn BabysitAgentProvider>,
    resolved: BabysitResolvedModel,
    process_generation: String,
    _lease: Option<tokio::sync::OwnedMutexGuard<()>>,
}

/// Codex 的 capability adapter；Codex runtime 细节不会进入 application orchestration。
struct CodexCapabilityBundle {
    provider: Arc<CodexAppServerProvider>,
    runtime: Arc<codex::runtime::CodexRuntime>,
}

#[async_trait]
impl BabysitProviderCapabilities for CodexCapabilityBundle {
    /// 使用同一次 settings 快照解析模型并保留 Codex lifecycle lease。
    async fn prepare_analysis(
        &self,
        workspace_path: &Path,
        preference: Option<&crate::application_settings::BabysitProviderSettings>,
    ) -> Result<PreparedBabysitAnalysis, BabysitError> {
        let preference = preference.ok_or(BabysitError::ModelPreferenceInvalid)?;
        let (resolved, lease) = self
            .runtime
            .prepare_analysis_with_preference(workspace_path, preference)
            .await?;
        Ok(PreparedBabysitAnalysis {
            provider: self.provider.clone(),
            resolved: resolved.into(),
            process_generation: self.provider.process_generation(),
            _lease: Some(lease),
        })
    }

    /// 读取 Codex App Server 的动态模型目录。
    async fn model_catalog(&self) -> Result<BabysitProviderModelCatalog, BabysitError> {
        self.runtime.model_catalog().await.map(Into::into)
    }

    /// 校验 Codex executable 而不保存设置。
    async fn validate_executable(&self, path: &Path) -> Result<std::path::PathBuf, BabysitError> {
        self.runtime.validate_executable(path).await
    }

    /// 关闭 Codex runtime 及其 App Server。
    async fn shutdown(&self) {
        self.runtime.shutdown().await;
    }
}

/// Claude Code 的 capability adapter；离线目录与偏好解析由 #117 深化。
struct ClaudeCapabilityBundle {
    provider: Arc<ClaudeCodeProvider>,
    runtime: Arc<claude::runtime::ClaudeRuntime>,
}

impl ClaudeCapabilityBundle {
    /// 使用离线能力表解析一次不可变设置快照，不触发 runtime 或模型服务 I/O。
    fn resolve_preference(
        preference: &crate::application_settings::BabysitProviderSettings,
    ) -> Result<BabysitResolvedModel, BabysitError> {
        claude::ClaudeModelCatalog::offline()
            .resolve(preference)
            .map(Into::into)
            .map_err(|_| BabysitError::ModelPreferenceInvalid)
    }
}

#[async_trait]
impl BabysitProviderCapabilities for ClaudeCapabilityBundle {
    /// 懒准备 Claude runtime 并冻结当前 provider 解析结果。
    async fn prepare_analysis(
        &self,
        workspace_path: &Path,
        preference: Option<&crate::application_settings::BabysitProviderSettings>,
    ) -> Result<PreparedBabysitAnalysis, BabysitError> {
        let preference = preference.ok_or(BabysitError::ModelPreferenceInvalid)?;
        let resolved = Self::resolve_preference(preference)?;
        self.runtime.ensure_ready(workspace_path).await?;
        Ok(PreparedBabysitAnalysis {
            provider: self.provider.clone(),
            resolved,
            process_generation: self.provider.process_generation(),
            _lease: None,
        })
    }

    /// 返回随应用发布且不启动 CLI 的 Claude alias/custom 离线目录。
    async fn model_catalog(&self) -> Result<BabysitProviderModelCatalog, BabysitError> {
        Ok(claude::ClaudeModelCatalog::offline().into())
    }

    /// 校验 Claude executable 而不保存设置。
    async fn validate_executable(&self, path: &Path) -> Result<std::path::PathBuf, BabysitError> {
        claude::runtime::ClaudeRuntime::validate_executable(path).await
    }

    /// Claude 每轮使用新进程；仍通过 runtime 统一执行关闭生命周期。
    async fn shutdown(&self) {
        self.runtime.shutdown().await;
    }
}

/// Babysit 运行阶段的 provider-independent IPC 事件。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase", tag = "phase")]
pub enum BabysitProgress {
    Generating,
    Repairing { attempt: u8 },
    Completed,
}

/// Line Explanation 所属的 before/after 文件侧。
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FileSide {
    Before,
    After,
}

/// Narrant 为 provider 维护的完整非空行目标 inventory。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LineTarget {
    pub side: FileSide,
    pub line_number: usize,
    pub source: String,
}

/// Provider 一次请求需要的完整 File Change 分析输入。
#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BabysitProviderRequest {
    /// 仅用于 provider 内部关联 generation/repair；不会进入 prompt 或 IPC payload。
    #[serde(skip)]
    pub analysis_run_id: u64,
    pub path: String,
    pub locale: String,
    /// 当前 Review Workspace 的绝对路径；provider 只能把它作为只读 cwd 使用。
    pub workspace_path: String,
    pub before: String,
    pub after: String,
    pub targets: Vec<LineTarget>,
    /// 分析启动前已由动态目录解析并固定的模型标识。
    pub model: String,
    /// 分析启动前已由动态目录解析并固定的思考强度。
    pub reasoning_effort: String,
}

impl BabysitProviderRequest {
    /// 从完整分析请求投影出一轮 Line Explanation Repair Patch 的目标 inventory。
    fn repair_request(&self, failures: &[ValidationFailure]) -> Self {
        let requested = failures
            .iter()
            .filter_map(|failure| match failure {
                ValidationFailure::MissingLine { side, line_number }
                | ValidationFailure::EmptyExplanation { side, line_number } => {
                    Some((*side, *line_number))
                }
                _ => None,
            })
            .collect::<HashSet<_>>();
        let mut repair = self.clone();
        repair
            .targets
            .retain(|target| requested.contains(&(target.side, target.line_number)));
        repair
    }
}

// Provider response parsing and validation live in the validation module.
// Stable domain and IPC errors live in the error module.
/// provider-independent 的 Installed/Built-in Agent 接口。
#[async_trait]
pub trait BabysitAgentProvider: Send + Sync {
    /// 返回本次 provider 进程 generation，进程重启后不得复用旧结果。
    fn process_generation(&self) -> String;
    /// 生成完整 before/after 解释；provider 不得拆成逐行请求。
    async fn generate(
        &self,
        request: BabysitProviderRequest,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError>;
    /// 只生成上一次结果中缺失或空白的目标行；provider 可复用会话或启动无状态轮次。
    async fn repair(
        &self,
        request: BabysitProviderRequest,
        previous: BabysitFileExplanations,
        failures: Vec<ValidationFailure>,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError>;
    /// 上层在 repair 之间提前退出时，释放 provider 私有的 thread/run 状态。
    async fn abandon(&self, _request: &BabysitProviderRequest) {}
    /// 应用退出时释放 provider 私有的线程与 App Server；fake provider 无需额外动作。
    async fn shutdown(&self) {}
}

// Application orchestration lives in the dedicated deep module.
/// 检查可取消操作是否已被前端取消。
fn ensure_not_cancelled(cancellation: &CancellationSignal) -> Result<(), BabysitError> {
    cancellation
        .is_cancelled()
        .then_some(BabysitError::Cancelled)
        .map_or(Ok(()), Err)
}

/// 将 Review 读取结果转换为 provider 需要的单次完整请求。
fn build_provider_request(
    request: &BabysitFileRequest,
    content: &FileChangeContent,
) -> Result<BabysitProviderRequest, BabysitError> {
    let before = content
        .before
        .as_ref()
        .and_then(|value| value.text())
        .unwrap_or_default();
    let after = content
        .after
        .as_ref()
        .and_then(|value| value.text())
        .unwrap_or_default();
    let mut targets = non_empty_targets(FileSide::Before, before);
    targets.extend(non_empty_targets(FileSide::After, after));
    validate_target_inventory(&targets)?;
    Ok(BabysitProviderRequest {
        analysis_run_id: NEXT_ANALYSIS_RUN_ID.fetch_add(1, Ordering::Relaxed),
        path: request.path.clone(),
        locale: request.locale.clone(),
        workspace_path: request.workspace_path.clone(),
        before: before.to_owned(),
        after: after.to_owned(),
        targets,
        model: String::new(),
        reasoning_effort: String::new(),
    })
}

/// 把一侧文本拆成一基、只含非空代码行的目标。
fn non_empty_targets(side: FileSide, text: &str) -> Vec<LineTarget> {
    text.lines()
        .enumerate()
        .filter_map(|(index, source)| {
            (!source.trim().is_empty()).then_some(LineTarget {
                side,
                line_number: index + 1,
                source: source.to_owned(),
            })
        })
        .collect()
}

/// 验证 Narrant 自己生成的 target identity 不重复且行号合法。
fn validate_target_inventory(targets: &[LineTarget]) -> Result<(), BabysitError> {
    let mut seen = HashSet::new();
    for target in targets {
        if target.line_number == 0 || !seen.insert((target.side, target.line_number)) {
            return Err(BabysitError::InvalidProviderOutput {
                detail: ValidationFailure::DuplicateTarget {
                    side: target.side,
                    line_number: target.line_number,
                }
                .to_string(),
            });
        }
    }
    Ok(())
}

// Repair normalization and patch merging live in the validation module.
// Test provider implementations live beside the tests that exercise them.
#[cfg(test)]
#[path = "tests/support/fake_provider.rs"]
mod fake_provider;
#[cfg(test)]
pub(crate) use fake_provider::FakeBabysitProvider;
#[cfg(test)]
#[path = "tests/support/application_builder.rs"]
mod application_builder;
#[cfg(test)]
pub(crate) use application_builder::{TestBabysitAgentProvider, TestBabysitApplication};

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
