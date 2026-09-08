import type { StructureGraphNavigation } from "@/review/structure-graph-types";
import type { StructureDeclaration, StructureDeclarationSide, StructureLocation, StructureOccurrence, StructurePeekContent, StructureResult, StructureSide } from "@/review/structure-types";

/** 图与 Peek 共用的复杂来源场景，全部源码都是独立手工样本。 */
export type StructureMultiSourceScenario = "long-path" | "multiple-impls" | "multiple-occurrences";

/** 构造一个类型、两个独立 Rust impl 文件和四个双侧调用点，不拼接虚拟文件。 */
export function createStructureMultiSourceFixture(scenario: StructureMultiSourceScenario): { result: StructureResult; contents: StructurePeekContent[]; intent: StructureGraphNavigation } {
  const identity = { reviewId: "preview-review", snapshotId: "preview-snapshot" };
  const paths = [
    "src/review/lifecycle/representative_sources/long_module_names_for_responsive_source_navigation/review_lifecycle_with_multiple_implementation_sources.rs",
    "src/review/lifecycle/representative_sources/long_module_names_for_responsive_source_navigation/implementations/refresh_review_access.rs",
    "src/review/lifecycle/representative_sources/long_module_names_for_responsive_source_navigation/implementations/persist_snapshot_state.rs",
  ];
  const texts = [
    ['pub struct ReviewLifecycle;', '', 'pub trait ReviewAccess { fn refresh(&self); }', 'pub fn record(message: &str) { println!("{}", message); }', ''].join("\n"),
    ['use super::{ReviewAccess, ReviewLifecycle, record};', '', 'impl ReviewAccess for ReviewLifecycle {', '    fn refresh(&self) {', '        record("begin");', '        record("after");', '    }', '}', ''].join("\n"),
    ['use super::{ReviewLifecycle, record};', '', 'impl ReviewLifecycle {', '    pub fn persist(&self) {', '        record("saved");', '    }', '}', ''].join("\n"),
  ];
  const contents: StructurePeekContent[] = paths.map((path, index) => {
    const sourceId = `preview:rust-source:${index}`;
    const source = { id: sourceId,
      before: { state: "present" as const, path, revision: "preview-before" }, after: { state: "present" as const, path, revision: "preview-after" },
      classification: index === 1 ? { kind: "file-change" as const, fileChange: { path, oldPath: null, status: "modified" as const } } : { kind: "unchanged" as const, path } };
    const beforeText = index === 1 ? texts[index].replace('record("after")', 'record("before")') : texts[index];
    return { ...identity, sourceId, source,
      before: { state: "available", path, revision: "preview-before", objectId: `preview-before-${index}`, text: beforeText, size: new TextEncoder().encode(beforeText).length },
      after: { state: "available", path, revision: "preview-after", objectId: `preview-after-${index}`, text: texts[index], size: new TextEncoder().encode(texts[index]).length } };
  });
  /** 行范围直接对应上方完整原文，各侧使用自己的源身份。 */
  const location = (source: number, side: StructureSide, line: number, endLine = line, role: StructureLocation["role"] = "declaration"): StructureLocation => {
    const content = contents[source][side];
    const text = content.state === "available" ? content.text : "";
    return { sourceId: contents[source].sourceId, side, role, range: { startLine: line, startColumn: 1, endLine, endColumn: (text.split("\n")[endLine - 1]?.length ?? 0) + 1 } };
  };
  /** 声明身份、原生种类与归属由样本明确给定，不让 UI 推断语言。 */
  const declaration = (id: string, name: string, category: StructureDeclarationSide["nativeKind"]["category"], signature: string, source: number, line: number, endLine: number, parentId = "file:rust:0", groupId: string | null = null, modified = false): StructureDeclaration => {
    const value: StructureDeclarationSide = { name, qualifiedName: `review::${name}`, signature, nativeKind: { language: "rust", category, name: category === "struct" ? "struct" : category === "trait" ? "trait" : category === "method" ? "method" : "function" }, parentId, implementationGroupId: groupId, locations: [location(source, "after", line, endLine)] };
    return { kind: "declaration", id, displayParentId: parentId, before: { state: "present", value: { ...value, locations: [location(source, "before", line, endLine)] } }, after: { state: "present", value }, changes: modified ? ["modified"] : [], comparison: "complete", diagnostics: [], containedChanges: { before: 0, after: 0, union: 0 } };
  };
  const owner = declaration("decl:rust:owner", "ReviewLifecycle", "struct", "pub struct ReviewLifecycle", 0, 1, 1);
  owner.containedChanges = { before: 1, after: 1, union: 1 };
  /** 同一 impl 的两侧来源保持同文件，两个 impl 始终独立。 */
  const implementation = (id: string, source: number, traitName: string | null, endLine: number) => {
    const value = { ownerId: owner.id, traitName, traitTargetId: traitName ? "decl:rust:trait" : null, signature: traitName ? "impl ReviewAccess for ReviewLifecycle" : "impl ReviewLifecycle", locations: [location(source, "after", 3, endLine, "implementation")] };
    return { id, before: { state: "present" as const, value: { ...value, locations: [location(source, "before", 3, endLine, "implementation")] } }, after: { state: "present" as const, value } };
  };
  /** 两个不同语句保留各自发生位置；仅参数内容变化不制造关系增删。 */
  const calls = (side: StructureSide): StructureOccurrence[] => [5, 6].map((line) => ({ location: location(1, side, line, line, "occurrence"), execution: { kind: "declaration-body" }, evidence: [{ kind: "explicit-import", detail: "手工样本：record 由 super 显式导入。" }] }));
  const result: StructureResult = { ...identity, status: "completed", sources: contents.map((content) => content.source),
    nodes: [
      ...contents.map((content, index) => ({ kind: "file" as const, id: `file:rust:${index}`, sourceId: content.sourceId, containedChanges: { before: index < 2 ? 1 : 0, after: index < 2 ? 1 : 0, union: index < 2 ? 1 : 0 } })),
      owner, declaration("decl:rust:trait", "ReviewAccess", "trait", "pub trait ReviewAccess", 0, 3, 3),
      declaration("decl:rust:record", "record", "function", "pub fn record(message: &str)", 0, 4, 4),
      declaration("decl:rust:refresh", "refresh", "method", "fn refresh(&self)", 1, 4, 7, owner.id, "impl:rust:access", true),
      declaration("decl:rust:persist", "persist", "method", "pub fn persist(&self)", 2, 4, 6, owner.id, "impl:rust:persistence"),
    ],
    implementationGroups: [implementation("impl:rust:access", 1, "ReviewAccess", 8), implementation("impl:rust:persistence", 2, null, 7)],
    relationships: [
      { id: "relation:rust:refresh:record", fromId: "decl:rust:refresh", toId: "decl:rust:record", kind: "calls", certainty: "resolved", change: "retained", before: { state: "present", value: calls("before") }, after: { state: "present", value: calls("after") } },
      { id: "relation:rust:persist:record", fromId: "decl:rust:persist", toId: "decl:rust:record", kind: "calls", certainty: "resolved", change: "retained",
        before: { state: "present", value: [{ ...calls("before")[0], location: location(2, "before", 5, 5, "occurrence") }] },
        after: { state: "present", value: [{ ...calls("after")[0], location: location(2, "after", 5, 5, "occurrence") }] } },
      { id: "relation:rust:owner:access", fromId: owner.id, toId: "decl:rust:trait", kind: "implements", certainty: "resolved", change: "retained",
        before: { state: "present", value: [{ location: location(1, "before", 3, 3, "occurrence"), execution: { kind: "file" }, evidence: [{ kind: "explicit-type", detail: "手工样本：impl ReviewAccess for ReviewLifecycle。" }] }] },
        after: { state: "present", value: [{ location: location(1, "after", 3, 3, "occurrence"), execution: { kind: "file" }, evidence: [{ kind: "explicit-type", detail: "手工样本：impl ReviewAccess for ReviewLifecycle。" }] }] } },
    ], unresolvedRelationships: [],
    initialFocus: { nodeIds: ["file:rust:0", owner.id, "decl:rust:trait", "decl:rust:record", "decl:rust:refresh", "decl:rust:persist"], relationshipIds: ["relation:rust:refresh:record"] },
    coverage: { discovery: { before: "complete", after: "complete" }, diagnostics: [], files: paths.flatMap((path) => (["before", "after"] as const).map((side) => ({ side, path, extraction: "complete", relationships: "complete", diagnostics: [] }))) },
    summary: { changedDeclarations: 1, relationshipChanges: 0, incompleteDeclarations: 0, unresolvedOccurrences: 0, complete: true },
  };
  const intent: StructureGraphNavigation = scenario === "multiple-occurrences" ? { kind: "relationship", relationshipId: "relation:rust:refresh:record" } : { kind: "node", nodeId: scenario === "long-path" ? "file:rust:0" : owner.id };
  return { result, contents, intent };
}
