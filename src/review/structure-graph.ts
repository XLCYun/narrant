import type { StructureDeclaration, StructureImplementationGroup, StructureNode, StructureRelationship, StructureResult } from "@/review/structure-types";
import type { StructureGraphPresentation } from "@/review/structure-graph-types";

/** 画布布局项，容器必须先于后代输出；坐标相对直接父容器。 */
export type StructureGraphItem = {
  id: string;
  parentId?: string;
  node?: StructureNode;
  implementation?: StructureImplementationGroup;
  label: string;
  signature: string;
  childCount: number;
  expanded: boolean;
  x: number;
  y: number;
  width: number;
  height: number;
};

/** 返回可见拓扑及完整可发现集合；关系变化列表不受连线偏好限制。 */
export type StructureGraph = {
  nodes: StructureGraphItem[];
  relationships: Array<{ relationship: StructureRelationship; source: string; target: string }>;
  relationshipChanges: StructureRelationship[];
  searchResults: StructureNode[];
  eligibleCount: number;
  visibleCount: number;
  pendingCount: number;
};

/** 新侧优先显示原生声明，删除声明回退旧侧，unknown 不伪造事实。 */
export function structureDeclarationValue(node: StructureDeclaration) {
  return node.after.state === "present" ? node.after.value : node.before.state === "present" ? node.before.value : null;
}

/** 将后端身份转为显示名称，文件路径直接使用来源描述符。 */
export function structureNodeLabel(node: StructureNode, result: StructureResult): string {
  if (node.kind === "declaration") return structureDeclarationValue(node)?.name ?? node.id;
  if (node.kind === "external") return `${node.packageName} / ${node.name}`;
  const source = result.sources.find((source) => source.id === node.sourceId);
  return source?.after.state === "present" ? source.after.path : source?.before.state === "present" ? source.before.path : node.id;
}

/** 创建 Snapshot 初次展示状态，变化后代容器展开而未修改成员默认收起。 */
export function createStructureGraphPresentation(result?: StructureResult | null): StructureGraphPresentation {
  const expandedIds = result?.nodes.filter((node) => node.kind === "file" || (node.kind === "declaration" && node.containedChanges.union > 0)).map((node) => node.id) ?? [];
  for (const node of result?.nodes ?? []) {
    if (node.kind !== "declaration" || node.changes.length === 0) continue;
    const group = structureDeclarationValue(node)?.implementationGroupId;
    if (group) expandedIds.push(group);
  }
  // 后端初始关系焦点的端点即使自身未修改，也必须露出到真实所属层级。
  const focusRelationships = new Set(result?.initialFocus.relationshipIds ?? []);
  const parentById = new Map<string, string>();
  for (const node of result?.nodes ?? []) if (node.kind === "declaration") parentById.set(node.id, structureDeclarationValue(node)?.implementationGroupId ?? node.displayParentId);
  for (const group of result?.implementationGroups ?? []) {
    const value = group.after.state === "present" ? group.after.value : group.before.state === "present" ? group.before.value : null;
    if (value) parentById.set(group.id, value.ownerId);
  }
  for (const edge of result?.relationships ?? []) {
    if (!focusRelationships.has(edge.id)) continue;
    for (const endpoint of [edge.fromId, edge.toId]) {
      const visited = new Set<string>();
      let parent = parentById.get(endpoint);
      while (parent && !visited.has(parent)) { visited.add(parent); expandedIds.push(parent); parent = parentById.get(parent); }
    }
  }
  return { viewport: null, viewportMode: "auto", selectedNodeId: null, selectedRelationshipId: null, expandedIds: [...new Set(expandedIds)], revealedNodeIds: [], query: "", displayLimit: 60,
    filters: { category: "all", change: "all", calls: "selected", inheritance: true, implementations: true, typeReferences: false, imports: false, candidates: false, external: true } };
}

