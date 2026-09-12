import ELK, { type ElkNode } from "elkjs/lib/elk.bundled.js";
import elkWorkerUrl from "elkjs/lib/elk-worker.min.js?url";
import { MarkerType, type Edge, type Node } from "@xyflow/react";
import type { ChangeKind, CodeNode, CodeReference, ExternalSymbol, NodeDifference, ReferenceTarget, StructureComparison } from "./types";

export type ObjectStatus = ChangeKind | "relations" | "unchanged";
/** React Flow 所需的展示数据；代码对象与变化详情仍引用原始比较。 */
export type ObjectData = Record<string, unknown> & {
  objectId?: string;
  label: string;
  nativeKind: string;
  status: ObjectStatus;
  group: boolean;
  hiddenChildren: number;
  targetKind?: "external" | "unresolved";
  targetScope?: ExternalSymbol["scope"];
  sourceY?: number;
  targetY?: number;
};
export type ObjectNode = Node<ObjectData, "codeObject">;
export type StructureEdge = Edge<{ route?: { path: string; labelX: number; labelY: number } }>;
export type StructureLayout = { nodes: ObjectNode[]; edges: StructureEdge[]; total: number; visible: number };
export type GraphOptions = { showAll: boolean; showUnchangedReferences: boolean; showImports: boolean; showDependencies: boolean; expanded: ReadonlySet<string> };

/** 已解析符号使用分析器描述；任何语言的内部 ID 都不作为可读名称解码。 */
export function externalSymbol(comparison: StructureComparison, id: string): ExternalSymbol | undefined {
  return comparison.after.externalSymbols[id] ?? comparison.before.externalSymbols[id];
}

/** 仓库外依赖和导入分别按需显示，缺少范围信息的目标保持可见而不猜测归属。 */
export function isReferenceVisible(comparison: StructureComparison, reference: CodeReference, options: Pick<GraphOptions, "showImports" | "showDependencies">): boolean {
  if (reference.semantic === "code.imports" && !options.showImports) return false;
  return options.showDependencies || reference.target.kind !== "external"
    || externalSymbol(comparison, reference.target.symbol)?.scope !== "dependency";
}

/** 关联集合按明确字段编码，不依赖对象属性序列化顺序。 */
export function referenceKey(reference: CodeReference): string {
  const target = reference.target;
  return JSON.stringify([reference.semantic, target.kind,
    target.kind === "node" ? target.id : target.kind === "external" ? target.symbol : target.name]);
}

