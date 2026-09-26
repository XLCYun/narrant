import { MarkerType } from "@xyflow/react";
import type { ObjectNode, StructureEdge, StructureLayout, StructureSelection } from "./graph-presentation";

/** 区分可承载子对象的容器与仅用于排版的成员表。 */
function isContainer(node: ObjectNode): boolean {
  return !node.data.table && (node.data.directory !== undefined
    || node.data.category === "container" || node.data.category === "type"
    || node.data.group || node.data.rows.some((row) => row.objectId !== undefined));
}

/** 展示端点只引用当前可见卡片及其端口，不改变原始 Code Graph 的关系。 */
type Endpoint = { card: string; handle: string; name: string };

/** 详细模式保留成员连线；简洁模式把容器的跨边界关系提升到同层级容器并按方向去重。 */
export function selectStructureEdges(
  expandedLayout: StructureLayout,
  layout: StructureLayout,
  selection: StructureSelection | undefined,
  detail: "compact" | "detailed",
): StructureEdge[] {
  if (!selection) return [];
  const selectedCard = expandedLayout.nodes.find(({ data }) => selection.kind === "object"
    ? data.objectId === selection.id : data.directory?.path === selection.path);
  const objects = new Set<string>(selection.kind === "object" ? [selection.id] : []);
  if (selectedCard) {
    const cards = new Set([selectedCard.id]);
    // 全展开布局保证父卡片在前，包含独立子卡片以及归入类卡片的字段、方法和 impl 行。
    for (const node of expandedLayout.nodes) {
      if (!cards.has(node.id) && (!node.parentId || !cards.has(node.parentId))) continue;
      cards.add(node.id);
      if (node.data.objectId !== undefined) objects.add(node.data.objectId);
      for (const row of node.data.rows) if (row.objectId !== undefined) objects.add(row.objectId);
    }
  }
  const related = layout.edges.filter(({ data }) => data !== undefined
    && (objects.has(data.sourceObjectId) || objects.has(data.targetObjectId)));
  // 直接选中函数或字段仍使用原成员端口；详细模式也保留容器内部的成员关系。
  if (detail === "detailed" || !selectedCard || !isContainer(selectedCard)) return related;

  const visible = new Map(layout.nodes.map((node) => [node.id, node]));
  const representatives = new Map<string, ObjectNode>();
  const containerPaths = new Map<string, string[]>();
  const objectPaths = new Map<string, string[]>();
  for (const node of expandedLayout.nodes) {
    const parentPath = node.parentId ? containerPaths.get(node.parentId)! : [];
    const path = isContainer(node) ? [...parentPath, node.id] : parentPath;
    containerPaths.set(node.id, path);
    if (node.data.objectId !== undefined) objectPaths.set(node.data.objectId, path);
    for (const row of node.data.rows) if (row.objectId !== undefined) objectPaths.set(row.objectId, path);
    const representative = visible.get(node.id) ?? (node.parentId ? representatives.get(node.parentId) : undefined);
    if (representative) representatives.set(node.id, representative);
  }
  const depth = containerPaths.get(selectedCard.id)!.length;

  /** 容器已折叠时使用可见祖先标题；无所属容器的外部函数保留原端口。 */
  const endpoint = (edge: StructureEdge, side: "source" | "target"): Endpoint => {
    const objectId = side === "source" ? edge.data!.sourceObjectId : edge.data!.targetObjectId;
    const path = objectPaths.get(objectId)!;
    const container = objects.has(objectId) ? selectedCard.id : path[Math.min(depth, path.length) - 1];
    if (container) {
      const node = representatives.get(container)!;
      const port = node.data.ports.find((item) => item.side === side && item.objectId === node.data.objectId)!;
      return { card: node.id, handle: port.id, name: node.data.filePath ?? node.data.directory?.path ?? node.data.label };
    }
    return {
      card: edge[side], handle: side === "source" ? edge.sourceHandle! : edge.targetHandle!,
      name: side === "source" ? edge.data!.sourceName : edge.data!.targetName,
    };
  };

  const groups = new Map<string, { source: Endpoint; target: Endpoint; edges: StructureEdge[] }>();
  for (const edge of related) {
    if (objects.has(edge.data!.sourceObjectId) && objects.has(edge.data!.targetObjectId)) continue;
    const source = endpoint(edge, "source"), target = endpoint(edge, "target");
    // 汇总或折叠后的端点若互为祖先与后代，不绘制指回所属容器的连线。
    if (source.card === target.card
      || containerPaths.get(source.card)!.includes(target.card)
      || containerPaths.get(target.card)!.includes(source.card)) continue;
    // 用有向端口对去重；两个无容器的外部函数即使共用成员表，也不应合并。
    const key = JSON.stringify([source.handle, target.handle]);
    const group = groups.get(key);
    if (group) group.edges.push(edge); else groups.set(key, { source, target, edges: [edge] });
  }
  return [...groups.values()].map(({ source, target, edges }) => {
    const first = edges[0];
    const change = edges.every((edge) => edge.data!.change === first.data!.change) ? first.data!.change : "mixed";
    return {
      ...first, id: `summary-${source.handle}-${target.handle}`,
      source: source.card, sourceHandle: source.handle, target: target.card, targetHandle: target.handle,
      label: undefined,
      data: { ...first.data!, change, summary: { count: edges.length, sourceName: source.name, targetName: target.name },
        route: undefined, collapsedSource: undefined, collapsedTarget: undefined },
      // 混合新增、删除和未变化关系时用关系变化色，避免以第一条关系误代表整组。
      ...(change === "mixed" ? {
        markerEnd: { type: MarkerType.ArrowClosed, color: "#9333ea" },
        style: { ...first.style, stroke: "#9333ea", strokeWidth: 1.8, strokeDasharray: undefined },
      } : {}),
    };
  });
}