/** 按明确双侧关系并集展开一跳；连线关闭不影响上下文探索。 */
export function expandStructureNeighbors(result: StructureResult, state: StructureGraphPresentation, direction: "upstream" | "downstream"): StructureGraphPresentation {
  if (!state.selectedNodeId) return state;
  const ids = new Set(state.revealedNodeIds);
  for (const edge of result.relationships) {
    if (edge.certainty === "candidate" && !state.filters.candidates) continue;
    if (direction === "upstream" && edge.toId === state.selectedNodeId) ids.add(edge.fromId);
    if (direction === "downstream" && edge.fromId === state.selectedNodeId) ids.add(edge.toId);
  }
  const expanded = new Set(state.expandedIds);
  const parents = new Map<string, string>();
  for (const node of result.nodes) if (node.kind === "declaration") parents.set(node.id, structureDeclarationValue(node)?.implementationGroupId ?? node.displayParentId);
  for (const group of result.implementationGroups) {
    const value = group.after.state === "present" ? group.after.value : group.before.state === "present" ? group.before.value : null;
    if (value) parents.set(group.id, value.ownerId);
  }
  for (const id of ids) {
    const visited = new Set<string>();
    let parent = parents.get(id);
    while (parent && !visited.has(parent)) { visited.add(parent); expanded.add(parent); parent = parents.get(parent); }
  }
  return { ...state, expandedIds: [...expanded], revealedNodeIds: [...ids], displayLimit: state.displayLimit + 60 };
}

