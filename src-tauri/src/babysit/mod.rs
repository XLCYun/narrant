use crate::operation::{CancellationSignal, OperationError};
use crate::review::{FileChangeContent, FileContentKind};
use async_trait::async_trait;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::path::Path;
#[cfg(test)]
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

mod codex;

pub use codex::CodexAppServerProvider;

/// 当前首个 Babysit 契约版本；它参与缓存 identity，避免契约变更复用旧结果。
pub const ANALYSIS_CONTRACT_VERSION: &str = "babysit-line-explanations-v1";
/// 对单个解释允许的最大 Unicode 字符数。
pub const MAX_EXPLANATION_CHARS: usize = 240;
/// 同一个 provider 输出失败后允许的最大修复次数。
pub const MAX_REPAIR_ATTEMPTS: u8 = 3;

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
    pub path: String,
    pub locale: String,
    /// 当前 Review Workspace 的绝对路径；provider 只能把它作为只读 cwd 使用。
    pub workspace_path: String,
    pub before: String,
    pub after: String,
    pub targets: Vec<LineTarget>,
}

/// Provider 与前端共用的紧凑 before/after 行解释结构。
///
/// Provider 协议使用字符串行号，因为 JSON 对象的 key 必须是字符串；内部校验
/// 仍然会把它们严格解析为一基行号，保证不会把错误 key 静默投影到 UI。
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BabysitFileExplanations {
    pub before: BTreeMap<String, String>,
    pub after: BTreeMap<String, String>,
}

/// 严格解析 provider JSON，拒绝未知顶层字段和重复行 key。
///
/// 生产 Codex adapter 会使用这个入口解析结构化输出；不能直接依赖 serde 默认的
/// Map 行为，因为默认行为会把重复 key 的前一个值静默覆盖掉。
pub fn parse_provider_response(json: &str) -> Result<BabysitFileExplanations, BabysitError> {
    serde_json::from_str(json).map_err(|error| BabysitError::InvalidProviderOutput {
        detail: format!("provider JSON 无效：{error}"),
    })
}

impl<'de> Deserialize<'de> for BabysitFileExplanations {
    /// 从 provider JSON 反序列化完整 before/after map，并交给严格 visitor 校验。
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(ExplanationsVisitor)
    }
}

/// `BabysitFileExplanations` 的严格顶层 map visitor。
struct ExplanationsVisitor;

impl<'de> Visitor<'de> for ExplanationsVisitor {
    type Value = BabysitFileExplanations;

    /// 声明顶层响应必须是包含 before 和 after 的解释对象。
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("包含 before 和 after 的解释对象")
    }

    /// 读取两个文件侧，同时拒绝未知字段、重复字段和缺失字段。
    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut before = None;
        let mut after = None;
        while let Some(key) = map.next_key::<String>()? {
            match key.as_str() {
                "before" if before.is_none() => before = Some(map.next_value::<UniqueLineMap>()?.0),
                "after" if after.is_none() => after = Some(map.next_value::<UniqueLineMap>()?.0),
                "before" | "after" => return Err(de::Error::custom(format!("重复的 {key} 字段"))),
                _ => return Err(de::Error::custom(format!("未知字段 {key}"))),
            }
        }
        let before = before.ok_or_else(|| de::Error::missing_field("before"))?;
        let after = after.ok_or_else(|| de::Error::missing_field("after"))?;
        Ok(BabysitFileExplanations { before, after })
    }
}

/// 保留 JSON map 中行号 key 的唯一性。
struct UniqueLineMap(BTreeMap<String, String>);

impl<'de> Deserialize<'de> for UniqueLineMap {
    /// 从 JSON 行号 map 反序列化，并保留重复 key 的检测能力。
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_map(UniqueLineMapVisitor)
    }
}

/// `UniqueLineMap` 的 map visitor；重复 key 会在反序列化阶段失败。
struct UniqueLineMapVisitor;

impl<'de> Visitor<'de> for UniqueLineMapVisitor {
    type Value = UniqueLineMap;

