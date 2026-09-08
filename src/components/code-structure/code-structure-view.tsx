import { useCallback, useEffect, useId, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { Background, Handle, MarkerType, Position, ReactFlow, type Node, type NodeProps, type ReactFlowInstance } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Toggle } from "@/components/ui/toggle";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { buildStructureGraph, createStructureGraphPresentation, expandStructureNeighbors, structureDeclarationValue, structureNodeLabel, type StructureGraphItem } from "@/review/structure-graph";
import type { StructureGraphAnalysis, StructureGraphNavigation, StructureGraphPresentation } from "@/review/structure-graph-types";
import type { StructureRelationship, StructureResult } from "@/review/structure-types";
import "./code-structure.css";

/** 纯受控画布边界；运行时、导航和 Peek 均由父级 Review 持有。 */
export type CodeStructureViewProps = {
  analysis: StructureGraphAnalysis;
  presentation: StructureGraphPresentation;
  onPresentationChange: (state: StructureGraphPresentation) => void;
  onNavigate: (intent: StructureGraphNavigation) => void;
  onCancel?: () => void;
  onRetry?: () => void;
  colorMode?: "light" | "dark";
};

/** 节点只拥有可显示字段与已绑定动作，不接触分析服务。 */
type CanvasNodeData = { item: StructureGraphItem; onToggle: () => void; onOpen: () => void };
type CanvasNode = Node<CanvasNodeData, "structure">;

/** 自定义嵌套节点：保留原生名称、签名、多标签和后代计数。 */
function StructureCanvasNode({ data, selected }: NodeProps<CanvasNode>) {
  const { t } = useTranslation("structure");
  const { item } = data;
  const node = item.node;
  const declaration = node?.kind === "declaration" ? structureDeclarationValue(node) : null;
  const changes = node?.kind === "declaration" ? node.changes : [];
  return <div className={`structure-node ${item.childCount && item.expanded ? "structure-container" : ""} ${selected ? "structure-selected" : ""}`}>
    <Handle type="target" position={Position.Left} isConnectable={false} />
    <div className="structure-node-header">
      <div className="flex min-w-0 items-center gap-2">
        {item.childCount > 0 && <Button size="icon-sm" variant="ghost" className="nodrag nopan shrink-0" aria-label={t(item.expanded ? "collapse" : "expand", { name: item.label, count: item.childCount })} onClick={(event) => { event.stopPropagation(); data.onToggle(); }}>{item.expanded ? "−" : "+"}</Button>}
        <Button variant="ghost" className="nodrag nopan h-auto min-w-0 flex-1 justify-start rounded-sm border-0 p-0 text-left font-mono text-sm font-semibold leading-normal active:translate-y-0" title={item.label} aria-label={t("open", { name: item.label })} onClick={(event) => { event.stopPropagation(); data.onOpen(); }}><span className="block min-w-0 truncate">{item.label}</span></Button>
      </div>
      <div className="mt-1 flex flex-wrap gap-x-2 text-[10px] text-muted-foreground">
        <span>{declaration?.nativeKind.name ?? (item.implementation ? "impl" : node?.kind === "external" ? t("external") : t("file"))}</span>
        {changes.map((change) => <span key={change} className={`structure-change-${change}`}>{change === "added" ? "+ " : change === "deleted" ? "− " : "● "}{t(change)}</span>)}
        {node?.kind === "declaration" && changes.length === 0 && <span>{t(node.comparison === "complete" ? "context" : "incomplete")}</span>}
        {node && node.kind !== "external" && node.containedChanges.union > 0 && <span>{t("contains", { count: node.containedChanges.union })}</span>}
      </div>
      {item.signature && <div className="mt-2 truncate font-mono text-xs text-muted-foreground" title={item.signature}>{item.signature}</div>}
    </div>
    <Handle type="source" position={Position.Right} isConnectable={false} />
  </div>;
}
const nodeTypes = { structure: StructureCanvasNode };

