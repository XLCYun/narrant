import ELK, { type ElkNode } from "elkjs/lib/elk.bundled.js";
import elkWorkerUrl from "elkjs/lib/elk-worker.min.js?url";
import { MarkerType, type Edge, type Node } from "@xyflow/react";
import type { ChangeKind, CodeNode, CodeReference, NodeDifference, StructureComparison } from "./types";

export type ObjectStatus = ChangeKind | "relations" | "unchanged";
/** 成员保留原对象身份；每行的高度同时决定文字位置和连线端口。 */
export type ObjectRow = {
  objectId?: string;
  label: string;
  status: ObjectStatus;
  kind: "value" | "callable" | "implementation" | "section";
  section?: "values" | "callables";
  top: number;
  height: number;
  hiddenChildren: number;
};
/** 每个对象拥有独立端口，即使多个对象被收进同一张类卡片。 */
export type ObjectPort = { id: string; objectId?: string; side: "source" | "target"; y: number; connected?: boolean };
/** React Flow 所需的展示数据；代码对象与变化详情仍引用原始比较。 */
export type ObjectData = Record<string, unknown> & {
  objectId?: string;
  label: string;
  nativeKind: string;
  status: ObjectStatus;
  group: boolean;
  hiddenChildren: number;
  category?: CodeNode["category"];
  table: boolean;
  headerHeight: number;
  rows: ObjectRow[];
  ports: ObjectPort[];
};
export type ObjectNode = Node<ObjectData, "codeObject">;
export type StructureEdge = Edge<{ route?: { path: string; labelX: number; labelY: number } }>;
export type StructureLayout = { nodes: ObjectNode[]; edges: StructureEdge[]; total: number; visible: number; changed: number };
export type GraphOptions = { showAll: boolean; showUnchangedReferences: boolean; showImports: boolean; expanded: ReadonlySet<string> };

/** 只显示图内已解析关系；导入开关仅决定图内导入关系是否出现。 */
export function isReferenceVisible(reference: CodeReference, options: Pick<GraphOptions, "showImports">): boolean {
  return reference.target.kind === "node" && (reference.semantic !== "code.imports" || options.showImports);
}

/** 关联集合按明确字段编码，不依赖对象属性序列化顺序。 */
export function referenceKey(reference: CodeReference): string {
  const target = reference.target;
  return JSON.stringify([reference.semantic, target.kind,
    target.kind === "node" ? target.id : target.kind === "external" ? target.symbol : target.name]);
}

/** 画布、计数和详情统一排除 use 声明及所有图外、未解析关系，保留完整分析结果。 */
function displayComparison(comparison: StructureComparison): StructureComparison {
  const objects = new Map([...comparison.before.nodes, ...comparison.after.nodes].map((node) => [node.id, node]));
  const hidden = new Set([...objects.values()]
    .filter((node) => node.language === "rust" && node.category === "extension" && node.nativeKind === "use")
    .map((node) => node.id));
  const keepsNode = (id: string) => objects.has(id) && !hidden.has(id);
  const keepsReference = (reference: CodeReference) => reference.target.kind === "node" && keepsNode(reference.target.id);
  const graph = (source: StructureComparison["before"]): StructureComparison["before"] => ({
    ...source, root: source.root.filter(keepsNode),
    nodes: source.nodes.filter((node) => keepsNode(node.id)).map((node) => ({
      ...node, contains: node.contains.filter(keepsNode), references: node.references.filter(keepsReference),
    })),
  });
  const differences = comparison.diff.nodes.filter((node) => keepsNode(node.id)).map((node) => ({
    ...node, contains: node.contains.filter((change) => keepsNode(change.value)),
    references: node.references.filter((change) => keepsReference(change.value)),
  })).filter((node) => node.content !== null || node.contains.length > 0 || node.references.length > 0);
  return { ...comparison, before: graph(comparison.before), after: graph(comparison.after), diff: { nodes: differences } };
}

