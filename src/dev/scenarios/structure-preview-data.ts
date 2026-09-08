import { createStructureMultiSourceFixture } from "./structure-multi-source-fixture";
import type { StructureDeclaration, StructureDeclarationSide, StructureResult } from "@/review/structure-types";
import { structureContractPreview } from "./structure-fixture";

/** 通用预览场景，所有数据均为手工样本，不触发生产分析。 */
export type CodeStructureScenario = "default" | "empty" | "loading" | "partial" | "cancelled" | "error" | "relationship-only" | "large" | "long-label" | "relationship-controls" | "missing-side" | "long-path" | "multiple-impls" | "multiple-occurrences";

/** 为拓扑和浏览器验收生成包含原生 struct、impl、方法与关系的稳定样本。 */
export function createStructurePreviewResult(scenario: CodeStructureScenario = "default"): StructureResult {
  if (scenario === "long-path" || scenario === "multiple-impls" || scenario === "multiple-occurrences") return createStructureMultiSourceFixture(scenario).result;
  const result = structuredClone(structureContractPreview);
  if (scenario === "empty") return { ...result, nodes: [], relationships: [], unresolvedRelationships: [], sources: [], summary: { changedDeclarations: 0, relationshipChanges: 0, incompleteDeclarations: 0, unresolvedOccurrences: 0, complete: true }, initialFocus: { nodeIds: [], relationshipIds: [] } };
  const sourceId = result.sources[0].id;
  /** 声明侧只提供示例原文位置，不使用扩展名推导语言语义。 */
  const side = (name: string, parentId: string, category: StructureDeclarationSide["nativeKind"]["category"], group: string | null = null): StructureDeclarationSide => ({ name, qualifiedName: `review::${name}`, signature: category === "struct" ? `pub struct ${name}` : `pub fn ${name}(&self) -> Result<(), ReviewError>`, nativeKind: { language: "rust", category, name: category === "struct" ? "struct" : "method" }, parentId, implementationGroupId: group, locations: [{ sourceId, side: "after", role: "declaration", range: { startLine: 12, startColumn: 1, endLine: 16, endColumn: 2 } }] });
  const ownerValue = side(scenario === "long-label" ? "ReviewLifecycleWithAnExtremelyLongButRealisticallyQualifiedResponsibilityName" : "ReviewLifecycle", "file:welcome", "struct");
  const owner: StructureDeclaration = { kind: "declaration", id: "decl:owner", before: { state: "present", value: { ...ownerValue, locations: ownerValue.locations.map((location) => ({ ...location, side: "before" })) } }, after: { state: "present", value: ownerValue }, changes: [], comparison: "complete", diagnostics: [], displayParentId: "file:welcome", containedChanges: { before: 1, after: 1, union: 1 } };
  const method = side("refresh", owner.id, "method", "impl:lifecycle");
  const unchanged = side("current_snapshot", owner.id, "method", "impl:lifecycle");
  result.nodes.push(owner,
    { ...owner, id: "decl:refresh", before: { state: "present", value: { ...method, locations: method.locations.map((location) => ({ ...location, side: "before" })) } }, after: { state: "present", value: method }, changes: ["modified", "renamed"], displayParentId: owner.id, containedChanges: { before: 0, after: 0, union: 0 } },
    { ...owner, id: "decl:current", before: { state: "present", value: unchanged }, after: { state: "present", value: unchanged }, changes: [], displayParentId: owner.id, containedChanges: { before: 0, after: 0, union: 0 } });
  const implValue = { ownerId: owner.id, traitName: "ReviewAccess", traitTargetId: null, signature: "impl ReviewAccess for ReviewLifecycle", locations: [{ ...method.locations[0], role: "implementation" as const }] };
  result.implementationGroups.push({ id: "impl:lifecycle", before: { state: "present", value: implValue }, after: { state: "present", value: implValue } });
  const occurrence = { location: { ...method.locations[0], role: "occurrence" as const }, execution: { kind: "anonymous-callback" as const, callbackRange: method.locations[0].range }, evidence: [{ kind: "explicit-import" as const, detail: "Preview: explicit imported binding" }] };
  result.relationships.push({ id: "relation:refresh:welcome", fromId: "decl:refresh", toId: "decl:welcome", kind: "calls", certainty: "resolved", before: { state: "absent" }, after: { state: "present", value: [occurrence] }, change: "added" },
    { id: "relation:owner:helper", fromId: owner.id, toId: "decl:helper", kind: "implements", certainty: "resolved", before: { state: "present", value: [{ ...occurrence, location: { ...occurrence.location, side: "before" } }] }, after: { state: "absent" }, change: "deleted" });
  result.initialFocus.nodeIds.push(owner.id, "decl:refresh");
  result.initialFocus.relationshipIds.push("relation:refresh:welcome", "relation:owner:helper");
  result.summary.changedDeclarations = 3;
  result.summary.relationshipChanges = 2;
  if (scenario === "relationship-only") {
    result.nodes = result.nodes.map((node) => node.kind === "declaration" ? { ...node, changes: [], containedChanges: { before: 0, after: 0, union: 0 } } : node);
    result.relationships = result.relationships.filter((edge) => edge.id === "relation:refresh:welcome").map((edge) => ({ ...edge, kind: "type-reference" }));
    result.initialFocus.nodeIds = ["decl:refresh", "decl:welcome"];
    result.summary.changedDeclarations = 0;
    result.summary.relationshipChanges = 1;
  }
  if (scenario === "large") {
    for (let index = 0; index < 180; index++) {
      const value = side(`indexed_operation_${String(index).padStart(3, "0")}`, "file:welcome", "method");
      result.nodes.push({ ...owner, id: `decl:bulk:${index}`, before: { state: "absent" }, after: { state: "present", value }, changes: ["added"], displayParentId: "file:welcome", containedChanges: { before: 0, after: 0, union: 0 } });
      result.initialFocus.nodeIds.push(`decl:bulk:${index}`);
    }
    result.summary.changedDeclarations += 180;
  }
  if (scenario === "relationship-controls") {
    const dependencySourceId = "preview:dependency.ts";
    result.sources.push({ id: dependencySourceId,
      before: { state: "present", path: "dependency.ts", revision: "preview-before" },
      after: { state: "present", path: "dependency.ts", revision: "preview-after" },
      classification: { kind: "unchanged", path: "dependency.ts" } });
    result.nodes.push({ kind: "file", id: "file:dependency", sourceId: dependencySourceId, containedChanges: { before: 0, after: 0, union: 0 } });
    const dependencyValue: StructureDeclarationSide = { ...side("sendOutput", "file:dependency", "function"),
      signature: "export function sendOutput(text: string): void", nativeKind: { language: "typescript", category: "function", name: "function" },
      locations: [{ sourceId: dependencySourceId, side: "after", role: "declaration", range: { startLine: 1, startColumn: 1, endLine: 3, endColumn: 2 } }] };
    result.nodes.push({ ...owner, id: "decl:dependency", displayParentId: "file:dependency", containedChanges: { before: 0, after: 0, union: 0 },
      before: { state: "present", value: { ...dependencyValue, locations: dependencyValue.locations.map((location) => ({ ...location, side: "before" })) } }, after: { state: "present", value: dependencyValue } });
    const beforeOccurrence = { ...occurrence, location: { ...occurrence.location, side: "before" as const } };
    result.relationships.push(
      { id: "relation:welcome:dependency:retained", fromId: "decl:welcome", toId: "decl:dependency", kind: "calls", certainty: "resolved", change: "retained", before: { state: "present", value: [beforeOccurrence] }, after: { state: "present", value: [occurrence] } },
      { id: "relation:refresh:dependency:candidate", fromId: "decl:refresh", toId: "decl:dependency", kind: "calls", certainty: "candidate", change: "indeterminate",
        before: { state: "unknown", diagnostics: [{ reason: "ambiguous-target", message: "Preview: multiple possible receiver bindings.", side: "before", path: "welcome.ts", range: occurrence.location.range, retryable: false }] },
        after: { state: "present", value: [{ ...occurrence, evidence: [{ kind: "qualified-name", detail: "Preview: candidate from the written receiver name; target is not confirmed." }] }] } },
      { id: "relation:welcome:dependency:imports", fromId: "file:welcome", toId: "file:dependency", kind: "imports", certainty: "resolved", change: "added", before: { state: "absent" }, after: { state: "present", value: [{ ...occurrence, execution: { kind: "file" }, evidence: [{ kind: "module-path", detail: "Preview: explicit ./dependency module path." }] }] } },
    );
    result.initialFocus.nodeIds.push("file:dependency", "decl:dependency");
    result.initialFocus.relationshipIds.push("relation:welcome:dependency:imports");
    result.summary.relationshipChanges += 1;
    result.coverage.files.push(...(["before", "after"] as const).map((side) => ({ side, path: "dependency.ts", extraction: "complete" as const, relationships: "complete" as const, diagnostics: [] })));
  }
  if (scenario === "missing-side") {
    const added = { ...side("newEntry", "file:added", "function"), nativeKind: { language: "typescript" as const, category: "function" as const, name: "function" as const }, signature: "export function newEntry() {}",
      locations: [{ sourceId: "preview:added.ts", side: "after" as const, role: "declaration" as const, range: { startLine: 1, startColumn: 1, endLine: 1, endColumn: 29 } }] };
    const deleted = { ...added, name: "removedEntry", qualifiedName: "removedEntry", signature: "export function removedEntry() {}", parentId: "file:deleted",
      locations: [{ ...added.locations[0], sourceId: "preview:deleted.ts", side: "before" as const }] };
    result.sources = [
      { id: "preview:added.ts", before: { state: "absent" }, after: { state: "present", path: "added.ts", revision: "preview-after" }, classification: { kind: "file-change", fileChange: { path: "added.ts", oldPath: null, status: "added" } } },
      { id: "preview:deleted.ts", before: { state: "present", path: "deleted.ts", revision: "preview-before" }, after: { state: "absent" }, classification: { kind: "file-change", fileChange: { path: "deleted.ts", oldPath: null, status: "deleted" } } },
    ];
    result.nodes = [
      { kind: "file", id: "file:added", sourceId: "preview:added.ts", containedChanges: { before: 0, after: 1, union: 1 } },
      { ...owner, id: "decl:added", displayParentId: "file:added", before: { state: "absent" }, after: { state: "present", value: added }, changes: ["added"], containedChanges: { before: 0, after: 0, union: 0 } },
      { kind: "file", id: "file:deleted", sourceId: "preview:deleted.ts", containedChanges: { before: 1, after: 0, union: 1 } },
      { ...owner, id: "decl:deleted", displayParentId: "file:deleted", before: { state: "present", value: deleted }, after: { state: "absent" }, changes: ["deleted"], containedChanges: { before: 0, after: 0, union: 0 } },
    ];
    result.implementationGroups = [];
    result.relationships = [];
    result.initialFocus = { nodeIds: result.nodes.map((node) => node.id), relationshipIds: [] };
    result.summary = { changedDeclarations: 2, relationshipChanges: 0, incompleteDeclarations: 0, unresolvedOccurrences: 0, complete: true };
    result.coverage = { discovery: { before: "complete", after: "complete" }, diagnostics: [], files: [
      { side: "before", path: "deleted.ts", extraction: "complete", relationships: "complete", diagnostics: [] },
      { side: "after", path: "added.ts", extraction: "complete", relationships: "complete", diagnostics: [] },
    ] };
  }
  if (scenario === "cancelled") {
    result.status = "cancelled";
    result.coverage.discovery.after = "partial";
    result.coverage.diagnostics.push({ reason: "cancelled", message: "Preview: analysis stopped before the remaining files were discovered.", side: "after", path: null, range: null, retryable: true });
  }
  if (scenario !== "partial" && scenario !== "cancelled" && scenario !== "relationship-controls") { result.summary.complete = true; result.unresolvedRelationships = []; result.summary.unresolvedOccurrences = 0; }
  else result.summary.complete = false;
  return result;
}
