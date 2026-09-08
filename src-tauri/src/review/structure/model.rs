//! Review 结构分析的 owned 应用契约；解析器、覆盖证明及请求运行身份留在内部。

use serde::{Deserialize, Serialize};

/// Snapshot 内配对完成的声明或容器身份，不能仅由名称推导。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StructureNodeId(pub String);

/// Snapshot 内独立于声明成员的 impl 分组身份。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StructureGroupId(pub String);

/// Snapshot 内按配对端点、关系种类与确定性建立的稳定身份。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StructureRelationId(pub String);

/// Snapshot 内真实文件对的身份，与分析请求或声明移动无关。
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StructureSourceId(pub String);

/// 旧侧固定为 Comparison Base，新侧固定为 Head。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureSide {
    Before,
    After,
}

/// 原始文本的 1-based UTF-16 范围，起点包含、终点不包含；诊断可为零宽。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureRange {
    pub start_line: u32,
    pub start_column: u32,
    pub end_line: u32,
    pub end_column: u32,
}

/// 应用拥有的语言名称，TSX/JSX 分别归入对应语言族。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StructureLanguage {
    TypeScript,
    JavaScript,
    Rust,
}

/// 跨语言过滤类别，不替换具体原生种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureCategory {
    Class,
    Struct,
    Trait,
    Interface,
    Type,
    Enum,
    Function,
    Method,
    Field,
    Constant,
    Module,
    Namespace,
    Member,
}

/// 首版稳定的原生词汇，由 adapter 映射；impl 另存为分组，React 不另设种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureNativeName {
    Class,
    Struct,
    Trait,
    Interface,
    TypeAlias,
    Enum,
    EnumVariant,
    Function,
    ArrowFunction,
    Method,
    AssociatedFunction,
    Constructor,
    Field,
    Property,
    Getter,
    Setter,
    Constant,
    Static,
    Module,
    Namespace,
    AssociatedType,
    AssociatedConstant,
    CallSignature,
    ConstructSignature,
    IndexSignature,
}

/// 分析模块直接提供原生名称和过滤维度，UI 不解释 grammar 节点。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureNativeKind {
    pub language: StructureLanguage,
    pub category: StructureCategory,
    pub name: StructureNativeName,
}

/// 失败、未覆盖和歧义分别保留，均不能证明不存在。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureDiagnosticReason {
    ParseError,
    MissingSyntax,
    UnreliableScope,
    UnsupportedLanguage,
    UnsupportedFileKind,
    Cancelled,
    ResourceLimit,
    GitObjectUnavailable,
    SourceUnreadable,
    ConfigurationUnavailable,
    DynamicTarget,
    AmbiguousMatch,
    AmbiguousTarget,
}

/// 可直接展示的诊断；缺少位置仅表示仓库级或无法确定准确范围。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureDiagnostic {
    pub reason: StructureDiagnosticReason,
    pub message: String,
    pub side: Option<StructureSide>,
    pub path: Option<String>,
    pub range: Option<StructureRange>,
    pub retryable: bool,
}

/// Absent 仅由内部覆盖证明产生；Unknown 必须携带至少一条直接诊断。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum StructureFact<T> {
    Present {
        value: T,
    },
    Absent,
    Unknown {
        diagnostics: Vec<StructureDiagnostic>,
    },
}

/// 定位用途；所有范围始终指向原始完整文件。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureLocationRole {
    Declaration,
    Implementation,
    Documentation,
    Occurrence,
}

/// 直接携带源码范围，避免向调用方暴露另一套位置 ID 图。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureLocation {
    pub source_id: StructureSourceId,
    pub side: StructureSide,
    pub range: StructureRange,
    pub role: StructureLocationRole,
}

/// 一侧声明的真实归属与来源；locations 非空，首项是默认打开位置。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureDeclarationSide {
    pub native_kind: StructureNativeKind,
    pub name: String,
    pub qualified_name: String,
    pub signature: String,
    pub parent_id: StructureNodeId,
    pub implementation_group_id: Option<StructureGroupId>,
    pub locations: Vec<StructureLocation>,
}

/// 一个声明可同时具备多个标签，但声明变化总数只计算一次。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureChangeLabel {
    Added,
    Deleted,
    Renamed,
    Moved,
    Modified,
    Documentation,
}

/// 空标签只有在比较完整时才表示未修改。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureComparison {
    Complete,
    Incomplete,
}

/// 各侧层级及两侧并集中的变化后代去重数，不包含父级自身。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureContainedChanges {
    pub before: u32,
    pub after: u32,
    pub union: u32,
}

