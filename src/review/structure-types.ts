import type { FileChangeSummary, ReviewIdentity } from "@/review/types";

/** Snapshot 内配对完成的声明或容器身份，调用方不得从路径重新推导。 */
export type StructureNodeId = string;

/** Snapshot 内独立于声明成员的 impl 分组身份。 */
export type StructureGroupId = string;

/** Snapshot 内由配对端点、关系种类及确定性组成的稳定身份。 */
export type StructureRelationId = string;

/** Snapshot 内真实文件对的身份，与分析请求和结构移动无关。 */
export type StructureSourceId = string;

/** 旧侧固定为 Comparison Base，新侧固定为 Head。 */
export type StructureSide = "before" | "after";

/** 原始文本中的 1-based UTF-16 范围，起点包含、终点不包含。 */
export type StructureRange = {
  startLine: number;
  startColumn: number;
  endLine: number;
  endColumn: number;
};

/** 分析模块拥有的语言名称，TSX/JSX 分别归入对应语言族。 */
export type StructureLanguage = "typescript" | "javascript" | "rust";

/** 跨语言过滤类别，不替换原生声明种类。 */
export type StructureCategory =
  | "class" | "struct" | "trait" | "interface" | "type" | "enum"
  | "function" | "method" | "field" | "constant" | "module" | "namespace" | "member";

/** 首版明确支持的原生词汇；adapter 映射语法，不向 UI 暴露 grammar 名称。 */
export type StructureNativeName =
  | "class" | "struct" | "trait" | "interface" | "type-alias" | "enum" | "enum-variant"
  | "function" | "arrow-function" | "method" | "associated-function" | "constructor"
  | "field" | "property" | "getter" | "setter" | "constant" | "static"
  | "module" | "namespace" | "associated-type" | "associated-constant"
  | "call-signature" | "construct-signature" | "index-signature";

/** 原生结构与过滤类别由分析结果给出；React 组件仍是 function/arrow-function。 */
export type StructureNativeKind = {
  language: StructureLanguage;
  category: StructureCategory;
  name: StructureNativeName;
};

/** 失败、未覆盖及静态分析歧义分别保留，均不等价于不存在。 */
export type StructureDiagnosticReason =
  | "parse-error" | "missing-syntax" | "unreliable-scope" | "unsupported-language"
  | "unsupported-file-kind" | "cancelled" | "resource-limit" | "git-object-unavailable"
  | "source-unreadable" | "configuration-unavailable" | "dynamic-target"
  | "ambiguous-match" | "ambiguous-target";

/** 可直接显示的诊断；null 位置仅表示仓库级或无准确范围的诊断。 */
export type StructureDiagnostic = {
  reason: StructureDiagnosticReason;
  message: string;
  side: StructureSide | null;
  path: string | null;
  range: StructureRange | null;
  retryable: boolean;
};

/** absent 仅由后端覆盖证明产生；unknown 必须提供至少一项直接诊断。 */
export type StructureFact<T> =
  | { state: "present"; value: T }
  | { state: "absent" }
  | { state: "unknown"; diagnostics: StructureDiagnostic[] };

/** 定位的用途，所有范围始终对应完整原始文件。 */
export type StructureLocationRole = "declaration" | "implementation" | "documentation" | "occurrence";

/** 位置直接携带范围，通过 sourceId 取得真实文件对，不另建位置身份协议。 */
export type StructureLocation = {
  sourceId: StructureSourceId;
  side: StructureSide;
  range: StructureRange;
  role: StructureLocationRole;
};

/** 一侧声明的真实归属及来源；locations 非空，首项为后端选定的默认位置。 */
export type StructureDeclarationSide = {
  nativeKind: StructureNativeKind;
  name: string;
  qualifiedName: string;
  signature: string;
  parentId: StructureNodeId;
  implementationGroupId: StructureGroupId | null;
  locations: StructureLocation[];
};

/** 同一声明可同时具备多个标签，但在声明变化总数中只计算一次。 */
export type StructureChangeLabel = "added" | "deleted" | "renamed" | "moved" | "modified" | "documentation";

/** 空标签只有在比较完整时才能解释为未修改。 */
export type StructureComparison = "complete" | "incomplete";

/** 各侧层级与两侧并集中的变化后代去重数，均不包含父级自身。 */
export type StructureContainedChanges = { before: number; after: number; union: number };