/** 合并展示优先采用 after 的包含树；删除对象保留 before 的归属。 */
export async function layoutStructureComparison(comparison: StructureComparison, options: GraphOptions, signal?: AbortSignal): Promise<StructureLayout> {
  const before = new Map(comparison.before.nodes.map((node) => [node.id, node]));
  const after = new Map(comparison.after.nodes.map((node) => [node.id, node]));
  const objects = new Map([...before, ...after]);
  const differences = new Map(comparison.diff.nodes.map((node) => [node.id, node]));
  const parents = new Map<string, string>();
  for (const node of after.values()) for (const child of node.contains) parents.set(child, node.id);
  for (const node of before.values()) for (const child of node.contains) if (!after.has(child)) parents.set(child, node.id);
  const children = new Map<string, string[]>();
  for (const [child, parent] of parents) children.set(parent, [...(children.get(parent) ?? []), child]);
  const visible = new Set<string>();
  /** 任何显示对象都带上包含祖先，保留真实容器语义。 */
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
    for (const change of difference.references) if (isReferenceVisible(comparison, change.value, options) && change.value.target.kind === "node") include(change.value.target.id);
  }
  for (const id of options.expanded) for (const child of children.get(id) ?? []) include(child);
  if (options.showUnchangedReferences) {
    for (const id of [...visible]) {
      for (const reference of [...(before.get(id)?.references ?? []), ...(after.get(id)?.references ?? [])]) {
        if (isReferenceVisible(comparison, reference, options) && reference.target.kind === "node") include(reference.target.id);
      }
    }
  }
  const visualIds = new Map([...objects.keys()].sort().map((id, index) => [id, `object-${index}`]));
  const data = new Map<string, ObjectData>();
  for (const id of visible) {
    const object = objects.get(id)!;
    const difference = differences.get(id);
    const descendants = children.get(id) ?? [];
    data.set(visualIds.get(id)!, {
      objectId: id, label: object.name ?? object.signature ?? object.nativeKind,
      nativeKind: object.nativeKind,
      status: difference?.content ?? (difference ? "relations" : "unchanged"),
      group: descendants.some((child) => visible.has(child)),
      hiddenChildren: descendants.filter((child) => !visible.has(child)).length,
    });
  }
  const edges: StructureEdge[] = [];
  const externalIds = new Map<string, string>();
  /** 图外已解析目标与未解析名称分开绘制，不把未解析引用伪造成符号身份。 */
  const targetId = (target: ReferenceTarget, source: string): string | undefined => {
    if (target.kind === "node") return visible.has(target.id) ? visualIds.get(target.id) : undefined;
    const key = JSON.stringify(target.kind === "external" ? [target.kind, target.symbol] : [target.kind, source, target.name]);
    let id = externalIds.get(key);
    if (!id) {
      id = `target-${externalIds.size}`;
      externalIds.set(key, id);
      const descriptor = target.kind === "external" ? externalSymbol(comparison, target.symbol) : undefined;
      data.set(id, { label: target.kind === "external" ? descriptor?.name ?? "" : target.name,
        nativeKind: target.kind, status: "unchanged", group: false, hiddenChildren: 0, targetKind: target.kind, targetScope: descriptor?.scope });
    }
    return id;
  };
  /** 颜色、虚线和文本前缀共同区分新增、删除与上下文关联。 */
  const addEdge = (source: string, target: string, semantic: string, status?: "added" | "removed") => {
    const color = status === "added" ? "#059669" : status === "removed" ? "#e11d48" : "#64748b";
    edges.push({ id: `relation-${edges.length}`, source, target, type: "routedRelation",
      label: `${status === "added" ? "+ " : status === "removed" ? "− " : ""}${semantic}`,
      markerEnd: { type: MarkerType.ArrowClosed, color },
      style: { stroke: color, strokeWidth: status ? 1.8 : 1, strokeDasharray: status === "removed" ? "6 4" : undefined },
      labelStyle: { fill: color, fontSize: 10 }, labelBgStyle: { fill: "var(--background)", fillOpacity: 0.95 },
      zIndex: 2,
    });
  };
  for (const id of visible) {
    const difference = differences.get(id);
    const changes = new Map(difference?.references.map((change) => [referenceKey(change.value), change.kind]));
    const references = new Map([...(before.get(id)?.references ?? []), ...(after.get(id)?.references ?? [])].map((reference) => [referenceKey(reference), reference]));
    for (const [key, reference] of references) {
      const status = changes.get(key);
      if (!status && !options.showUnchangedReferences) continue;
      if (!isReferenceVisible(comparison, reference, options)) continue;
      const target = targetId(reference.target, id);
      if (target) addEdge(visualIds.get(id)!, target, reference.semantic, status);
    }
    for (const change of difference?.contains ?? []) {
      // 当前包含关系直接由容器表达；迁移前的包含关系以删除边保留。
      if (change.kind === "removed" && parents.get(change.value) !== id && visible.has(change.value)) {
        addEdge(visualIds.get(id)!, visualIds.get(change.value)!, "contains", "removed");
      }
    }
  }
  /** 父节点根据可见子树计算尺寸；叶节点给长名称留固定截断宽度。 */
  /** 每个节点固定左右入口，让走线路径与可见连接点保持一致。 */
  const ports = (id: string) => [{ id: `${id}-in`, width: 1, height: 1, layoutOptions: { "elk.port.side": "WEST" } }, { id: `${id}-out`, width: 1, height: 1, layoutOptions: { "elk.port.side": "EAST" } }];
  const elkObject = (id: string): ElkNode => {
    const nested = (children.get(id) ?? []).filter((child) => visible.has(child)).sort();
    return {
      id: visualIds.get(id)!, width: 280, height: data.get(visualIds.get(id)!)!.hiddenChildren > 0 ? 108 : 92, ports: ports(visualIds.get(id)!),
      layoutOptions: { "elk.portConstraints": "FIXED_SIDE" },
      ...(nested.length ? { children: nested.map(elkObject), layoutOptions: {
        "elk.portConstraints": "FIXED_SIDE",
        "elk.padding": "[top=100,left=24,bottom=24,right=24]",
        "elk.nodeSize.minimum": "(328,192)",
      } } : {}),
    };
  };
  const roots = [...visible].filter((id) => !parents.has(id)).sort().map(elkObject);
  for (const id of externalIds.values()) roots.push({ id, width: 280, height: 92, ports: ports(id), layoutOptions: { "elk.portConstraints": "FIXED_SIDE" } });
  if (!roots.length) return { nodes: [], edges: [], total: objects.size, visible: 0 };
  /** ancestor 边不参加层次求解，避免包含容器同时成为自身层次约束。 */
  const isAncestor = (ancestor: string, child: string) => {
    let parent = parents.get(child);
    while (parent) { if (parent === ancestor) return true; parent = parents.get(parent); }
    return false;
  };
  const layout = await arrange({ id: "structure-root", children: roots,
    edges: edges.filter((edge) => {
      const source = data.get(edge.source)?.objectId, target = data.get(edge.target)?.objectId;
      return !source || !target || (!isAncestor(source, target) && !isAncestor(target, source));
    }).map((edge) => ({ id: edge.id, sources: [`${edge.source}-out`], targets: [`${edge.target}-in`], labels: [{ text: String(edge.label), width: String(edge.label).length * 6, height: 16 }] })),
    layoutOptions: { "elk.algorithm": "layered", "elk.direction": "RIGHT", "elk.hierarchyHandling": "INCLUDE_CHILDREN",
      "elk.spacing.nodeNode": "48", "elk.layered.spacing.nodeNodeBetweenLayers": "140", "elk.padding": "[top=24,left=24,bottom=24,right=24]" },
  }, signal);
  const nodes: ObjectNode[] = [];
  const origins = new Map<string, { x: number; y: number }>([["structure-root", { x: 0, y: 0 }]]);
  /** React Flow 要求父节点先于子节点，位置相对直接父级。 */
  const flatten = (node: ElkNode, parentId?: string) => {
    const origin = origins.get(parentId ?? "structure-root")!;
    origins.set(node.id, { x: origin.x + (node.x ?? 0), y: origin.y + (node.y ?? 0) });
    const objectData = data.get(node.id)!;
    objectData.sourceY = node.ports?.find((port) => port.id === `${node.id}-out`)?.y;
    objectData.targetY = node.ports?.find((port) => port.id === `${node.id}-in`)?.y;
    nodes.push({ id: node.id, type: "codeObject", data: objectData,
      position: { x: node.x ?? 0, y: node.y ?? 0 }, parentId,
      style: { width: node.width ?? 280, height: node.height ?? 92 },
      draggable: false, connectable: false,
    });
    node.children?.forEach((child) => flatten(child, node.id));
  };
  layout.children?.forEach((node) => flatten(node));
  // ELK 走线相对其 container；转成画布坐标，避免默认连线穿过其他对象。
  for (const routed of layout.edges ?? []) {
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
  return { nodes, edges, total: objects.size, visible: visible.size };
}

/** 详情读取两侧原始声明，保持内容变化和关系集合差异相互独立。 */
export function objectDetails(comparison: StructureComparison, id: string): {
  before?: CodeNode; after?: CodeNode; difference?: NodeDifference;
} {
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