/** 复用 shadcn Select 实现有完整可访问标签的单值过滤器。 */
function GraphSelect({ label, value, options, onChange }: { label: string; value: string; options: Array<{ value: string; label: string }>; onChange: (value: string) => void }) {
  return <Select value={value} onValueChange={(next) => { if (next) onChange(next); }}>
    <SelectTrigger size="sm" aria-label={label} className="max-w-full"><SelectValue>{options.find((option) => option.value === value)?.label ?? value}</SelectValue></SelectTrigger>
    <SelectContent>{options.map((option) => <SelectItem key={option.value} value={option.value}>{option.label}</SelectItem>)}</SelectContent>
  </Select>;
}

/** 每条关系提供发生位置、原始证据与独立定义入口，回调来源保持真实。 */
function RelationshipCard({ edge, result, onNavigate }: { edge: StructureRelationship; result: StructureResult; onNavigate: CodeStructureViewProps["onNavigate"] }) {
  const { t } = useTranslation("structure");
  const [evidenceOpen, setEvidenceOpen] = useState(false);
  const [occurrenceLimit, setOccurrenceLimit] = useState(20);
  const from = result.nodes.find((node) => node.id === edge.fromId);
  const target = result.nodes.find((node) => node.id === edge.toId);
  const occurrences = [...(edge.after.state === "present" ? edge.after.value : []), ...(edge.before.state === "present" ? edge.before.value : [])];
  return <li className="rounded-md border p-2 text-xs">
    <div className="break-words font-mono">{from ? structureNodeLabel(from, result) : edge.fromId} → {target ? structureNodeLabel(target, result) : edge.toId}</div>
    <p className="my-1 text-muted-foreground">{edge.kind} · {t(edge.change)} · {t(edge.certainty)}</p>
    <div className="flex flex-wrap gap-1">
      <Button variant="outline" size="sm" onClick={() => onNavigate({ kind: "relationship", relationshipId: edge.id })}>{t("occurrences")} ({occurrences.length})</Button>
      {target?.kind !== "external" && <Button variant="ghost" size="sm" onClick={() => onNavigate({ kind: "target-definition", nodeId: edge.toId, relationshipId: edge.id })}>{t("targetDefinition")}</Button>}
    </div>
    {target?.kind === "external" && <p className="mt-1 text-muted-foreground">{t("externalUnavailable")}</p>}
    {occurrences.some((occurrence) => occurrence.execution.kind === "anonymous-callback") && <p className="mt-1">↳ {t("callback")}</p>}
    {(["before", "after"] as const).map((side) => edge[side].state === "unknown" && <div key={side} className="mt-1 text-muted-foreground">{t(side)} · {t("indeterminate")}{edge[side].diagnostics.map((diagnostic, index) => <p key={index}>{diagnostic.message}</p>)}</div>)}
    {occurrences.length > 0 && <details className="mt-1" onToggle={(event) => setEvidenceOpen(event.currentTarget.open)}><summary>{t("evidence")}</summary>{evidenceOpen && occurrences.slice(0, occurrenceLimit).map((occurrence, index) => <div key={index} className="mt-2 break-words">
      <Button variant="link" className="block h-auto max-w-full whitespace-normal rounded-none border-0 p-0 text-left text-xs font-normal leading-normal text-foreground underline active:translate-y-0" onClick={() => onNavigate({ kind: "location", location: occurrence.location })}>{t(occurrence.location.side)} · L{occurrence.location.range.startLine} · {t(occurrence.execution.kind === "anonymous-callback" ? "callback" : occurrence.execution.kind === "file" ? "file" : "body")}</Button>
      {occurrence.evidence.map((evidence, evidenceIndex) => <p key={evidenceIndex}>{evidence.kind}: {evidence.detail}</p>)}
    </div>)}{evidenceOpen && occurrences.length > occurrenceLimit && <Button size="sm" variant="outline" onClick={() => setOccurrenceLimit((limit) => limit + 20)}>{t("more", { count: Math.min(20, occurrences.length - occurrenceLimit) })}</Button>}</details>}
  </li>;
}