/// 配对声明只绘制一次；双侧 parent 保留移动前后的真实层级。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureDeclaration {
    pub id: StructureNodeId,
    pub before: StructureFact<StructureDeclarationSide>,
    pub after: StructureFact<StructureDeclarationSide>,
    pub changes: Vec<StructureChangeLabel>,
    pub comparison: StructureComparison,
    pub diagnostics: Vec<StructureDiagnostic>,
    pub display_parent_id: StructureNodeId,
    pub contained_changes: StructureContainedChanges,
}

/// impl signature 仅含自身头部；成员签名、正文和文档不参与聚合类型自身修改。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureImplementationSide {
    pub owner_id: StructureNodeId,
    pub trait_name: Option<String>,
    pub trait_target_id: Option<StructureNodeId>,
    pub signature: String,
    pub locations: Vec<StructureLocation>,
}

/// 类型内的 impl 保持各自源码与成员归属，分组本身不增加声明计数。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureImplementationGroup {
    pub id: StructureGroupId,
    pub before: StructureFact<StructureImplementationSide>,
    pub after: StructureFact<StructureImplementationSide>,
}

/// 文件容器的路径来自真实 source，未成功提取的可读文件仍可导航。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureFileNode {
    pub id: StructureNodeId,
    pub source_id: StructureSourceId,
    pub contained_changes: StructureContainedChanges,
}

/// 首版不扫描仓库外依赖源码，不能提供虚假的本地定义。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureExternalDefinitionUnavailable {
    ExternalSourceNotIndexed,
}

/// 外部类型可拥有本地 impl 分组，但其定义本身没有本地位置。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureExternalNode {
    pub id: StructureNodeId,
    pub name: String,
    pub package_name: String,
    pub definition_unavailable: StructureExternalDefinitionUnavailable,
}

/// 仅承载 owned 领域节点；内部标签为前端稳定的 kind 字段。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum StructureNode {
    Declaration(StructureDeclaration),
    File(StructureFileNode),
    External(StructureExternalNode),
}

/// 关系种类独立于变化标签；归属由双侧 parent/group 表达。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureRelationshipKind {
    Calls,
    Extends,
    Implements,
    TypeReference,
    Imports,
}

/// 候选边默认隐藏，不能当作已确认的关系变化。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureCertainty {
    Resolved,
    Candidate,
}

/// 缺侧未能确认时保留 Indeterminate，不根据空数组推断增删。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureRelationshipChange {
    Added,
    Deleted,
    Retained,
    Indeterminate,
}

/// 匿名回调携带自身范围，不能把其调用称为词法父声明的直接调用。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum StructureExecution {
    DeclarationBody,
    AnonymousCallback { callback_range: StructureRange },
    File,
}

/// 内置分析可展示的绑定依据，不包含语法查询或 provider 句柄。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureEvidenceKind {
    LexicalBinding,
    ExplicitImport,
    QualifiedName,
    KnownReceiver,
    ExplicitType,
    ModulePath,
}

/// 同一位置可以保留多个独立绑定依据。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureEvidence {
    pub kind: StructureEvidenceKind,
    pub detail: String,
}

/// 聚合边保留每个真实发生位置和执行来源。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureOccurrence {
    pub location: StructureLocation,
    pub execution: StructureExecution,
    pub evidence: Vec<StructureEvidence>,
}

/// 按匹配端点及关系种类汇总，仅行号或发生次数变化不制造边增删。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureRelationship {
    pub id: StructureRelationId,
    pub from_id: StructureNodeId,
    pub to_id: StructureNodeId,
    pub kind: StructureRelationshipKind,
    pub certainty: StructureCertainty,
    pub before: StructureFact<Vec<StructureOccurrence>>,
    pub after: StructureFact<Vec<StructureOccurrence>>,
    pub change: StructureRelationshipChange,
}

/// 未解析的位置可独立查看，候选列表不构成确定目标。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureUnresolvedRelationship {
    pub side: StructureSide,
    pub from_id: StructureNodeId,
    pub kind: StructureRelationshipKind,
    pub written_target: String,
    pub occurrence: StructureOccurrence,
    pub candidate_target_ids: Vec<StructureNodeId>,
    pub diagnostics: Vec<StructureDiagnostic>,
}

/// 展示覆盖程度；完整语法覆盖不能替代关系解析覆盖。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureCoverageStatus {
    Complete,
    Partial,
    NotStarted,
}

/// 已发现文件逐项说明提取和关系覆盖，不外传内部 absence proof。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureFileCoverage {
    pub side: StructureSide,
    pub path: String,
    pub extraction: StructureCoverageStatus,
    pub relationships: StructureCoverageStatus,
    pub diagnostics: Vec<StructureDiagnostic>,
}

/// 两侧完整 tree 的发现状态，与单个文件的语法状态无关。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureDiscoveryCoverage {
    pub before: StructureCoverageStatus,
    pub after: StructureCoverageStatus,
}

