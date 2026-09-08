import type {
  StructureDiagnostic,
  StructureLocation,
  StructurePeekContent,
  StructurePeekSource,
  StructureResult,
} from "@/review/structure-types";

/** 纯手工预览数据，revision/objectId 均为示例值，不得提交到业务数据。 */
const source: StructurePeekSource = {
  id: "preview:welcome.ts",
  before: { state: "present", path: "welcome.ts", revision: "preview-before" },
  after: { state: "present", path: "welcome.ts", revision: "preview-after" },
  classification: {
    kind: "file-change",
    fileChange: { path: "welcome.ts", oldPath: null, status: "modified" },
  },
};

/** 参数调用不能确定目标，不据此声称外部 toast 关系在旧侧不存在。 */
const unknownTarget: StructureDiagnostic = {
  reason: "dynamic-target",
  message: "参数 output 的运行时调用目标无法静态确定。",
  side: "before",
  path: "welcome.ts",
  range: { startLine: 2, startColumn: 3, endLine: 2, endColumn: 18 },
  retryable: false,
};

/** 旧侧参数调用的真实位置，仍可打开完整文件。 */
const beforeOccurrence: StructureLocation = {
  sourceId: source.id,
  side: "before",
  range: { startLine: 2, startColumn: 3, endLine: 2, endColumn: 18 },
  role: "occurrence",
};

/** 修改、确认新增、未知旧侧关系和外部目标的最小图预览。 */
export const structureContractPreview: StructureResult = {
  reviewId: "preview-review",
  snapshotId: "preview-snapshot",
  status: "completed",
  nodes: [
    {
      kind: "file", id: "file:welcome", sourceId: source.id,
      containedChanges: { before: 1, after: 2, union: 2 },
    },
    {
      kind: "declaration", id: "decl:welcome",
      before: {
        state: "present",
        value: {
          nativeKind: { language: "typescript", category: "function", name: "function" },
          name: "welcome", qualifiedName: "welcome", signature: "export function welcome(output: (text: string) => void)",
          parentId: "file:welcome", implementationGroupId: null,
          locations: [{ sourceId: source.id, side: "before", role: "declaration",
            range: { startLine: 1, startColumn: 1, endLine: 3, endColumn: 2 } }],
        },
      },
      after: {
        state: "present",
        value: {
          nativeKind: { language: "typescript", category: "function", name: "function" },
          name: "welcome", qualifiedName: "welcome", signature: "export function welcome()",
          parentId: "file:welcome", implementationGroupId: null,
          locations: [{ sourceId: source.id, side: "after", role: "declaration",
            range: { startLine: 2, startColumn: 1, endLine: 4, endColumn: 2 } }],
        },
      },
      changes: ["modified"], comparison: "complete", diagnostics: [],
      displayParentId: "file:welcome", containedChanges: { before: 0, after: 0, union: 0 },
    },
    {
      kind: "declaration", id: "decl:helper", before: { state: "absent" },
      after: {
        state: "present",
        value: {
          nativeKind: { language: "typescript", category: "function", name: "function" },
          name: "helper", qualifiedName: "helper", signature: "export function helper()",
          parentId: "file:welcome", implementationGroupId: null,
          locations: [{ sourceId: source.id, side: "after", role: "declaration",
            range: { startLine: 6, startColumn: 1, endLine: 6, endColumn: 28 } }],
        },
      },
      changes: ["added"], comparison: "complete", diagnostics: [],
      displayParentId: "file:welcome", containedChanges: { before: 0, after: 0, union: 0 },
    },
    {
      kind: "external", id: "external:sonner:toast", name: "toast", packageName: "sonner",
      definitionUnavailable: "external-source-not-indexed",
    },
  ],
  implementationGroups: [],
  relationships: [{
    id: "relation:welcome:toast:calls", fromId: "decl:welcome", toId: "external:sonner:toast",
    kind: "calls", certainty: "resolved", change: "indeterminate",
    before: { state: "unknown", diagnostics: [unknownTarget] },
    after: {
      state: "present",
      value: [{
        location: { sourceId: source.id, side: "after", role: "occurrence",
          range: { startLine: 3, startColumn: 3, endLine: 3, endColumn: 17 } },
        execution: { kind: "declaration-body" },
        evidence: [{ kind: "explicit-import", detail: "从 sonner 显式导入 toast。" }],
      }],
    },
  }],
  unresolvedRelationships: [{
    side: "before", fromId: "decl:welcome", kind: "calls", writtenTarget: "output",
    occurrence: { location: beforeOccurrence, execution: { kind: "declaration-body" }, evidence: [] },
    candidateTargetIds: [], diagnostics: [unknownTarget],
  }],
  sources: [source],
  coverage: {
    discovery: { before: "complete", after: "complete" },
    files: [
      { side: "before", path: "welcome.ts", extraction: "complete", relationships: "partial", diagnostics: [unknownTarget] },
      { side: "after", path: "welcome.ts", extraction: "complete", relationships: "complete", diagnostics: [] },
    ],
    diagnostics: [],
  },
  summary: { changedDeclarations: 2, relationshipChanges: 0, incompleteDeclarations: 0, unresolvedOccurrences: 1, complete: false },
  initialFocus: { nodeIds: ["file:welcome", "decl:welcome", "decl:helper", "external:sonner:toast"], relationshipIds: [] },
};

/** 与图位置一致的完整原始文件对；外部定义不生成任何虚构本地源码。 */
export const structurePeekPreview: StructurePeekContent = {
  reviewId: "preview-review", snapshotId: "preview-snapshot", sourceId: source.id, source,
  before: {
    state: "available", path: "welcome.ts", revision: "preview-before", objectId: "preview-before-blob",
    text: 'export function welcome(output: (text: string) => void) {\n  output("hello");\n}\n',
    size: 79,
  },
  after: {
    state: "available", path: "welcome.ts", revision: "preview-after", objectId: "preview-after-blob",
    text: 'import { toast } from "sonner";\nexport function welcome() {\n  toast("hello");\n}\n\nexport function helper() {}\n',
    size: 109,
  },
};