    /// 声明 map 的 value 必须是行号到解释文本的映射。
    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("行号到解释文本的对象")
    }

    /// 读取每个行号解释，拒绝重复行号 key。
    fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
    where
        M: MapAccess<'de>,
    {
        let mut values = BTreeMap::new();
        while let Some(key) = map.next_key::<String>()? {
            if values.contains_key(&key) {
                return Err(de::Error::custom(format!("重复的行号 key {key}")));
            }
            let value = map.next_value::<String>()?;
            values.insert(key, value);
        }
        Ok(UniqueLineMap(values))
    }
}

/// 后端把 provider 输出拒绝原因限制为不含源代码的稳定错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationFailure {
    DuplicateTarget { side: FileSide, line_number: usize },
    MissingLine { side: FileSide, line_number: usize },
    UnexpectedLine { side: FileSide, line_number: String },
    InvalidLineNumber { side: FileSide, line_number: String },
    EmptyExplanation { side: FileSide, line_number: usize },
    ExplanationTooLong { side: FileSide, line_number: usize },
    UnsafeExplanation { side: FileSide, line_number: usize },
}

impl fmt::Display for ValidationFailure {
    /// 将校验失败转换为不含源代码的 repair 诊断。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateTarget { side, line_number } => {
                write!(formatter, "{side:?} 第 {line_number} 行目标重复")
            }
            Self::MissingLine { side, line_number } => {
                write!(formatter, "{side:?} 缺少第 {line_number} 行解释")
            }
            Self::UnexpectedLine { side, line_number } => {
                write!(formatter, "{side:?} 出现未请求的第 {line_number} 行解释")
            }
            Self::InvalidLineNumber { side, line_number } => {
                write!(formatter, "{side:?} 行号 {line_number} 无效")
            }
            Self::EmptyExplanation { side, line_number } => {
                write!(formatter, "{side:?} 第 {line_number} 行解释为空")
            }
            Self::ExplanationTooLong { side, line_number } => {
                write!(formatter, "{side:?} 第 {line_number} 行解释过长")
            }
            Self::UnsafeExplanation { side, line_number } => {
                write!(formatter, "{side:?} 第 {line_number} 行解释包含控制字符")
            }
        }
    }
}

/// Babysit 对外暴露的稳定失败类别。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BabysitError {
    UnsupportedFile {
        reason: String,
    },
    InvalidProviderOutput {
        detail: String,
    },
    RepairExhausted {
        detail: String,
    },
    Cancelled,
    Provider {
        detail: String,
    },
    ProgressClosed,
    CacheUnavailable,
    /// 未找到可用的 Codex 可执行文件，需要安装或重新选择。
    CodexNotFound,
    /// 用户选择的 Codex 路径无法通过 `codex --version` 验证。
    CodexExecutableInvalid,
    /// Codex App Server 要求登录，但 Narrant 不会代替用户执行登录。
    CodexSignedOut,
    /// App Server 缺少 Babysit 所需的稳定能力。
    CodexIncompatible,
    /// App Server 初始化或握手失败。
    CodexInitializationFailed,
}

/// Babysit command 对前端暴露的稳定错误 code；detail 只承载当前失败的可读诊断。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BabysitErrorCode {
    UnsupportedFile,
    InvalidProviderOutput,
    RepairExhausted,
    Cancelled,
    Provider,
    ProgressClosed,
    CacheUnavailable,
    InputUnavailable,
    InvalidRequest,
    OperationConflict,
    OperationUnavailable,
    CodexNotFound,
    CodexExecutableInvalid,
    CodexSignedOut,
    CodexIncompatible,
    CodexInitializationFailed,
}

/// 前端可以直接执行的最小恢复动作；不携带可执行路径或进程原文。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BabysitRecoveryAction {
    LoginCodex,
    UpgradeCodex,
    SelectCodexExecutable,
    Retry,
}

/// Tauri command 失败时保持 code/detail 结构，避免把 BabysitError 压扁为字符串。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BabysitErrorPayload {
    pub code: BabysitErrorCode,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recovery: Option<BabysitRecoveryAction>,
}

impl BabysitErrorPayload {
    /// 将 immutable Review 读取失败映射为不依赖 provider 的输入错误。
    pub fn input_unavailable(_detail: impl Into<String>) -> Self {
        Self {
            code: BabysitErrorCode::InputUnavailable,
            // 下层 Review/Git 诊断可能含绝对路径；IPC detail 只保留稳定错误类别。
            detail: "Review File Change 内容不可用。".to_owned(),
            recovery: None,
        }
    }