/** 计算纯展示拓扑：不重新分析语言、声明身份、关系确定性或缺侧。 */
export function buildStructureGraph(result: StructureResult, state: StructureGraphPresentation): StructureGraph {
  const items = new Map<string, StructureGraphItem>();
  for (const node of result.nodes) {
    const declaration = node.kind === "declaration" ? structureDeclarationValue(node) : null;
    items.set(node.id, { id: node.id, node, label: structureNodeLabel(node, result), signature: declaration?.signature ?? "",
      parentId: node.kind === "declaration" ? declaration?.implementationGroupId ?? node.displayParentId : undefined,
      childCount: 0, expanded: state.expandedIds.includes(node.id), x: 0, y: 0, width: 300, height: 116 });
  }
  for (const group of result.implementationGroups) {
    const value = group.after.state === "present" ? group.after.value : group.before.state === "present" ? group.before.value : null;
    if (value) items.set(group.id, { id: group.id, implementation: group, label: value.traitName ? `impl ${value.traitName}` : "impl", signature: value.signature,
      parentId: value.ownerId, childCount: 0, expanded: state.expandedIds.includes(group.id), x: 0, y: 0, width: 300, height: 116 });
  }
  for (const item of items.values()) {
    if (item.parentId && items.has(item.parentId)) items.get(item.parentId)!.childCount++;
    else item.parentId = undefined;
  }
  const query = state.query.trim().toLocaleLowerCase();
  /** 过滤只解释明确的类别与变化标签，unknown 不被展示成未修改。 */
  const matches = (node: StructureNode) => {
    if (node.kind === "external" && !state.filters.external) return false;
    if (state.filters.category !== "all" && (node.kind !== "declaration" || structureDeclarationValue(node)?.nativeKind.category !== state.filters.category)) return false;
    if (state.filters.change !== "all") {
      if (node.kind !== "declaration") return false;
      if (state.filters.change === "context") { if (node.comparison !== "complete" || node.changes.length > 0) return false; }
      else if (state.filters.change === "incomplete") { if (node.comparison !== "incomplete") return false; }
      else if (!node.changes.includes(state.filters.change)) return false;
    }
    return !query || `${structureNodeLabel(node, result)} ${node.kind === "declaration" ? structureDeclarationValue(node)?.qualifiedName ?? "" : ""}`.toLocaleLowerCase().includes(query);
  };
  const searchResults = result.nodes.filter(matches);
  const seeds = new Set([...result.initialFocus.nodeIds, ...state.revealedNodeIds]);
  if (state.selectedNodeId) seeds.add(state.selectedNodeId);
  const filtering = !!query || state.filters.category !== "all" || state.filters.change !== "all";
  const eligible = result.nodes.filter((node) => matches(node) && (filtering || seeds.has(node.id)));
  const selected = [...eligible].sort((a, b) => Number(b.id === state.selectedNodeId) - Number(a.id === state.selectedNodeId)).slice(0, state.displayLimit);
  const eligibleWithParents = new Set<string>();
  for (const node of eligible) {
    const visited = new Set<string>();
    let item = items.get(node.id);
    while (item && !visited.has(item.id)) {
      visited.add(item.id);
      if (item.node) eligibleWithParents.add(item.id);
      item = item.parentId ? items.get(item.parentId) : undefined;
    }
  }
  const visible = new Set<string>();
  const forceOpen = new Set<string>();
  /** 加入必要归属容器，搜索结果自动打开沿途层级。 */
  const include = (id: string, open: boolean) => {
    const visited = new Set<string>();
    let current = items.get(id);
    while (current && !visited.has(current.id)) {
      visited.add(current.id); visible.add(current.id);
      if (current.parentId && open) forceOpen.add(current.parentId);
      current = current.parentId ? items.get(current.parentId) : undefined;
    }
  };
  for (const node of selected) include(node.id, filtering || node.id === state.selectedNodeId);
  for (const id of forceOpen) if (items.has(id)) items.get(id)!.expanded = true;
  /** 折叠子节点提升至最近可见父级，仅改变边的绘制锚点。 */
  const representative = (id: string) => {
    let current = items.get(id);
    let representativeId = id;
    const visited = new Set<string>();
    while (current?.parentId && !visited.has(current.id)) {
      visited.add(current.id);
      const parent = items.get(current.parentId);
      if (parent && !parent.expanded) representativeId = parent.id;
      current = parent;
    }
    return representativeId;
  };
  for (const id of [...visible]) if (representative(id) !== id) visible.delete(id);
  /** 按层级自动排布，子容器先量尺寸再排列；每层最多两列，避免重叠。 */
  const layout = (parentId: string | undefined): StructureGraphItem[] => {
    const siblings = [...items.values()].filter((item) => visible.has(item.id) && item.parentId === parentId);
    let x = parentId ? 20 : 0;
    let y = parentId ? 112 : 0;
    let rowHeight = 0;
    const output: StructureGraphItem[] = [];
    siblings.forEach((item, index) => {
      const children = layout(item.id);
      if (children.length) {
        const direct = children.filter((child) => child.parentId === item.id);
        item.width = Math.max(300, ...direct.map((child) => child.x + child.width + 20));
        item.height = Math.max(116, ...direct.map((child) => child.y + child.height + 20));
      }
      if (index > 0 && index % 2 === 0) { x = parentId ? 20 : 0; y += rowHeight + 32; rowHeight = 0; }
      item.x = x; item.y = y; x += item.width + 32; rowHeight = Math.max(rowHeight, item.height);
      output.push(item, ...children);
    });
    return output;
  };
  const nodes = layout(undefined);
  const relationships = result.relationships.filter((edge) => {
    if (edge.certainty === "candidate" && !state.filters.candidates) return false;
    if (edge.kind === "calls" && (state.filters.calls === "hidden" || (state.filters.calls === "selected" && edge.fromId !== state.selectedNodeId && edge.toId !== state.selectedNodeId))) return false;
    if (edge.kind === "extends" && !state.filters.inheritance) return false;
    if (edge.kind === "implements" && !state.filters.implementations) return false;
    if (edge.kind === "type-reference" && !state.filters.typeReferences) return false;
    if (edge.kind === "imports" && !state.filters.imports) return false;
    return visible.has(representative(edge.fromId)) && visible.has(representative(edge.toId));
  }).map((relationship) => ({ relationship, source: representative(relationship.fromId), target: representative(relationship.toId) }));
  return { nodes, relationships, relationshipChanges: result.relationships.filter((edge) => edge.certainty === "resolved" && (edge.change === "added" || edge.change === "deleted")), searchResults,
    eligibleCount: eligibleWithParents.size, visibleCount: nodes.filter((item) => item.node).length, pendingCount: Math.max(0, eligible.length - selected.length) };
}
