use crate::operation::CancellationSignal;
use crate::review::{FileChangeContent, FileContentKind};
use async_trait::async_trait;
use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::sync::{
    atomic::{AtomicU8, AtomicUsize, Ordering},
    Arc, Mutex,
};

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

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("包含 before 和 after 的解释对象")
    }

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
        Ok(BabysitFileExplanations {
            before: before.unwrap_or_default(),
            after: after.unwrap_or_default(),
        })
    }
}

/// 保留 JSON map 中行号 key 的唯一性。
struct UniqueLineMap(BTreeMap<String, String>);

impl<'de> Deserialize<'de> for UniqueLineMap {
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

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("行号到解释文本的对象")
    }

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
    UnsupportedFile { reason: String },
    InvalidProviderOutput { detail: String },
    RepairExhausted { detail: String },
    Cancelled,
    Provider { detail: String },
    ProgressClosed,
    CacheUnavailable,
}

impl fmt::Display for BabysitError {
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
}

/// 仅在进程内存在的完整结果缓存 key。
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
struct CacheKey {
    review_id: String,
    snapshot_id: String,
    path: String,
    provider_id: String,
    process_generation: String,
    contract_version: String,
}

/// 负责 eligibility、provider orchestration、校验和 process-local cache 的深模块。
pub struct BabysitApplication {
    provider: Arc<dyn BabysitAgentProvider>,
    cache: Mutex<HashMap<CacheKey, BabysitFileExplanations>>,
}

impl BabysitApplication {
    /// 注入一个 provider，生产与测试都从这个 seam 进入 Babysit。
    pub fn new(provider: Arc<dyn BabysitAgentProvider>) -> Self {
        Self {
            provider,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// 构造当前 vertical slice 使用的 deterministic fake provider。
    pub fn with_fake_provider() -> Self {
        Self::new(Arc::new(FakeBabysitProvider::default()))
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
        ensure_eligible(&request.path, &content)?;
        let key = CacheKey {
            review_id: request.identity.review_id.clone(),
            snapshot_id: request.identity.snapshot_id.clone(),
            path: request.path.clone(),
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
                return Err(BabysitError::RepairExhausted {
                    detail: current_failure.to_string(),
                });
            }
            attempt += 1;
            ensure_not_cancelled(cancellation)?;
            notify(BabysitProgress::Repairing { attempt })?;
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
}

/// 检查可取消操作是否已被前端取消。
fn ensure_not_cancelled(cancellation: &CancellationSignal) -> Result<(), BabysitError> {
    cancellation
        .is_cancelled()
        .then_some(BabysitError::Cancelled)
        .map_or(Ok(()), Err)
}

/// 第一版显式代码文件 eligibility；内容类别先由 Review Reader 的 immutable 结果决定。
fn ensure_eligible(path: &str, content: &FileChangeContent) -> Result<(), BabysitError> {
    if content.content_kind() != FileContentKind::Text {
        return Err(BabysitError::UnsupportedFile {
            reason: "仅支持文本代码文件".to_owned(),
        });
    }
    let normalized = path.replace('\\', "/");
    let components = normalized.split('/').collect::<Vec<_>>();
    if components.iter().any(|component| {
        matches!(
            *component,
            "node_modules" | "vendor" | "dist" | "build" | "target"
        )
    }) {
        return Err(BabysitError::UnsupportedFile {
            reason: "不分析依赖、构建产物或 vendored 文件".to_owned(),
        });
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
    {
        return Err(BabysitError::UnsupportedFile {
            reason: "不分析 lock、minified 或 snapshot 文件".to_owned(),
        });
    }
    let extension = basename.rsplit_once('.').map(|(_, extension)| extension);
    let allowed = matches!(
        extension,
        Some(
            "ts" | "tsx"
                | "js"
                | "jsx"
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
    if !allowed {
        return Err(BabysitError::UnsupportedFile {
            reason: "文件扩展名不在首批代码语言白名单中".to_owned(),
        });
    }
    Ok(())
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FakeProviderMode {
    #[default]
    Complete,
    InvalidThenRepair,
}

/// 不依赖网络、登录或模型的 deterministic provider。
pub struct FakeBabysitProvider {
    mode: FakeProviderMode,
    invalid_rounds_remaining: AtomicU8,
    generate_calls: AtomicUsize,
    repair_calls: AtomicUsize,
}

impl Default for FakeBabysitProvider {
    fn default() -> Self {
        Self::new(FakeProviderMode::Complete)
    }
}

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
        }
    }

    /// 构造前 N 次输出无效、随后输出完整结果的 provider。
    pub fn with_invalid_rounds(rounds: u8) -> Self {
        Self {
            mode: FakeProviderMode::InvalidThenRepair,
            invalid_rounds_remaining: AtomicU8::new(rounds),
            generate_calls: AtomicUsize::new(0),
            repair_calls: AtomicUsize::new(0),
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
impl BabysitAgentProvider for FakeBabysitProvider {
    fn provider_id(&self) -> &str {
        "fake"
    }

    fn process_generation(&self) -> &str {
        "fake-process-v1"
    }

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
}

#[cfg(test)]
#[path = "tests/mod.rs"]
mod tests;