    /// 将 cancellable operation registry 的错误映射到稳定 Babysit error code。
    pub fn from_operation(error: OperationError) -> Self {
        let code = match &error {
            OperationError::InvalidOperationId => BabysitErrorCode::InvalidRequest,
            OperationError::DuplicateOperation => BabysitErrorCode::OperationConflict,
            OperationError::Cancelled => BabysitErrorCode::Cancelled,
            OperationError::RegistryUnavailable => BabysitErrorCode::OperationUnavailable,
        };
        Self {
            code,
            detail: error.to_string(),
            recovery: None,
        }
    }
}

impl From<BabysitError> for BabysitErrorPayload {
    /// 将领域错误投影为 provider-independent 的 Tauri payload。
    fn from(error: BabysitError) -> Self {
        let code = match &error {
            BabysitError::UnsupportedFile { .. } => BabysitErrorCode::UnsupportedFile,
            BabysitError::InvalidProviderOutput { .. } => BabysitErrorCode::InvalidProviderOutput,
            BabysitError::RepairExhausted { .. } => BabysitErrorCode::RepairExhausted,
            BabysitError::Cancelled => BabysitErrorCode::Cancelled,
            BabysitError::Provider { .. } => BabysitErrorCode::Provider,
            BabysitError::ProgressClosed => BabysitErrorCode::ProgressClosed,
            BabysitError::CacheUnavailable => BabysitErrorCode::CacheUnavailable,
            BabysitError::CodexNotFound => BabysitErrorCode::CodexNotFound,
            BabysitError::CodexExecutableInvalid => BabysitErrorCode::CodexExecutableInvalid,
            BabysitError::CodexSignedOut => BabysitErrorCode::CodexSignedOut,
            BabysitError::CodexIncompatible => BabysitErrorCode::CodexIncompatible,
            BabysitError::CodexInitializationFailed => BabysitErrorCode::CodexInitializationFailed,
        };
        let recovery = match &error {
            BabysitError::CodexNotFound => Some(BabysitRecoveryAction::SelectCodexExecutable),
            BabysitError::CodexExecutableInvalid => {
                Some(BabysitRecoveryAction::SelectCodexExecutable)
            }
            BabysitError::CodexSignedOut => Some(BabysitRecoveryAction::LoginCodex),
            BabysitError::CodexIncompatible => Some(BabysitRecoveryAction::UpgradeCodex),
            BabysitError::CodexInitializationFailed => Some(BabysitRecoveryAction::Retry),
            BabysitError::RepairExhausted { .. }
            | BabysitError::Provider { .. }
            | BabysitError::InvalidProviderOutput { .. }
            | BabysitError::Cancelled
            | BabysitError::ProgressClosed
            | BabysitError::CacheUnavailable
            | BabysitError::UnsupportedFile { .. } => None,
        };
        Self {
            code,
            detail: error.to_string(),
            recovery,
        }
    }
}

impl fmt::Display for BabysitError {
    /// 将领域错误转换为可读诊断；稳定分支由 BabysitErrorCode 承担。
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedFile { reason } => write!(formatter, "此文件不支持宝宝模式：{reason}"),
            Self::InvalidProviderOutput { detail } => {
                write!(formatter, "宝宝模式返回格式无效：{detail}")
            }
            Self::RepairExhausted { detail } => write!(formatter, "宝宝模式连续修复失败：{detail}"),
            Self::Cancelled => formatter.write_str("宝宝模式分析已取消"),
            Self::Provider { detail } => write!(formatter, "宝宝模式 provider 失败：{detail}"),
            Self::ProgressClosed => formatter.write_str("宝宝模式进度通道已关闭"),
            Self::CacheUnavailable => formatter.write_str("宝宝模式缓存不可用"),
            Self::CodexNotFound => formatter.write_str("未找到 Codex 可执行文件"),
            Self::CodexExecutableInvalid => formatter.write_str("选择的 Codex 可执行文件不可用"),
            Self::CodexSignedOut => formatter.write_str("Codex 尚未登录"),
            Self::CodexIncompatible => formatter.write_str("Codex App Server 缺少宝宝模式所需能力"),
            Self::CodexInitializationFailed => formatter.write_str("Codex App Server 初始化失败"),
        }
    }
}