/** 按类图聚合成员；代码身份、两侧声明与差异仍保留在原始比较中。 */
export async function layoutStructureComparison(comparison: StructureComparison, options: GraphOptions, signal?: AbortSignal): Promise<StructureLayout> {
  // 归属判定保留完整语义，不能因隐藏一个候选而把歧义关系误当成唯一归属。
  const semanticObjects = new Map([...comparison.before.nodes, ...comparison.after.nodes].map((node) => [node.id, node]));
  comparison = displayComparison(comparison);
  const before = new Map(comparison.before.nodes.map((node) => [node.id, node]));
  const after = new Map(comparison.after.nodes.map((node) => [node.id, node]));
  const objects = new Map([...before, ...after]);
  const differences = new Map(comparison.diff.nodes.map((node) => [node.id, node]));
  const containment = new Map<string, string>();
  for (const node of after.values()) for (const child of node.contains) containment.set(child, node.id);
  for (const node of before.values()) for (const child of node.contains) if (!after.has(child)) containment.set(child, node.id);
  const parents = new Map(containment);
  /** 只接受唯一且在图内的类型归属，不根据名称、ID 或前后关系的并集猜测。 */
  const typeTarget = (object: CodeNode, semantic: string) => {
    const targets = new Map((semanticObjects.get(object.id)?.references ?? []).filter((reference) => reference.semantic === semantic).map((reference) => [referenceKey(reference), reference.target]));
    const target = targets.size === 1 ? [...targets.values()][0] : undefined;
    return target?.kind === "node" && objects.get(target.id)?.category === "type" ? target.id : undefined;
  };
  /** 语义归属只改变展示分组；拒绝会形成包含环的候选。 */
  const place = (id: string, parent: string) => {
    let ancestor: string | undefined = parent;
    while (ancestor) { if (ancestor === id) return false; ancestor = parents.get(ancestor); }
    parents.set(id, parent);
    return true;
  };
  const implementations = new Set<string>();
  for (const object of objects.values()) {
    if (object.category !== "extension") continue;
    const target = typeTarget(object, "code.applies_to");
    if (target && place(object.id, target)) implementations.add(object.id);
  }
  for (const object of objects.values()) {
    if (object.category !== "callable" && object.category !== "value") continue;
    if (implementations.has(parents.get(object.id) ?? "")) continue;
    const target = typeTarget(object, "code.member_of");
    if (target) place(object.id, target);
  }
  const children = new Map<string, string[]>();
  for (const [child, parent] of parents) children.set(parent, [...(children.get(parent) ?? []), child]);
  const visible = new Set<string>();
  /** 成员变化带上类卡片和模块上下文，不把子对象变化当作类型自身修改。 */
  const include = (id: string) => {
    let current: string | undefined = id;
    while (current && objects.has(current) && !visible.has(current)) {
      visible.add(current);
      current = parents.get(current);
    }
  };
  for (const id of options.showAll ? objects.keys() : differences.keys()) include(id);
  for (const difference of differences.values()) {
    for (const change of difference.contains) include(change.value);
    for (const change of difference.references) if (isReferenceVisible(change.value, options) && change.value.target.kind === "node") include(change.value.target.id);
  }
  for (const id of options.expanded) for (const child of children.get(id) ?? []) {
    include(child);
    if (implementations.has(child)) for (const member of children.get(child) ?? []) include(member);
  }
  if (options.showUnchangedReferences) for (const id of [...visible]) {
    for (const reference of [...(before.get(id)?.references ?? []), ...(after.get(id)?.references ?? [])]) {
      if (isReferenceVisible(reference, options) && reference.target.kind === "node") include(reference.target.id);
    }
  }
  const visualIds = new Map([...objects.keys()].sort().map((id, index) => [id, `object-${index}`]));
  const status = (id: string): ObjectStatus => differences.get(id)?.content ?? (differences.has(id) ? "relations" : "unchanged");
  const hiddenChildren = (id: string) => (children.get(id) ?? []).filter((child) => !visible.has(child)).length;
  const isLeafMember = (id: string) => ["callable", "value"].includes(objects.get(id)!.category) && !(children.get(id)?.length);
  // 含嵌套命名声明的对象继续保留容器；普通字段、方法及 impl 标题收进成员表。
  const inlineImplementations = new Set([...implementations].filter((id) => (children.get(id) ?? []).every(isLeafMember)));
  const rowOwners = new Map<string, string | undefined>();
  for (const id of visible) {
    if (!isLeafMember(id) && !inlineImplementations.has(id)) continue;
    const parent = parents.get(id);
    rowOwners.set(id, parent && inlineImplementations.has(parent) ? parents.get(parent) : parent);
  }
  const cardObjects = [...visible].filter((id) => !rowOwners.has(id)).sort();
  const data = new Map<string, ObjectData>();
  const cardParents = new Map<string, string>();
  const sizes = new Map<string, { width: number; height: number }>();
  const attachments = new Map<string, { card: string; source: string; target: string }>();
  /** 每个真实对象只出现一次；成员端口与卡片标题端口共用同一身份映射。 */
  const attach = (id: string, card: string, y: number) => {
    const base = visualIds.get(id)!;
    const ports = { card, source: `${base}-out`, target: `${base}-in` };
    attachments.set(id, ports);
    data.get(card)!.ports.push({ id: ports.source, objectId: id, side: "source", y }, { id: ports.target, objectId: id, side: "target", y });
  };
  for (const id of cardObjects) {
    const object = objects.get(id)!;
    const card = visualIds.get(id)!;
    const hidden = hiddenChildren(id);
    data.set(card, { objectId: id, label: object.name ?? object.signature ?? object.nativeKind,
      nativeKind: object.nativeKind, category: object.category, status: status(id), group: false,
      hiddenChildren: hidden, table: false, headerHeight: hidden > 0 ? 82 : 56, rows: [], ports: [] });
    const parent = parents.get(id);
    if (parent) cardParents.set(card, visualIds.get(parent)!);
    attach(id, card, 28);
  }
  const groupedRows = new Map<string | undefined, string[]>();
  for (const [id, owner] of rowOwners) groupedRows.set(owner, [...(groupedRows.get(owner) ?? []), id]);
  /** 成员按声明签名稳定排列，不伪造跨语言的参数或可见性语法。 */
  const compare = (left: string, right: string) => {
    const a = objects.get(left)!, b = objects.get(right)!;
    return (a.name ?? a.signature ?? left).localeCompare(b.name ?? b.signature ?? right) || left.localeCompare(right);
  };
  for (const [owner, ids] of groupedRows) {
    const ownerCard = owner ? visualIds.get(owner)! : undefined;
    const nestedCards = ownerCard && [...cardParents.values()].includes(ownerCard);
    const card = ownerCard && !nestedCards ? ownerCard : `members-${ownerCard ?? "root"}`;
    if (card !== ownerCard) {
      data.set(card, { label: "", nativeKind: "", category: owner ? objects.get(owner)?.category : undefined,
        status: "unchanged", group: false, hiddenChildren: 0, table: true, headerHeight: 0, rows: [], ports: [] });
      if (ownerCard) cardParents.set(card, ownerCard);
    }
    const content = data.get(card)!;
    let top = content.headerHeight;
    /** 分隔属性与方法，固定行高用于连线落点和真实渲染的一致性。 */
    const section = (kind: "values" | "callables") => {
      content.rows.push({ label: "", kind: "section", section: kind, status: "unchanged", top, height: 26, hiddenChildren: 0 });
      top += 26;
    };
    /** impl 标题也可查看自身差异；折叠不丢失实现块的条件、trait 和泛型信息。 */
    const member = (id: string) => {
      const object = objects.get(id)!;
      const implementation = inlineImplementations.has(id);
      const height = 30;
      content.rows.push({ objectId: id, label: (object.signature ?? object.name ?? object.nativeKind).replace(/\s+/g, " ").trim(),
        kind: implementation ? "implementation" : object.category === "value" ? "value" : "callable",
        status: status(id), top, height, hiddenChildren: hiddenChildren(id) });
      attach(id, card, top + height / 2);
      top += height;
    };
    const direct = ids.filter((id) => !inlineImplementations.has(id) && !inlineImplementations.has(parents.get(id) ?? ""));
    const values = direct.filter((id) => objects.get(id)!.category === "value").sort(compare);
    const callables = direct.filter((id) => objects.get(id)!.category === "callable").sort(compare);
    const blocks = ids.filter((id) => inlineImplementations.has(id)).sort(compare);
    if (values.length) { section("values"); values.forEach(member); }
    if (callables.length || blocks.length) {
      section("callables");
      callables.forEach(member);
      for (const block of blocks) {
        member(block);
        ids.filter((id) => parents.get(id) === block).sort((a, b) => {
          const categories = Number(objects.get(a)!.category === "callable") - Number(objects.get(b)!.category === "callable");
          return categories || compare(a, b);
        }).forEach(member);
      }
    }
    sizes.set(card, { width: 360, height: top + 6 });
  }
  const edges: StructureEdge[] = [];
  /** 连线连接原成员的独立端口，不把多个方法的关系合成类级关系。 */
  const addEdge = (source: string, target: { card: string; source: string; target: string }, semantic: string, change?: "added" | "removed") => {
    const origin = attachments.get(source)!;
    // 同卡片的归属已由分区表达；实际调用（包括递归和成员间调用）继续绘制。
    if (["code.member_of", "code.applies_to"].includes(semantic)) {
      let containingCard: string | undefined = origin.card;
      while (containingCard) {
        if (containingCard === target.card) return;
        containingCard = cardParents.get(containingCard);
      }
    }
    const color = change === "added" ? "#059669" : change === "removed" ? "#e11d48" : "#64748b";
    edges.push({ id: `relation-${edges.length}`, source: origin.card, sourceHandle: origin.source, target: target.card, targetHandle: target.target, type: "routedRelation",
      label: `${change === "added" ? "+ " : change === "removed" ? "− " : ""}${semantic}`,
      markerEnd: { type: MarkerType.ArrowClosed, color },
      style: { stroke: color, strokeWidth: change ? 1.8 : 1, strokeDasharray: change === "removed" ? "6 4" : undefined },
      labelStyle: { fill: color, fontSize: 10 }, labelBgStyle: { fill: "var(--background)", fillOpacity: 0.95 }, zIndex: 2 });
  };
  for (const id of visible) {
    const difference = differences.get(id);
    const changes = new Map(difference?.references.map((change) => [referenceKey(change.value), change.kind]));
    const references = new Map([...(before.get(id)?.references ?? []), ...(after.get(id)?.references ?? [])].map((reference) => [referenceKey(reference), reference]));
    for (const [key, reference] of references) {
      const change = changes.get(key);
      if ((!change && !options.showUnchangedReferences) || !isReferenceVisible(reference, options) || reference.target.kind !== "node") continue;
      const target = attachments.get(reference.target.id);
      if (target) addEdge(id, target, reference.semantic, change);
    }
    for (const change of difference?.contains ?? []) {
      if (change.kind === "removed" && containment.get(change.value) !== id) {
        const target = attachments.get(change.value);
        if (target) addEdge(id, target, "contains", "removed");
      }
    }
  }
  const cardChildren = new Map<string, string[]>();
  const connectedPorts = new Set(edges.flatMap((edge) => [edge.sourceHandle, edge.targetHandle]));
  for (const content of data.values()) for (const port of content.ports) port.connected = connectedPorts.has(port.id);
  for (const [child, parent] of cardParents) cardChildren.set(parent, [...(cardChildren.get(parent) ?? []), child]);
  /** ELK 只排列卡片；固定成员端口保证方法与属性仍可逐行连线。 */
  const elkCard = (id: string): ElkNode => {
    const content = data.get(id)!;
    const nested = (cardChildren.get(id) ?? []).sort();
    content.group = nested.length > 0;
    const { width, height } = sizes.get(id) ?? { width: 360, height: content.headerHeight + 6 };
    return { id, width, height,
      ports: content.ports.map((port) => ({ id: port.id, width: 0, height: 0,
        ...(!nested.length ? { x: port.side === "source" ? width : 0, y: port.y } : {}),
        layoutOptions: { "elk.port.side": port.side === "source" ? "EAST" : "WEST" } })),
      layoutOptions: nested.length ? { "elk.portConstraints": "FIXED_SIDE",
        "elk.padding": `[top=${content.headerHeight + 12},left=16,bottom=16,right=16]`, "elk.nodeSize.minimum": "(392,100)" }
        : { "elk.portConstraints": "FIXED_POS" },
      ...(nested.length ? { children: nested.map(elkCard) } : {}),
    };
  };
  const roots = [...data.keys()].filter((id) => !cardParents.has(id)).sort().map(elkCard);
  if (!roots.length) return { nodes: [], edges: [], total: objects.size, visible: 0, changed: differences.size };
  /** 容器祖先关系不参加层级求解，成员间和递归调用均交给 ELK 走线。 */
  const isAncestor = (ancestor: string, child: string) => {
    let parent = cardParents.get(child);
    while (parent) { if (parent === ancestor) return true; parent = cardParents.get(parent); }
    return false;
  };
  const layout = await arrange({ id: "structure-root", children: roots,
    edges: edges.filter((edge) => !isAncestor(edge.source, edge.target) && !isAncestor(edge.target, edge.source))
      .map((edge) => ({ id: edge.id, sources: [edge.sourceHandle!], targets: [edge.targetHandle!], labels: [{ text: String(edge.label), width: String(edge.label).length * 6, height: 16 }] })),
    layoutOptions: { "elk.algorithm": "layered", "elk.direction": "RIGHT", "elk.hierarchyHandling": "INCLUDE_CHILDREN",
      "elk.spacing.nodeNode": "24", "elk.layered.spacing.nodeNodeBetweenLayers": "100", "elk.padding": "[top=16,left=16,bottom=16,right=16]" },
  }, signal);
  const nodes: ObjectNode[] = [];
  const origins = new Map<string, { x: number; y: number }>([["structure-root", { x: 0, y: 0 }]]);
  const routedEdges = [...(layout.edges ?? [])];
  /** React Flow 要求父卡片在前；成员行由卡片内部渲染，无额外节点盒子。 */
  const flatten = (node: ElkNode, parentId?: string) => {
    const origin = origins.get(parentId ?? "structure-root")!;
    origins.set(node.id, { x: origin.x + (node.x ?? 0), y: origin.y + (node.y ?? 0) });
    const objectData = data.get(node.id)!;
    for (const port of objectData.ports) port.y = node.ports?.find((item) => item.id === port.id)?.y ?? port.y;
    nodes.push({ id: node.id, type: "codeObject", data: objectData, position: { x: node.x ?? 0, y: node.y ?? 0 }, parentId,
      style: { width: node.width ?? 360, height: node.height ?? 64 }, draggable: false, connectable: false });
    routedEdges.push(...(node.edges ?? []));
    node.children?.forEach((child) => flatten(child, node.id));
  };
  layout.children?.forEach((node) => flatten(node));
  for (const routed of routedEdges) {
    const edge = edges.find((item) => item.id === routed.id);
    const section = routed.sections?.[0];
    if (!edge || !section) continue;
    const origin = origins.get(routed.container ?? "structure-root")!;
    const points = [section.startPoint, ...(section.bendPoints ?? []), section.endPoint]
      .map((point) => ({ x: point.x + origin.x, y: point.y + origin.y }));
    const label = routed.labels?.[0];
    edge.data = { route: {
      path: points.map((point, index) => `${index ? "L" : "M"}${point.x},${point.y}`).join(" "),
      labelX: label?.x === undefined ? (points[0].x + points[1].x) / 2 : label.x + origin.x + (label.width ?? 0) / 2,
      labelY: label?.y === undefined ? points[0].y : label.y + origin.y + (label.height ?? 0) / 2,
    } };
  }
  return { nodes, edges, total: objects.size, visible: visible.size, changed: differences.size };
}

