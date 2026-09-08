import type { ReviewIdentity } from "@/review/types";
import type { StructureGraphNavigation } from "@/review/structure-graph-types";
import type { StructureLocation, StructurePeekContent, StructurePeekSource, StructureResult } from "@/review/structure-types";

/** 一次图导航的全部真实来源；默认来源始终位于首项。 */
export type StructurePeekSelection = ReviewIdentity & {
  title: string;
  locations: StructureLocation[];
  sources: StructurePeekSource[];
  targetDefinition: Extract<StructureGraphNavigation, { kind: "target-definition" }> | null;
  unavailableReason: "external-source-not-indexed" | "no-location" | null;
};

/** 读取状态只属于 Peek；requestId 是前端重复定位序号，不是后端协议。 */
export type StructurePeekState = {
  selection: StructurePeekSelection;
  locationIndex: number;
  requestId: number;
  load:
    | { status: "idle" | "loading" }
    | { status: "failed"; message: string; retryable: boolean }
    | { status: "ready"; content: StructurePeekContent };
};

/** 将图意图解析成真实源码位置，关系发生位置与目标定义永不互换。 */
export function resolveStructurePeekNavigation(result: StructureResult, intent: StructureGraphNavigation): StructurePeekSelection | null {
  const selection: StructurePeekSelection = {
    reviewId: result.reviewId, snapshotId: result.snapshotId, title: "", locations: [], sources: result.sources,
    targetDefinition: null, unavailableReason: null,
  };
  if (intent.kind === "relationship") {
    const edge = result.relationships.find((item) => item.id === intent.relationshipId);
    if (!edge) return null;
    selection.title = edge.kind;
    for (const side of edge.change === "deleted" ? [edge.before, edge.after] : [edge.after, edge.before]) {
      if (side.state === "present") selection.locations.push(...side.value.map((item) => item.location));
    }
    selection.targetDefinition = { kind: "target-definition", nodeId: edge.toId, relationshipId: edge.id };
  } else if (intent.kind === "location") {
    selection.locations = [intent.location];
  } else if (intent.kind === "implementation") {
    const group = result.implementationGroups.find((item) => item.id === intent.groupId);
    if (!group) return null;
    for (const side of [group.after, group.before]) {
      if (side.state === "present") {
        selection.title ||= side.value.signature;
        selection.locations.push(...side.value.locations);
      }
    }
  } else {
    const node = result.nodes.find((item) => item.id === intent.nodeId);
    if (!node) return null;
    if (node.kind === "external") {
      selection.title = node.name;
      selection.unavailableReason = node.definitionUnavailable;
    } else if (node.kind === "file") {
      const source = result.sources.find((item) => item.id === node.sourceId);
      if (!source) return null;
      for (const side of ["after", "before"] as const) {
        if (source[side].state !== "present") continue;
        selection.title ||= source[side].path;
        selection.locations.push({ sourceId: source.id, side, role: "declaration", range: { startLine: 1, startColumn: 1, endLine: 1, endColumn: 1 } });
      }
    } else {
      for (const side of node.changes.includes("deleted") ? [node.before, node.after] : [node.after, node.before]) {
        if (side.state === "present") {
          selection.title ||= side.value.name;
          selection.locations.push(...side.value.locations);
        }
      }
      // 类型声明优先于所有 impl；成员只拥有自己的位置，不收集父类型来源。
      for (const group of result.implementationGroups) {
        for (const side of [group.after, group.before]) {
          if (side.state === "present" && side.value.ownerId === node.id) selection.locations.push(...side.value.locations);
        }
      }
    }
  }
  // 只接受结果拥有的真实文件侧；未知来源不能变成可读取的伪路径。
  selection.locations = selection.locations.filter((location, index, all) =>
    result.sources.some((source) => source.id === location.sourceId && source[location.side].state === "present")
    && all.findIndex((item) => JSON.stringify(item) === JSON.stringify(location)) === index);
  if (!selection.locations.length) selection.unavailableReason ??= "no-location";
  return selection;
}