/// 仓库级发现失败与逐文件分析限制分别报告。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureCoverage {
    pub discovery: StructureDiscoveryCoverage,
    pub files: Vec<StructureFileCoverage>,
    pub diagnostics: Vec<StructureDiagnostic>,
}

/// 任务结束方式不代替覆盖程度；Completed 仍可能只有部分语义结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureAnalysisStatus {
    Running,
    Completed,
    Cancelled,
    Failed,
}

/// 声明与关系分别去重计数；complete 为所请求分析的覆盖结论。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureSummary {
    pub changed_declarations: u32,
    pub relationship_changes: u32,
    pub incomplete_declarations: u32,
    pub unresolved_occurrences: u32,
    pub complete: bool,
}

/// 初始变化焦点、双侧一跳上下文及必要容器；隐藏连线不删除变化入口。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureInitialFocus {
    pub node_ids: Vec<StructureNodeId>,
    pub relationship_ids: Vec<StructureRelationId>,
}

/// 固定 Snapshot 的完整 owned 检查点，引用必须自洽，发布的匹配 ID 不再重排。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureResult {
    pub review_id: String,
    pub snapshot_id: String,
    pub status: StructureAnalysisStatus,
    pub nodes: Vec<StructureNode>,
    pub implementation_groups: Vec<StructureImplementationGroup>,
    pub relationships: Vec<StructureRelationship>,
    pub unresolved_relationships: Vec<StructureUnresolvedRelationship>,
    pub sources: Vec<StructurePeekSource>,
    pub coverage: StructureCoverage,
    pub summary: StructureSummary,
    pub initial_focus: StructureInitialFocus,
}

/// 后台分析阶段只用于报告进度，调用方不编排阶段执行。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructurePhase {
    Discovery,
    Extraction,
    Matching,
    Relationships,
}

/// 进度统一按文件计数，发现未结束时 total_files 为 None。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureProgress {
    pub phase: StructurePhase,
    pub side: Option<StructureSide>,
    pub processed_files: u32,
    pub total_files: Option<u32>,
}

/// 同请求按序发送；旧回写由调用方 generation、Snapshot 和终态保护拦截。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum StructureEvent {
    Progress { progress: StructureProgress },
    Checkpoint { result: StructureResult },
}

/// 文件对一侧的真实路径和版本；缺侧必须被确认，读取失败不改变此描述符。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum StructureSourceSide {
    Present { path: String, revision: String },
    Absent,
}

/// 与现有 File Change 摘要一致的状态，不引入结构变化语义。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureFileChangeStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
}

/// 实际 File Change 的最小 owned 摘要，允许显式在 File Changes 中打开。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureFileChangeSummary {
    pub path: String,
    pub old_path: Option<String>,
    pub status: StructureFileChangeStatus,
}

/// 未修改上下文不创建假的 File Change；文件对由 Snapshot 决定。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum StructureSourceClassification {
    FileChange {
        file_change: StructureFileChangeSummary,
    },
    Unchanged {
        path: String,
    },
}

/// rename 沿用真实 oldPath/path；跨文件结构移动必须使用两个真实文件对。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructurePeekSource {
    pub id: StructureSourceId,
    pub before: StructureSourceSide,
    pub after: StructureSourceSide,
    pub classification: StructureSourceClassification,
}

/// Peek 来源身份独立于 File Changes 选择和分析运行身份。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructurePeekIdentity {
    pub review_id: String,
    pub snapshot_id: String,
    pub source_id: StructureSourceId,
}

/// 不可阅读的原因，与 Git 确认的文件缺侧不同。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureSourceUnavailableReason {
    GitObjectUnavailable,
    SourceUnreadable,
    Binary,
    Symlink,
    Submodule,
    Cancelled,
    ResourceLimit,
}

/// 只有真实 Git tree 缺失可以构造空侧，解析失败不满足这个原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum StructureSourceAbsenceReason {
    GitTreeAbsence,
}

/// Available 始终是完整原始文本；空文本、确认缺侧和不可用分别表达。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "state",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum StructureSourceContent {
    Available {
        path: String,
        revision: String,
        object_id: String,
        text: String,
        size: u64,
    },
    Absent {
        reason: StructureSourceAbsenceReason,
    },
    Unavailable {
        reason: StructureSourceUnavailableReason,
        message: String,
        retryable: bool,
    },
}

/// 一侧读取失败仍保留另一侧，不能把不可用内容转为空文本。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructurePeekContent {
    pub review_id: String,
    pub snapshot_id: String,
    pub source_id: StructureSourceId,
    pub source: StructurePeekSource,
    pub before: StructureSourceContent,
    pub after: StructureSourceContent,
}

/// 读取固定 Snapshot 的实际侧和路径，不依赖已经完成的分析结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StructureSourceRequest {
    pub side: StructureSide,
    pub path: String,
}