/** 真实节点/边画布及独立发现面板，隐藏连线不会删除关系变化入口。 */
export function CodeStructureView({ analysis, presentation, onPresentationChange, onNavigate, onCancel, onRetry, colorMode = "light" }: CodeStructureViewProps) {
  const { t } = useTranslation("structure");
  const flow = useRef<ReactFlowInstance<CanvasNode> | null>(null);
  const canvasRef = useRef<HTMLDivElement>(null);
  const inspectorId = useId();
  const [inspectorOpen, setInspectorOpen] = useState(() => Boolean(presentation.query));
  const fitFrame = useRef<number | null>(null);
  const cameraMode = useRef(presentation.viewportMode ?? (presentation.viewport ? "manual" : "auto"));
  /** 同步跨 Review 保存的相机意图；程序初始 fit 不算作用户移动。 */
  useEffect(() => { cameraMode.current = presentation.viewportMode ?? (presentation.viewport ? "manual" : "auto"); }, [presentation.viewportMode, presentation.viewport]);
  /** 等待实际画布尺寸提交后再适配；手动移动会取消尚未执行的自动 fit。 */
  const scheduleFit = useCallback(() => {
    if (fitFrame.current !== null) cancelAnimationFrame(fitFrame.current);
    fitFrame.current = requestAnimationFrame(() => {
      fitFrame.current = null;
      const bounds = canvasRef.current?.getBoundingClientRect();
      if (cameraMode.current === "auto" && bounds && bounds.width > 0 && bounds.height > 0) {
        void flow.current?.fitView({ padding: 0.12, minZoom: 0.02, maxZoom: 1 });
      }
    });
  }, []);
  const result = analysis.result;
  const graph = useMemo(() => result ? buildStructureGraph(result, presentation) : null,
    // 相机运动无需重排拓扑；只有节点集合与展示规则改变时重新计算。
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [result, presentation.selectedNodeId, presentation.expandedIds, presentation.revealedNodeIds, presentation.query, presentation.displayLimit, presentation.filters]);
  /** Peek 开关与分割条改变的是可用画布；未手动移动时持续保留完整初始焦点。 */
  useEffect(() => {
    const element = canvasRef.current;
    if (!element) return;
    const observer = new ResizeObserver(scheduleFit);
    observer.observe(element);
    scheduleFit();
    return () => { observer.disconnect(); if (fitFrame.current !== null) cancelAnimationFrame(fitFrame.current); };
  }, [scheduleFit]);
  useEffect(() => { scheduleFit(); }, [graph, scheduleFit]);
  /** 使用父级最新展示状态提交局部更新。 */
  const update = (patch: Partial<StructureGraphPresentation>) => onPresentationChange({ ...presentation, ...patch });
  /** 侧栏导航同步受控选择，目标定义与关系发生位置不会混用。 */
  const navigate = (intent: StructureGraphNavigation) => {
    if (intent.kind === "relationship") update({ selectedRelationshipId: intent.relationshipId });
    if (intent.kind === "target-definition") update({ selectedNodeId: intent.nodeId, selectedRelationshipId: null });
    onNavigate(intent);
  };
  /** 选择节点同时保留图视野，让 Peek 独立决定源码来源。 */
  const openNode = (id: string) => { update({ selectedNodeId: id, selectedRelationshipId: null }); onNavigate({ kind: "node", nodeId: id }); };
  /** 展开容器时显式加入直接子项；初始文件展开不导入无关声明。 */
  const toggle = (item: StructureGraphItem) => {
    const expanded = presentation.expandedIds.includes(item.id);
    const children = result?.nodes.filter((node) => node.kind === "declaration" && (structureDeclarationValue(node)?.implementationGroupId ?? node.displayParentId) === item.id).map((node) => node.id) ?? [];
    const groups = result?.implementationGroups.filter((group) => (group.after.state === "present" ? group.after.value.ownerId : group.before.state === "present" ? group.before.value.ownerId : null) === item.id).map((group) => group.id) ?? [];
    update({ selectedNodeId: expanded ? item.id : presentation.selectedNodeId, expandedIds: expanded ? presentation.expandedIds.filter((id) => id !== item.id) : [...presentation.expandedIds, item.id, ...groups], revealedNodeIds: expanded ? presentation.revealedNodeIds : [...new Set([...presentation.revealedNodeIds, ...children, ...(result?.nodes.filter((node) => node.kind === "declaration" && groups.includes(structureDeclarationValue(node)?.implementationGroupId ?? "")).map((node) => node.id) ?? [])])] });
  };
  const nodes: CanvasNode[] = (graph?.nodes ?? []).map((item) => ({ id: item.id, type: "structure", parentId: item.parentId, position: { x: item.x, y: item.y }, style: { width: item.width, height: item.height }, selected: presentation.selectedNodeId === item.id,
    data: { item, onToggle: () => toggle(item), onOpen: () => item.implementation ? onNavigate({ kind: "implementation", groupId: item.id }) : openNode(item.id) } }));
  const edges = (graph?.relationships ?? []).map(({ relationship, source, target }) => ({ id: relationship.id, source, target, label: `${relationship.change === "added" ? "+" : relationship.change === "deleted" ? "−" : relationship.change === "retained" ? "=" : "?"} ${relationship.kind}${relationship.certainty === "candidate" ? " ◇" : ""}`, className: `structure-edge-${relationship.change}`, style: { strokeWidth: 2, strokeDasharray: relationship.certainty === "candidate" ? "5 5" : undefined }, markerEnd: { type: MarkerType.ArrowClosed }, selected: presentation.selectedRelationshipId === relationship.id, ariaLabel: `${relationship.kind}: ${t(relationship.change)}, ${t(relationship.certainty)}` }));
  const selected = result?.nodes.find((node) => node.id === presentation.selectedNodeId);
  const selectedValue = selected?.kind === "declaration" ? structureDeclarationValue(selected) : null;
  const related = result?.relationships.filter((edge) => edge.id === presentation.selectedRelationshipId || edge.fromId === presentation.selectedNodeId || edge.toId === presentation.selectedNodeId) ?? [];
  const changes = graph?.relationshipChanges ?? [];
  const displayedRelations = [...new Map([...changes, ...related].map((edge) => [edge.id, edge])).values()];
  const categoryOptions = ["all", ...new Set(result?.nodes.flatMap((node) => node.kind === "declaration" ? [structureDeclarationValue(node)?.nativeKind.category].filter((value): value is NonNullable<typeof value> => !!value) : []) ?? [])];
  return <section aria-label={t("title")} className="structure-view flex h-full min-h-0 min-w-0 flex-col overflow-hidden rounded-lg border bg-background">
    <header className="flex shrink-0 flex-wrap items-center gap-2 border-b p-3">
      <Input aria-label={t("search")} placeholder={t("search")} value={presentation.query} onChange={(event) => { setInspectorOpen(Boolean(event.target.value)); update({ query: event.target.value, displayLimit: 60 }); }} className="min-w-40 max-w-sm flex-1" />
      <details className="structure-filters"><summary className="cursor-pointer text-xs text-muted-foreground">{t("filters")}</summary><div className="mt-2 flex flex-wrap gap-2">
      <GraphSelect label={t("category")} value={presentation.filters.category} options={categoryOptions.map((value) => ({ value, label: value === "all" ? t("category") : value }))} onChange={(value) => update({ filters: { ...presentation.filters, category: value as typeof presentation.filters.category } })} />
      <GraphSelect label={t("change")} value={presentation.filters.change} options={["all", "added", "deleted", "modified", "renamed", "moved", "documentation", "context", "incomplete"].map((value) => ({ value, label: value === "all" ? t("change") : t(value) }))} onChange={(value) => update({ filters: { ...presentation.filters, change: value as typeof presentation.filters.change } })} />

        <GraphSelect label={t("calls")} value={presentation.filters.calls} options={["selected", "graph", "hidden"].map((value) => ({ value, label: t(value) }))} onChange={(value) => update({ filters: { ...presentation.filters, calls: value as typeof presentation.filters.calls } })} />
        {(["inheritance", "implementations", "typeReferences", "imports", "candidates", "external"] as const).map((key) => <Toggle key={key} size="sm" variant="outline" pressed={presentation.filters[key]} onPressedChange={(pressed) => update({ filters: { ...presentation.filters, [key]: pressed } })}>{t(key)}</Toggle>)}
      </div></details>
    </header>
    <div role="status" className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-1 border-b px-3 py-2 text-xs">
      <span>{t(analysis.status)}</span>
      {analysis.progress && <span>{t("progress", { phase: t(analysis.progress.phase), processed: analysis.progress.processedFiles, total: analysis.progress.totalFiles ?? t("unknownTotal") })}</span>}
      {result && <><span>{t("summary", { declarations: result.summary.changedDeclarations, relationships: result.summary.relationshipChanges })}</span>{!result.summary.complete && <span className="text-amber-700 dark:text-amber-300">{t("partial")}</span>}</>}
      {analysis.status === "running" && onCancel && <Button size="sm" variant="outline" onClick={onCancel}>{t("cancel")}</Button>}
      {(analysis.status === "failed" || analysis.status === "cancelled" || analysis.status === "idle" || (result && !result.summary.complete && analysis.status !== "running")) && onRetry && <Button size="sm" variant="outline" onClick={onRetry}>{t("retry")}</Button>}
    </div>
    {analysis.error && <p role="alert" className="p-3 text-sm text-destructive">{analysis.error}</p>}
    <div className="structure-workspace min-h-0 flex-1">
      <div className="flex min-h-0 min-w-0 flex-col">
        <div className="structure-toolbar flex shrink-0 flex-wrap gap-1 border-b bg-background/95 p-1">
          {result && <Button size="sm" variant={inspectorOpen || selected ? "secondary" : "outline"} className="structure-details-toggle shrink-0" aria-expanded={inspectorOpen} aria-controls={inspectorId} onClick={() => setInspectorOpen((open) => !open)}>{t("details")}{result.summary.relationshipChanges > 0 && <span> · {result.summary.relationshipChanges}</span>}{selected && <span aria-hidden="true"> ●</span>}</Button>}
          <Button size="sm" variant="ghost" disabled={!selected} onClick={() => result && onPresentationChange(expandStructureNeighbors(result, presentation, "upstream"))}>{t("upstream")}</Button>
          <Button size="sm" variant="ghost" disabled={!selected} onClick={() => result && onPresentationChange(expandStructureNeighbors(result, presentation, "downstream"))}>{t("downstream")}</Button>
          <Button size="sm" variant="ghost" onClick={() => { cameraMode.current = "auto"; update({ viewportMode: "auto" }); scheduleFit(); }}>{t("fit")}</Button>
          <Button size="sm" variant="ghost" aria-label={t("zoomIn")} onClick={() => { cameraMode.current = "manual"; update({ viewportMode: "manual" }); void flow.current?.zoomIn(); }}>＋</Button>
          <Button size="sm" variant="ghost" aria-label={t("zoomOut")} onClick={() => { cameraMode.current = "manual"; update({ viewportMode: "manual" }); void flow.current?.zoomOut(); }}>−</Button>
          <Button size="sm" variant="ghost" onClick={() => { cameraMode.current = "auto"; onPresentationChange({ ...createStructureGraphPresentation(result), filters: presentation.filters }); scheduleFit(); }}>{t("reset")}</Button>
        </div>
        <div ref={canvasRef} className="structure-flow-canvas relative min-h-0 flex-1">
        <ReactFlow<CanvasNode> nodes={nodes} edges={edges} nodeTypes={nodeTypes} colorMode={colorMode} nodesDraggable={false} nodesConnectable={false} deleteKeyCode={null} minZoom={0.02} maxZoom={2} fitView={!presentation.viewport} fitViewOptions={{ padding: 0.12, minZoom: 0.02, maxZoom: 1 }} viewport={presentation.viewport ?? undefined}
          onInit={(instance) => { flow.current = instance; scheduleFit(); }} onMoveStart={(event) => { if (event) cameraMode.current = "manual"; }} onViewportChange={(viewport) => update({ viewport, viewportMode: cameraMode.current })}
          onNodeClick={(_event, node) => node.data.onOpen()} onEdgeClick={(_event, edge) => { update({ selectedRelationshipId: edge.id }); onNavigate({ kind: "relationship", relationshipId: edge.id }); }}>
          <Background />
        </ReactFlow>
        {nodes.length === 0 && <p className="pointer-events-none absolute inset-0 grid place-content-center px-4 text-center text-sm text-muted-foreground">{t(presentation.query ? "noMatches" : "empty")}</p>}
        </div>
      </div>
      {result && graph && <aside id={inspectorId} data-expanded={inspectorOpen} aria-label={t("selection")} className="structure-inspector min-w-0 space-y-4 overflow-auto border-l p-3 text-xs">
        <div><p>{t("analyzed", { count: result.nodes.length })}</p><p className="mt-1">{t("visible", { visible: graph.visibleCount, eligible: graph.eligibleCount, pending: graph.pendingCount })}</p>{graph.pendingCount > 0 && <Button className="mt-2" size="sm" variant="outline" onClick={() => update({ displayLimit: presentation.displayLimit + 60 })}>{t("more", { count: Math.min(60, graph.pendingCount) })}</Button>}</div>
        {presentation.query && <section><h3>{t("searchCount", { count: graph.searchResults.length })}</h3><ul className="mt-2 space-y-1">{graph.searchResults.slice(0, presentation.displayLimit).map((node) => <li key={node.id}><Button variant="link" className="block h-auto max-w-full truncate rounded-none border-0 p-0 text-left text-xs font-normal leading-normal text-foreground underline active:translate-y-0" title={structureNodeLabel(node, result)} onClick={() => { update({ selectedNodeId: node.id, revealedNodeIds: [...new Set([...presentation.revealedNodeIds, node.id])] }); onNavigate({ kind: "node", nodeId: node.id }); }}>{structureNodeLabel(node, result)}</Button></li>)}</ul></section>}
        {selected && <section className="space-y-2"><h3 className="break-words font-mono font-semibold">{structureNodeLabel(selected, result)}</h3>{selectedValue && <p className="break-words font-mono text-muted-foreground">{selectedValue.signature}</p>}{selected.kind === "external" && <p>{t("externalUnavailable")}</p>}
          {selected.kind === "declaration" && selected.diagnostics.map((diagnostic, index) => <p key={index} className="break-words text-muted-foreground">{diagnostic.message}</p>)}
          {selected.kind === "declaration" && (["before", "after"] as const).map((side) => { const fact = selected[side]; if (fact.state !== "present") return null; const parent = result.nodes.find((node) => node.id === fact.value.parentId); return <p key={side} className="break-words text-muted-foreground">{t(side)} → {parent ? structureNodeLabel(parent, result) : fact.value.parentId}</p>; })}
          {selected.kind === "declaration" && (["before", "after"] as const).flatMap((side) => selected[side].state === "present" ? selected[side].value.locations.map((location, index) => <Button key={`${side}-${index}`} size="sm" variant="outline" onClick={() => onNavigate({ kind: "location", location })}>{t(side)} · L{location.range.startLine}</Button>) : [])}
          {result.implementationGroups.filter((group) => (group.after.state === "present" && group.after.value.ownerId === selected.id) || (group.before.state === "present" && group.before.value.ownerId === selected.id)).map((group) => <Button key={group.id} variant="outline" size="sm" onClick={() => onNavigate({ kind: "implementation", groupId: group.id })}>{t("implementationSource")} · {group.after.state === "present" ? group.after.value.traitName ?? "impl" : "impl"}</Button>)}
        </section>}
        {displayedRelations.length > 0 && <section><h3 className="mb-2 font-semibold">{t(changes.length ? "relationChanges" : "relations")} ({displayedRelations.length})</h3><ul className="space-y-2">{displayedRelations.slice(0, presentation.displayLimit).map((edge) => <RelationshipCard key={edge.id} edge={edge} result={result} onNavigate={navigate} />)}</ul>{displayedRelations.length > presentation.displayLimit && <Button size="sm" variant="outline" onClick={() => update({ displayLimit: presentation.displayLimit + 60 })}>{t("more", { count: Math.min(60, displayedRelations.length - presentation.displayLimit) })}</Button>}</section>}
        {result.unresolvedRelationships.length > 0 && <details><summary>{t("unresolved")} ({result.unresolvedRelationships.length})</summary><ul className="space-y-2 pt-2">{result.unresolvedRelationships.slice(0, presentation.displayLimit).map((edge, index) => <li key={index}><Button variant="link" className="block h-auto max-w-full whitespace-normal break-words rounded-none border-0 p-0 text-left text-xs font-normal leading-normal text-foreground underline active:translate-y-0" onClick={() => onNavigate({ kind: "location", location: edge.occurrence.location })}>{edge.writtenTarget} · {t(edge.side)} · L{edge.occurrence.location.range.startLine}</Button>{edge.diagnostics.map((diagnostic, index) => <p key={index} className="break-words text-muted-foreground">{diagnostic.message}</p>)}</li>)}</ul>{result.unresolvedRelationships.length > presentation.displayLimit && <Button size="sm" variant="outline" onClick={() => update({ displayLimit: presentation.displayLimit + 60 })}>{t("more", { count: 60 })}</Button>}</details>}
        <details><summary>{t("coverage")}</summary><div className="space-y-2 pt-2"><p>{t("discovery")}: {t("before")} {t(result.coverage.discovery.before === "partial" ? "sourcePartial" : result.coverage.discovery.before)} / {t("after")} {t(result.coverage.discovery.after === "partial" ? "sourcePartial" : result.coverage.discovery.after)}</p>{result.coverage.diagnostics.map((diagnostic, index) => <p key={index} className="break-words">{diagnostic.message}</p>)}{result.coverage.files.slice(0, presentation.displayLimit).map((file, index) => <div key={index} className="break-words"><p>{t("sourceCoverage", { side: t(file.side), path: file.path, extraction: t(file.extraction === "partial" ? "sourcePartial" : file.extraction), relationships: t(file.relationships === "partial" ? "sourcePartial" : file.relationships) })}</p>{file.diagnostics.map((diagnostic, index) => <p key={index} className="text-muted-foreground">{diagnostic.message}</p>)}</div>)}{result.coverage.files.length > presentation.displayLimit && <Button size="sm" variant="outline" onClick={() => update({ displayLimit: presentation.displayLimit + 60 })}>{t("more", { count: 60 })}</Button>}</div></details>
      </aside>}
    </div>
    <footer aria-label={t("direction")} className="flex shrink-0 flex-wrap gap-x-4 gap-y-1 border-t px-3 py-2 text-[11px] text-muted-foreground"><span>{t("direction")}</span>{(["added", "deleted", "retained", "indeterminate"] as const).map((change, index) => <span key={change} className={`structure-change-${change}`}>{["+ →", "− →", "= →", "? →"][index]} {t(change)}</span>)}<span>┄◇→ {t("candidate")}</span></footer>
  </section>;
}