impl std::error::Error for BabysitError {}

/// provider-independent 的 Installed/Built-in Agent 接口。
#[async_trait]
pub trait BabysitAgentProvider: Send + Sync {
    /// 返回用于缓存隔离和诊断分类的稳定 provider identity。
    fn provider_id(&self) -> &str;
    /// 返回本次 provider 进程 generation，进程重启后不得复用旧结果。
    fn process_generation(&self) -> &str;
    /// 生成完整 before/after 解释；provider 不得拆成逐行请求。
    async fn generate(
        &self,
        request: BabysitProviderRequest,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError>;
    /// 在同一 provider thread 上修复上一次被 Narrant 拒绝的完整输出。
    async fn repair(
        &self,
        request: BabysitProviderRequest,
        previous: BabysitFileExplanations,
        failure: ValidationFailure,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError>;
    /// 上层在 repair 之间提前退出时，释放 provider 私有的 thread/run 状态。
    async fn abandon(&self, _request: &BabysitProviderRequest) {}
    /// 应用退出时释放 provider 私有的线程与 App Server；fake provider 无需额外动作。
    async fn shutdown(&self) {}
}

/// 缓存中不可变的 Review identity；不把 review 与 snapshot 合并成单一 ID。
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ReviewIdentity {
    pub review_id: String,
    pub snapshot_id: String,
}

/// 一次 file analysis 的调用参数与精确 Review Snapshot identity。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BabysitFileRequest {
    pub identity: ReviewIdentity,
    pub path: String,
    pub locale: String,
    /// 当前 Review Workspace 的绝对路径，不进入前端 identity 或缓存 key。
    pub workspace_path: String,
    pub detected_language: Option<String>,
}

/// UI 查询文件是否可进入 Babysit 的 provider-independent 结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BabysitEligibility {
    pub eligible: bool,
    pub reason: Option<BabysitEligibilityReason>,
}

/// 后端唯一维护的 Babysit 不可用原因；前端只负责本地化展示。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BabysitEligibilityReason {
    NonText,
    GeneratedOrVendored,
    LockMinifiedOrSnapshot,
    UnsupportedLanguage,
}

/// 仅在进程内存在的完整结果缓存 key。
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct CacheKey {
    review_id: String,
    snapshot_id: String,
    path: String,
    locale: String,
    detected_language: Option<String>,
    provider_id: String,
    process_generation: String,
    contract_version: String,
}

/// 负责 eligibility、provider orchestration、校验和 process-local cache 的深模块。
pub struct BabysitApplication {
    provider: Arc<dyn BabysitAgentProvider>,
    codex_runtime: Option<Arc<codex::runtime::CodexRuntime>>,
    cache: Mutex<HashMap<CacheKey, BabysitFileExplanations>>,
}