/** 配对声明仅渲染一次，旧侧归属仍可作为原位置入口。 */
export type StructureDeclaration = {
  kind: "declaration";
  id: StructureNodeId;
  before: StructureFact<StructureDeclarationSide>;
  after: StructureFact<StructureDeclarationSide>;
  changes: StructureChangeLabel[];
  comparison: StructureComparison;
  diagnostics: StructureDiagnostic[];
  displayParentId: StructureNodeId;
  containedChanges: StructureContainedChanges;
};

/** impl signature 仅含自身头部；成员签名、正文及文档不参与聚合类型自身修改。 */
export type StructureImplementationSide = {
  ownerId: StructureNodeId;
  traitName: string | null;
  traitTargetId: StructureNodeId | null;
  signature: string;
  locations: StructureLocation[];
};

/** 同一类型的不同 impl 保持独立来源和成员归属，不增加声明计数。 */
export type StructureImplementationGroup = {
  id: StructureGroupId;
  before: StructureFact<StructureImplementationSide>;
  after: StructureFact<StructureImplementationSide>;
};

/** 文件容器的路径来自真实 source；可读但未成功提取的文件也可导航。 */
export type StructureFileNode = {
  kind: "file";
  id: StructureNodeId;
  sourceId: StructureSourceId;
  containedChanges: StructureContainedChanges;
};

/** 外部目标有包身份但没有伪造的本地定义；本地 impl 来源属于对应分组。 */
export type StructureExternalNode = {
  kind: "external";
  id: StructureNodeId;
  name: string;
  packageName: string;
  definitionUnavailable: "external-source-not-indexed";
};

/** 图只消费 owned 节点，不包含解析器或语言服务句柄。 */
export type StructureNode = StructureDeclaration | StructureFileNode | StructureExternalNode;

/** 关系类型与变化标签是两个独立维度；归属由双侧 parent/group 表达。 */
export type StructureRelationshipKind = "calls" | "extends" | "implements" | "type-reference" | "imports";

/** 候选边默认隐藏，不能当作已确认的关系变化。 */
export type StructureCertainty = "resolved" | "candidate";

/** 关系缺侧未能确认时必须保持 indeterminate。 */
export type StructureRelationshipChange = "added" | "deleted" | "retained" | "indeterminate";

/** 匿名回调单独保留执行来源，不能声称是词法父声明的直接调用。 */
export type StructureExecution =
  | { kind: "declaration-body" }
  | { kind: "anonymous-callback"; callbackRange: StructureRange }
  | { kind: "file" };

/** 内置分析可展示的绑定依据，不包含 parser 查询或 AST 引用。 */
export type StructureEvidenceKind =
  | "lexical-binding" | "explicit-import" | "qualified-name"
  | "known-receiver" | "explicit-type" | "module-path";

/** 同一发生位置可具有多个独立绑定依据。 */
export type StructureEvidence = { kind: StructureEvidenceKind; detail: string };

/** 聚合边仍保留每个原始发生位置及执行来源。 */
export type StructureOccurrence = {
  location: StructureLocation;
  execution: StructureExecution;
  evidence: StructureEvidence[];
};

/** 按匹配端点及关系种类汇总；仅行号或调用点数量变化不会制造边增删。 */
export type StructureRelationship = {
  id: StructureRelationId;
  fromId: StructureNodeId;
  toId: StructureNodeId;
  kind: StructureRelationshipKind;
  certainty: StructureCertainty;
  before: StructureFact<StructureOccurrence[]>;
  after: StructureFact<StructureOccurrence[]>;
  change: StructureRelationshipChange;
};

/** 未解析的位置可独立查看，候选引用不得伪装成确定目标。 */
export type StructureUnresolvedRelationship = {
  side: StructureSide;
  fromId: StructureNodeId;
  kind: StructureRelationshipKind;
  writtenTarget: string;
  occurrence: StructureOccurrence;
  candidateTargetIds: StructureNodeId[];
  diagnostics: StructureDiagnostic[];
};

/** 展示覆盖程度；完整语法覆盖不代表完整关系解析。 */
export type StructureCoverageStatus = "complete" | "partial" | "not-started";