/** 详情保留两侧声明内容，并与画布使用相同的对象及关系投影。 */
export function objectDetails(comparison: StructureComparison, id: string): {
  before?: CodeNode; after?: CodeNode; difference?: NodeDifference;
} {
  comparison = displayComparison(comparison);
  return { before: comparison.before.nodes.find((node) => node.id === id),
    after: comparison.after.nodes.find((node) => node.id === id),
    difference: comparison.diff.nodes.find((node) => node.id === id) };
}

/** 布局 worker 的寿命由本次展示拥有；页面离开、脚本加载失败和超时都能收敛。 */
async function arrange(graph: ElkNode, signal?: AbortSignal): Promise<ElkNode> {
  if (signal?.aborted) throw new DOMException("Layout cancelled", "AbortError");
  const worker = typeof Worker !== "undefined" ? new Worker(elkWorkerUrl) : undefined;
  const elk = new ELK(worker ? { workerFactory: () => worker } : undefined);
  let interrupt!: (reason: Error) => void;
  const interrupted = new Promise<never>((_, reject) => { interrupt = reject; });
  const abort = () => interrupt(new DOMException("Layout cancelled", "AbortError"));
  const failed = (event: ErrorEvent) => interrupt(new Error(event.message || "Layout worker failed to load"));
  signal?.addEventListener("abort", abort, { once: true });
  worker?.addEventListener("error", failed);
  const timeout = setTimeout(() => interrupt(new Error("Structure layout timed out")), 60_000);
  try { return await Promise.race([elk.layout(graph), interrupted]); }
  finally {
    clearTimeout(timeout);
    signal?.removeEventListener("abort", abort);
    worker?.terminate();
  }
}