impl BabysitApplication {
    /// 注入一个 provider，供模块测试验证 provider-independent 编排。
    #[cfg(test)]
    pub fn new(provider: Arc<dyn BabysitAgentProvider>) -> Self {
        Self {
            provider,
            codex_runtime: None,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// 构造带有 application settings 路径的生产 Codex provider；发现仍保持懒加载。
    pub fn with_codex_provider_at(settings_path: impl Into<std::path::PathBuf>) -> Self {
        let provider = Arc::new(CodexAppServerProvider::new("codex"));
        let runtime = Arc::new(codex::runtime::CodexRuntime::new(
            provider.clone(),
            settings_path.into(),
        ));
        Self {
            provider,
            codex_runtime: Some(runtime),
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// 将首次 provider 发现和 App Server 握手保持在 Babysit 点击后的最高 seam。
    pub async fn prepare_runtime(&self, workspace_path: &Path) -> Result<(), BabysitError> {
        if let Some(runtime) = &self.codex_runtime {
            runtime.ensure_ready(workspace_path).await
        } else {
            Ok(())
        }
    }

    /// 由 native picker 选择并验证 Codex executable，再写入 durable application setting。
    pub async fn select_codex_executable(&self, path: &Path) -> Result<(), BabysitError> {
        self.codex_runtime
            .as_ref()
            .ok_or(BabysitError::CodexNotFound)?
            .select_executable(path)
            .await
    }

    /// 用后端规则判断 eligibility；detected_language 允许无扩展名和特殊文件名表达已识别语言。
    pub fn eligibility(
        &self,
        path: &str,
        content: &FileChangeContent,
        detected_language: Option<&str>,
    ) -> BabysitEligibility {
        eligibility(path, content, detected_language)
    }

    /// 在启动 discovery/provider 前复用 Babysit 的唯一后端 eligibility gate。
    pub fn validate_file_eligibility(
        &self,
        path: &str,
        content: &FileChangeContent,
        detected_language: Option<&str>,
    ) -> Result<(), BabysitError> {
        ensure_eligible(path, content, detected_language)
    }

    /// 以一个完整 File Change 为单位生成并缓存 before/after Line Explanations。
    pub async fn explain_file<F>(
        &self,
        request: BabysitFileRequest,
        content: FileChangeContent,
        cancellation: &CancellationSignal,
        mut notify: F,
    ) -> Result<BabysitFileExplanations, BabysitError>
    where
        F: FnMut(BabysitProgress) -> Result<(), BabysitError>,
    {
        ensure_not_cancelled(cancellation)?;
        ensure_eligible(
            &request.path,
            &content,
            request.detected_language.as_deref(),
        )?;
        let key = CacheKey {
            review_id: request.identity.review_id.clone(),
            snapshot_id: request.identity.snapshot_id.clone(),
            path: request.path.clone(),
            locale: request.locale.clone(),
            detected_language: request.detected_language.clone(),
            provider_id: self.provider.provider_id().to_owned(),
            process_generation: self.provider.process_generation().to_owned(),
            contract_version: ANALYSIS_CONTRACT_VERSION.to_owned(),
        };
        if let Some(result) = self
            .cache
            .lock()
            .map_err(|_| BabysitError::CacheUnavailable)?
            .get(&key)
            .cloned()
        {
            notify(BabysitProgress::Completed)?;
            return Ok(result);
        }

        notify(BabysitProgress::Generating)?;
        let provider_request = build_provider_request(&request, &content)?;
        let mut output = self
            .provider
            .generate(provider_request.clone(), cancellation)
            .await?;
        let mut failure = match validate_response(&provider_request, &output) {
            Ok(()) => None,
            Err(failure) => Some(failure),
        };
        let mut attempt = 0;
        while let Some(current_failure) = failure.take() {
            if attempt >= MAX_REPAIR_ATTEMPTS {
                self.provider.abandon(&provider_request).await;
                return Err(BabysitError::RepairExhausted {
                    detail: current_failure.to_string(),
                });
            }
            attempt += 1;
            if let Err(error) = ensure_not_cancelled(cancellation) {
                self.provider.abandon(&provider_request).await;
                return Err(error);
            }
            if let Err(error) = notify(BabysitProgress::Repairing { attempt }) {
                self.provider.abandon(&provider_request).await;
                return Err(error);
            }
            output = self
                .provider
                .repair(
                    provider_request.clone(),
                    output,
                    current_failure,
                    cancellation,
                )
                .await?;
            failure = match validate_response(&provider_request, &output) {
                Ok(()) => None,
                Err(failure) => Some(failure),
            };
        }
        ensure_not_cancelled(cancellation)?;
        notify(BabysitProgress::Completed)?;
        self.cache
            .lock()
            .map_err(|_| BabysitError::CacheUnavailable)?
            .insert(key, output.clone());
        Ok(output)
    }

    /// 暴露缓存清理 seam；Review 关闭和应用退出时由后续生命周期 issue 调用。
    pub fn clear(&self) -> Result<(), BabysitError> {
        self.cache
            .lock()
            .map_err(|_| BabysitError::CacheUnavailable)?
            .clear();
        Ok(())
    }

    /// 删除一个旧 Snapshot 的全部解释，避免 refresh 后残留旧 revision 的结果。
    pub fn clear_snapshot(&self, review_id: &str, snapshot_id: &str) -> Result<(), BabysitError> {
        self.cache
            .lock()
            .map_err(|_| BabysitError::CacheUnavailable)?
            .retain(|key, _| !(key.review_id == review_id && key.snapshot_id == snapshot_id));
        Ok(())
    }

    /// 删除一个 Review 的全部解释；切换或关闭 Review 时不保留跨 Review 缓存。
    pub fn clear_review(&self, review_id: &str) -> Result<(), BabysitError> {
        self.cache
            .lock()
            .map_err(|_| BabysitError::CacheUnavailable)?
            .retain(|key, _| key.review_id != review_id);
        Ok(())
    }

    /// 应用退出时清空缓存并交给 provider 释放进程/线程资源。
    pub async fn shutdown(&self) -> Result<(), BabysitError> {
        let clear_result = self.clear();
        if let Some(runtime) = &self.codex_runtime {
            runtime.shutdown().await;
        } else {
            self.provider.shutdown().await;
        }
        clear_result
    }
}

/// 检查可取消操作是否已被前端取消。
fn ensure_not_cancelled(cancellation: &CancellationSignal) -> Result<(), BabysitError> {
    cancellation
        .is_cancelled()
        .then_some(BabysitError::Cancelled)
        .map_or(Ok(()), Err)
}

/// 第一版显式代码文件 eligibility；内容类别先由 Review Reader 的 immutable 结果决定。
fn ensure_eligible(
    path: &str,
    content: &FileChangeContent,
    detected_language: Option<&str>,
) -> Result<(), BabysitError> {
    let result = eligibility(path, content, detected_language);
    if result.eligible {
        return Ok(());
    }
    Err(BabysitError::UnsupportedFile {
        reason: format!(
            "{:?}",
            result
                .reason
                .unwrap_or(BabysitEligibilityReason::UnsupportedLanguage)
        ),
    })
}

/// 计算稳定 eligibility code，避免 UI 与 provider 各自维护文件白名单。
fn eligibility(
    path: &str,
    content: &FileChangeContent,
    detected_language: Option<&str>,
) -> BabysitEligibility {
    let rejected = |reason| BabysitEligibility {
        eligible: false,
        reason: Some(reason),
    };
    if content.content_kind() != FileContentKind::Text {
        return rejected(BabysitEligibilityReason::NonText);
    }
    let normalized = path.replace('\\', "/");
    let components = normalized.split('/').collect::<Vec<_>>();
    if components.iter().any(|component| {
        matches!(
            *component,
            "node_modules" | "vendor" | "dist" | "build" | "target" | "generated" | "__snapshots__"
        )
    }) {
        return rejected(BabysitEligibilityReason::GeneratedOrVendored);
    }
    let basename = components
        .last()
        .copied()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if basename.ends_with(".lock")
        || matches!(
            basename.as_str(),
            "cargo.lock" | "package-lock.json" | "yarn.lock" | "pnpm-lock.yaml"
        )
        || basename.ends_with(".min.js")
        || basename.ends_with(".min.ts")
        || basename.ends_with(".snap")
        || basename.ends_with(".snapshot")
        || basename.contains(".generated.")
        || basename.ends_with("_generated.rs")
    {
        return rejected(BabysitEligibilityReason::LockMinifiedOrSnapshot);
    }
    let extension = basename.rsplit_once('.').map(|(_, extension)| extension);
    let allowed = matches!(
        extension,
        Some(
            "ts" | "tsx"
                | "mts"
                | "js"
                | "jsx"
                | "mjs"
                | "rs"
                | "py"
                | "go"
                | "java"
                | "c"
                | "cc"
                | "cpp"
                | "cxx"
                | "h"
                | "hpp"
        )
    );
    let detected_allowed = matches!(
        detected_language.map(str::to_ascii_lowercase).as_deref(),
        Some("typescript" | "javascript" | "rust" | "python" | "go" | "java" | "c" | "cpp")
    );
    if !allowed && !detected_allowed {
        return rejected(BabysitEligibilityReason::UnsupportedLanguage);
    }
    BabysitEligibility {
        eligible: true,
        reason: None,
    }
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
        path: request.path.clone(),
        locale: request.locale.clone(),
        workspace_path: request.workspace_path.clone(),
        before: before.to_owned(),
        after: after.to_owned(),
        targets,
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

/// 对 provider 返回的 before/after map 执行严格 coverage 与文本安全校验。
pub fn validate_response(
    request: &BabysitProviderRequest,
    output: &BabysitFileExplanations,
) -> Result<(), ValidationFailure> {
    validate_target_inventory_for_response(&request.targets)?;
    validate_side(FileSide::Before, &request.targets, &output.before)?;
    validate_side(FileSide::After, &request.targets, &output.after)
}

/// 检查 provider 请求中的 target identity 是否出现重复。
fn validate_target_inventory_for_response(targets: &[LineTarget]) -> Result<(), ValidationFailure> {
    let mut seen = HashSet::new();
    for target in targets {
        if !seen.insert((target.side, target.line_number)) {
            return Err(ValidationFailure::DuplicateTarget {
                side: target.side,
                line_number: target.line_number,
            });
        }
    }
    Ok(())
}

/// 校验一侧 map 的 exact coverage、key 格式和解释安全约束。
fn validate_side(
    side: FileSide,
    targets: &[LineTarget],
    explanations: &BTreeMap<String, String>,
) -> Result<(), ValidationFailure> {
    let expected = targets
        .iter()
        .filter(|target| target.side == side)
        .map(|target| target.line_number.to_string())
        .collect::<HashSet<_>>();
    for key in explanations.keys() {
        if key.parse::<usize>().ok().filter(|line| *line > 0).is_none() {
            return Err(ValidationFailure::InvalidLineNumber {
                side,
                line_number: key.clone(),
            });
        }
        if !expected.contains(key) {
            return Err(ValidationFailure::UnexpectedLine {
                side,
                line_number: key.clone(),
            });
        }
    }
    if let Some(missing) = expected
        .iter()
        .find(|line_number| !explanations.contains_key(*line_number))
    {
        return Err(ValidationFailure::MissingLine {
            side,
            line_number: missing.parse().unwrap_or_default(),
        });
    }
    for (key, explanation) in explanations {
        let line_number = key.parse::<usize>().unwrap_or_default();
        if explanation.trim().is_empty() {
            return Err(ValidationFailure::EmptyExplanation { side, line_number });
        }
        if explanation.chars().count() > MAX_EXPLANATION_CHARS {
            return Err(ValidationFailure::ExplanationTooLong { side, line_number });
        }
        if explanation.chars().any(char::is_control) {
            return Err(ValidationFailure::UnsafeExplanation { side, line_number });
        }
    }
    Ok(())
}

/// Fake provider 的可预测错误脚本；用于契约和 repair loop 测试。
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FakeProviderMode {
    #[default]
    Complete,
    InvalidThenRepair,
}

/// 不依赖网络、登录或模型的 deterministic provider。
#[cfg(test)]
pub struct FakeBabysitProvider {
    mode: FakeProviderMode,
    invalid_rounds_remaining: AtomicU8,
    generate_calls: AtomicUsize,
    repair_calls: AtomicUsize,
    shutdown_calls: AtomicUsize,
}

#[cfg(test)]
impl Default for FakeBabysitProvider {
    /// 创建默认返回完整解释的 deterministic fake provider。
    fn default() -> Self {
        Self::new(FakeProviderMode::Complete)
    }
}

#[cfg(test)]
impl FakeBabysitProvider {
    /// 构造一个指定输出脚本的 fake provider。
    pub fn new(mode: FakeProviderMode) -> Self {
        Self {
            mode,
            invalid_rounds_remaining: AtomicU8::new(
                if mode == FakeProviderMode::InvalidThenRepair {
                    1
                } else {
                    0
                },
            ),
            generate_calls: AtomicUsize::new(0),
            repair_calls: AtomicUsize::new(0),
            shutdown_calls: AtomicUsize::new(0),
        }
    }

    /// 构造前 N 次输出无效、随后输出完整结果的 provider。
    pub fn with_invalid_rounds(rounds: u8) -> Self {
        Self {
            mode: FakeProviderMode::InvalidThenRepair,
            invalid_rounds_remaining: AtomicU8::new(rounds),
            generate_calls: AtomicUsize::new(0),
            repair_calls: AtomicUsize::new(0),
            shutdown_calls: AtomicUsize::new(0),
        }
    }

    /// 返回 generate 次数，测试用来证明完整文件只触发一次 provider request。
    pub fn generate_calls(&self) -> usize {
        self.generate_calls.load(Ordering::Acquire)
    }

    /// 返回 repair 次数，测试用来证明 repair budget 的边界。
    pub fn repair_calls(&self) -> usize {
        self.repair_calls.load(Ordering::Acquire)
    }

    /// 返回 shutdown 次数，用于验证应用退出只释放一次 provider 资源。
    pub fn shutdown_calls(&self) -> usize {
        self.shutdown_calls.load(Ordering::Acquire)
    }

    /// 按请求 targets 构造稳定解释，故意不复述 source，避免 fake 测试输出污染日志。
    fn complete_output(request: &BabysitProviderRequest) -> BabysitFileExplanations {
        let mut output = BabysitFileExplanations::default();
        for target in &request.targets {
            let text = if request.locale.to_ascii_lowercase().starts_with("en") {
                format!(
                    "Explains line {} in the {:?} file side.",
                    target.line_number, target.side
                )
            } else {
                format!(
                    "解释文件 {:?} 一侧的第 {} 行代码。",
                    target.side, target.line_number
                )
            };
            match target.side {
                FileSide::Before => {
                    output.before.insert(target.line_number.to_string(), text);
                }
                FileSide::After => {
                    output.after.insert(target.line_number.to_string(), text);
                }
            }
        }
        output
    }

    /// 按错误脚本删去一个 target，制造一个可被 repair 修复的 malformed output。
    fn scripted_output(&self, request: &BabysitProviderRequest) -> BabysitFileExplanations {
        let mut output = Self::complete_output(request);
        if self.invalid_rounds_remaining.load(Ordering::Acquire) > 0 {
            if let Some(target) = request.targets.first() {
                match target.side {
                    FileSide::Before => {
                        output.before.remove(&target.line_number.to_string());
                    }
                    FileSide::After => {
                        output.after.remove(&target.line_number.to_string());
                    }
                }
            }
        }
        output
    }
}

#[async_trait]
#[cfg(test)]
impl BabysitAgentProvider for FakeBabysitProvider {
    /// 返回 fake provider 的稳定缓存 identity。
    fn provider_id(&self) -> &str {
        "fake"
    }

    /// 返回 fake 进程 generation，测试中保持固定以便验证缓存行为。
    fn process_generation(&self) -> &str {
        "fake-process-v1"
    }

    /// 一次生成完整 File Change 的所有解释，不拆分逐行调用。
    async fn generate(
        &self,
        request: BabysitProviderRequest,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        ensure_not_cancelled(cancellation)?;
        self.generate_calls.fetch_add(1, Ordering::AcqRel);
        let output = if self.mode == FakeProviderMode::InvalidThenRepair {
            self.scripted_output(&request)
        } else {
            Self::complete_output(&request)
        };
        if self.invalid_rounds_remaining.load(Ordering::Acquire) > 0 {
            self.invalid_rounds_remaining.fetch_sub(1, Ordering::AcqRel);
        }
        Ok(output)
    }

    /// 按脚本修复上一轮完整响应，模拟同一 provider thread 的 repair。
    async fn repair(
        &self,
        request: BabysitProviderRequest,
        _previous: BabysitFileExplanations,
        _failure: ValidationFailure,
        cancellation: &CancellationSignal,
    ) -> Result<BabysitFileExplanations, BabysitError> {
        ensure_not_cancelled(cancellation)?;
        self.repair_calls.fetch_add(1, Ordering::AcqRel);
        let output = self.scripted_output(&request);
        if self.invalid_rounds_remaining.load(Ordering::Acquire) > 0 {
            self.invalid_rounds_remaining.fetch_sub(1, Ordering::AcqRel);
        }
        Ok(output)
    }

    /// Fake provider 没有子进程，但记录 shutdown 以覆盖应用生命周期 seam。
    async fn shutdown(&self) {
        self.shutdown_calls.fetch_add(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