/** 已发现文件逐项展示提取与关系覆盖，不向前端传递内部 absence proof。 */
export type StructureFileCoverage = {
  side: StructureSide;
  path: string;
  extraction: StructureCoverageStatus;
  relationships: StructureCoverageStatus;
  diagnostics: StructureDiagnostic[];
};

/** 仓库发现失败与逐文件分析限制分别报告。 */
export type StructureCoverage = {
  discovery: { before: StructureCoverageStatus; after: StructureCoverageStatus };
  files: StructureFileCoverage[];
  diagnostics: StructureDiagnostic[];
};

/** 任务结束方式不代替覆盖状态；completed 仍可能只有部分语义结论。 */
export type StructureAnalysisStatus = "running" | "completed" | "cancelled" | "failed";

/** 声明与关系变化分别去重计数，complete 为全部所请求分析的覆盖结论。 */
export type StructureSummary = {
  changedDeclarations: number;
  relationshipChanges: number;
  incompleteDeclarations: number;
  unresolvedOccurrences: number;
  complete: boolean;
};

/** 初始焦点包括变化、一跳双侧上下文与必要容器，隐藏边不移除变化入口。 */
export type StructureInitialFocus = { nodeIds: StructureNodeId[]; relationshipIds: StructureRelationId[] };

/** 固定 Snapshot 的完整 owned 检查点，引用集合必须自洽，匹配 ID 不再重排。 */
export type StructureResult = ReviewIdentity & {
  status: StructureAnalysisStatus;
  nodes: StructureNode[];
  implementationGroups: StructureImplementationGroup[];
  relationships: StructureRelationship[];
  unresolvedRelationships: StructureUnresolvedRelationship[];
  sources: StructurePeekSource[];
  coverage: StructureCoverage;
  summary: StructureSummary;
  initialFocus: StructureInitialFocus;
};

/** 单次后台分析的阶段，仅用于进度，不要求前端编排执行。 */
export type StructurePhase = "discovery" | "extraction" | "matching" | "relationships";

/** 计数均按文件；发现未结束时 totalFiles 为 null。 */
export type StructureProgress = {
  phase: StructurePhase;
  side: StructureSide | null;
  processedFiles: number;
  totalFiles: number | null;
};

/** 同一请求按序发送；旧回写由闭包 generation、Snapshot、abort/settled guard 拦截。 */
export type StructureEvent =
  | { kind: "progress"; progress: StructureProgress }
  | { kind: "checkpoint"; result: StructureResult };

/** 真实文件对的一侧；absent 必须由 Snapshot 来源确认，读取失败不改变描述符。 */
export type StructureSourceSide =
  | { state: "present"; path: string; revision: string }
  | { state: "absent" };

/** 只有实际 File Change 才能显式跳转到 File Changes。 */
export type StructureSourceClassification =
  | { kind: "file-change"; fileChange: FileChangeSummary }
  | { kind: "unchanged"; path: string };

/** 文件 rename 沿用 oldPath/path；结构从 A 移到 B 必须使用各自真实文件对。 */
export type StructurePeekSource = {
  id: StructureSourceId;
  before: StructureSourceSide;
  after: StructureSourceSide;
  classification: StructureSourceClassification;
};

/** Peek 身份独立于 File Changes 选择和分析运行身份。 */
export type StructurePeekIdentity = ReviewIdentity & { sourceId: StructureSourceId };

/** 未能阅读源码的原因，与 Git 确认缺侧不同。 */
export type StructureSourceUnavailableReason =
  | "git-object-unavailable" | "source-unreadable" | "binary" | "symlink" | "submodule"
  | "cancelled" | "resource-limit";

/** available 始终是完整原始文本；空文本、缺侧、不可用严格区分。 */
export type StructureSourceContent =
  | { state: "available"; path: string; revision: string; objectId: string; text: string; size: number }
  | { state: "absent"; reason: "git-tree-absence" }
  | { state: "unavailable"; reason: StructureSourceUnavailableReason; message: string; retryable: boolean };

/** 一侧读取失败仍返回另一侧内容，不能把不可用内容替换为空字符串。 */
export type StructurePeekContent = StructurePeekIdentity & {
  source: StructurePeekSource;
  before: StructureSourceContent;
  after: StructureSourceContent;
};

/** 以固定 Snapshot 的实际侧和路径读源码，无需存在已完成的分析结果。 */
export type StructureSourceRequest = { side: StructureSide; path: string };
