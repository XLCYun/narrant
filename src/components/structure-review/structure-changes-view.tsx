import { createContext, useContext, useEffect, useMemo, useState } from "react";
import { Background, BaseEdge, Controls, Handle, Position, ReactFlow, getSmoothStepPath, type EdgeProps, type NodeProps } from "@xyflow/react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { StructureDetails, type StructureSelection } from "./structure-details";
import { Spinner } from "@/components/ui/spinner";
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import { layoutStructureComparison, type ObjectNode, type ObjectStatus, type StructureEdge, type StructureLayout } from "@/structure-review/graph-presentation";
import type { StructureBinding, StructureComparison } from "@/structure-review/types";
import "@xyflow/react/dist/style.css";
import "./structure-changes.css";

const ObjectActions = createContext<{ selectedId?: string; connectedPorts: ReadonlySet<string>; expand(id: string): void; open(id: string): void }>({ connectedPorts: new Set(), expand: () => {}, open: () => {} });
const statusClasses: Record<ObjectStatus, string> = {
  added: "structure-added", removed: "structure-removed", modified: "structure-modified",
  relations: "structure-relations", unchanged: "structure-unchanged",
};

/** 类图卡片用紧凑行展示字段和方法；每行保留原对象的详情入口与独立端口。 */
function CodeObjectNode({ data }: NodeProps<ObjectNode>) {
  const { t } = useTranslation("structure");
  const actions = useContext(ObjectActions);
  const selected = data.objectId !== undefined && actions.selectedId === data.objectId;
  const markers: Record<ObjectStatus, string> = { added: "+", removed: "−", modified: "~", relations: "↔", unchanged: "·" };
  return <div className={cn("structure-object", statusClasses[data.status], data.group && "structure-group", data.table && "structure-member-table", selected && "structure-selected")}
    data-object-id={data.objectId} data-object-status={data.status}>
    {!data.table && <div className="structure-object-header" style={{ height: data.headerHeight }}>
    <div className="flex items-center justify-between gap-2 text-[10px]">
      <span className="min-w-0 truncate text-muted-foreground">{data.nativeKind}</span>
      <span className="structure-status shrink-0">{t(`status.${data.status}`)}</span>
    </div>
    {data.objectId ? <Button variant="ghost" className="nodrag nopan mt-0.5 h-6 w-full justify-start overflow-hidden px-0 font-mono text-sm font-semibold hover:bg-transparent hover:underline"
      title={data.label} aria-pressed={selected} onClick={(event) => { event.stopPropagation(); actions.open(data.objectId!); }}><span className="truncate">{data.label}</span></Button>
      : <div className="mt-2 truncate font-mono text-sm font-medium" title={data.label}>{data.label}</div>}
    {data.hiddenChildren > 0 && data.objectId && <Button variant="ghost" size="sm" className="nodrag nopan mt-1 h-6 px-1 text-[10px]"
      onClick={(event) => { event.stopPropagation(); actions.expand(data.objectId!); }}>{t("expandChildren", { count: data.hiddenChildren })}</Button>}
    </div>}
    {data.rows.map((row, index) => row.kind === "section"
      ? <div key={`section-${index}`} className="structure-member-section" style={{ top: row.top, height: row.height }}>
        {t(`memberSections.${row.section === "values" ? data.category === "type" ? "attributes" : "values" : data.category === "type" ? "methods" : "functions"}`)}
      </div>
      : <div key={row.objectId} className={cn("structure-member-row", statusClasses[row.status], row.kind === "implementation" && "structure-implementation", actions.selectedId === row.objectId && "structure-selected")}
        style={{ top: row.top, height: row.height }} data-object-id={row.objectId} data-object-status={row.status}>
        <Button variant="ghost" className="nodrag nopan structure-member-button" title={`${row.label} · ${t(`status.${row.status}`)}`}
          aria-label={`${row.label} · ${t(`status.${row.status}`)}`} aria-pressed={actions.selectedId === row.objectId}
          onClick={(event) => { event.stopPropagation(); actions.open(row.objectId!); }}>
          <span className="structure-member-marker" aria-hidden>{markers[row.status]}</span><span className="truncate">{row.label}</span>
        </Button>
        {row.hiddenChildren > 0 && <Button variant="ghost" className="nodrag nopan h-6 shrink-0 px-2 text-[10px]"
          aria-label={t("expandChildren", { count: row.hiddenChildren })}
          onClick={(event) => { event.stopPropagation(); actions.expand(row.objectId!); }}>+{row.hiddenChildren}</Button>}
      </div>)}
    {data.ports.map((port) => <Handle key={port.id} id={port.id} type={port.side} position={port.side === "source" ? Position.Right : Position.Left}
      style={{ top: port.y, opacity: actions.connectedPorts.has(port.id) ? 1 : 0 }} isConnectable={false} />)}
  </div>;
}
const nodeTypes = { codeObject: CodeObjectNode };

/** 绘制布局算法的避让路径；跨版本的旧包含边使用普通正交路径。 */
function RoutedRelationEdge(props: EdgeProps<StructureEdge>) {
  const fallback = props.data?.route ? undefined : getSmoothStepPath(props);
  const route = props.data?.route ?? { path: fallback![0], labelX: fallback![1], labelY: fallback![2] };
  return <BaseEdge id={props.id} path={route.path} markerEnd={props.markerEnd} style={props.style}
    label={props.label} labelX={route.labelX} labelY={route.labelY} labelStyle={props.labelStyle} labelBgStyle={props.labelBgStyle} />;
}
const edgeTypes = { routedRelation: RoutedRelationEdge };

/** loading、失败、取消与已证明的空 diff 使用不同状态，避免把分析失败当作无变化。 */
export default function StructureChangesView({ binding, dark }: { binding?: StructureBinding; dark: boolean }) {
  const { t } = useTranslation(["structure", "common"]);
  const state = binding?.state;
  if (state?.kind === "ready") return <ComparisonView key={JSON.stringify([state.comparison.reviewId, state.comparison.snapshotId])} comparison={state.comparison} dark={dark} />;
  const loading = state?.kind === "loading" || state?.kind === "idle";
  return <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 overflow-auto p-6 text-center" role={state?.kind === "failed" ? "alert" : "status"}>
    {loading && <Spinner className="size-6" />}
    <h3 className="font-medium">{t(!state ? "unavailable" : loading ? "loading" : state.kind === "cancelled" ? "cancelled" : "failed")}</h3>
    <p className="max-w-xl whitespace-pre-wrap break-words text-sm text-muted-foreground">{state?.kind === "failed" ? state.detail : loading ? t("loadingDescription") : t("retryDescription")}</p>
    {binding && <Button variant="outline" onClick={loading ? binding.cancel : binding.retry}>{loading ? t("common:cancel") : t("common:retry")}</Button>}
  </div>;
}

/** 只读 combined graph，布局、展开和选中状态留在展示层。 */
function ComparisonView({ comparison, dark }: { comparison: StructureComparison; dark: boolean }) {
  const { t } = useTranslation("structure");
  const [showAll, setShowAll] = useState(false);
  const [showUnchangedReferences, setShowUnchangedReferences] = useState(false);
  const [showImports, setShowImports] = useState(false);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [selection, setSelection] = useState<StructureSelection>();
  const [layout, setLayout] = useState<StructureLayout>();
  const [layoutVersion, setLayoutVersion] = useState(0);
  const [layoutError, setLayoutError] = useState<string>();
  const [layingOut, setLayingOut] = useState(true);
  const selectedId = selection?.kind === "object" ? selection.id : undefined;
  // 按原对象端口筛选双向关系；同一类卡片中的其他成员不会一并展开连线。
  const visibleEdges = useMemo(() => {
    if (!layout || selectedId === undefined) return [];
    const ports = new Set(layout.nodes.flatMap((node) => node.data.ports.filter((port) => port.objectId === selectedId).map((port) => port.id)));
    return layout.edges.filter((edge) => ports.has(edge.sourceHandle ?? "") || ports.has(edge.targetHandle ?? ""));
  }, [layout, selectedId]);
  const connectedPorts = useMemo(() => new Set(visibleEdges.flatMap((edge) => [edge.sourceHandle, edge.targetHandle]).filter((id): id is string => typeof id === "string")), [visibleEdges]);
  const options = useMemo(() => ({ showAll, showUnchangedReferences, showImports, expanded }), [expanded, showAll, showUnchangedReferences, showImports]);
  useEffect(() => {
    let current = true;
    const abort = new AbortController();
    setLayingOut(true);
    setLayoutError(undefined);
    void layoutStructureComparison(comparison, options, abort.signal).then((result) => {
      if (current) { setLayout(result); setLayoutVersion((version) => version + 1); setLayingOut(false); }
    }).catch((reason: unknown) => {
      if (current) { setLayoutError(String(reason)); setLayingOut(false); }
    });
    return () => { current = false; abort.abort(); };
  }, [comparison, options]);
  const analyzed = comparison.coverage.filter((file) => file.before === "analyzed" || file.after === "analyzed").length;
  const partial = comparison.coverage.some((file) => file.before.startsWith("unsupported") || file.after.startsWith("unsupported"));
  const changed = layout?.changed ?? 0;
  const emptyState = analyzed === 0 && comparison.coverage.length > 0 ? "unsupported"
    : comparison.diff.nodes.length > 0 ? "filtered" : "empty";
  return <div className="structure-view flex min-h-0 flex-1 flex-col" aria-label={t("title")}>
    <div className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-2 border-b px-3 py-2">
      <div className="mr-auto flex min-w-0 flex-wrap items-center gap-2 text-xs">
        <span className="font-medium">{t("changedCount", { count: changed })}</span>
        <span className="text-muted-foreground">{t("visibleCount", { visible: layout?.visible ?? 0, total: layout?.total ?? 0 })}</span>
        <Button size="sm" variant="ghost" className="h-7 px-2 text-xs" onClick={() => setSelection({ kind: "coverage" })}>
          {t(partial ? "partialCoverage" : "coverage", { count: analyzed, total: comparison.coverage.length })}
        </Button>
      </div>
      <DropdownMenu>
        <DropdownMenuTrigger render={<Button variant="ghost" size="sm" className="h-7 text-xs" />}>{t("displayOptions")}</DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-64 max-w-[calc(100vw-2rem)]">
          <DropdownMenuCheckboxItem checked={showAll} onCheckedChange={setShowAll}>{t("showAll")}</DropdownMenuCheckboxItem>
          <DropdownMenuCheckboxItem checked={showUnchangedReferences} onCheckedChange={setShowUnchangedReferences}>{t("showUnchangedReferences")}</DropdownMenuCheckboxItem>
          <DropdownMenuSeparator />
          <DropdownMenuCheckboxItem checked={showImports} onCheckedChange={setShowImports}>{t("showImports")}</DropdownMenuCheckboxItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
    <ResizablePanelGroup orientation="horizontal" className="h-auto! min-h-0 flex-1">
      <ResizablePanel id="structure-details" defaultSize="320px" minSize="180px" maxSize="55%" groupResizeBehavior="preserve-pixel-size">
        <StructureDetails comparison={comparison} selection={selection} showImports={showImports} />
      </ResizablePanel>
      <ResizableHandle withHandle aria-label={t("resizeDetails")} />
      <ResizablePanel id="structure-graph" minSize="45%">
        <div className="relative flex h-full min-h-0 min-w-0 flex-col">
          <div className="relative min-h-0 flex-1" data-testid="structure-canvas">
            {layoutError ? <div role="alert" className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
              <h3>{t("layoutFailed")}</h3><p className="max-w-xl break-words text-xs text-muted-foreground">{layoutError}</p>
              <Button variant="outline" onClick={() => setExpanded(new Set(expanded))}>{t("retryLayout")}</Button>
            </div> : layout && layout.nodes.length > 0 ? <ObjectActions.Provider value={{ selectedId, connectedPorts, expand: (id) => setExpanded((current) => new Set([...current, id])), open: (id) => setSelection({ kind: "object", id }) }}>
              <ReactFlow<ObjectNode, StructureEdge> key={layoutVersion} nodes={layout.nodes} edges={visibleEdges} nodeTypes={nodeTypes} edgeTypes={edgeTypes}
                colorMode={dark ? "dark" : "light"} fitView fitViewOptions={{ padding: 0.16, maxZoom: 1 }} minZoom={0.08} maxZoom={2}
                nodesDraggable={false} nodesConnectable={false} nodesFocusable={false} edgesReconnectable={false} deleteKeyCode={null}
                onNodeClick={(_, node) => { if (node.data.objectId) setSelection({ kind: "object", id: node.data.objectId }); }}
                onPaneClick={() => setSelection(undefined)}
                ariaLabelConfig={{ "controls.zoomIn.ariaLabel": t("zoomIn"), "controls.zoomOut.ariaLabel": t("zoomOut"), "controls.fitView.ariaLabel": t("fitView") }}>
                <Background gap={20} size={1} />
                <Controls showInteractive={false} />
              </ReactFlow>
            </ObjectActions.Provider> : !layingOut && <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center" role="status">
              <h3 className="font-medium">{t(`${emptyState}Title`)}</h3>
              <p className="max-w-md text-sm text-muted-foreground">{t(`${emptyState}Description`)}</p>
            </div>}
            {layingOut && <div className="absolute inset-0 z-10 flex items-center justify-center gap-2 bg-background/80 text-sm" role="status"><Spinner />{t("layoutLoading")}</div>}
          </div>
          <div className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-1 border-t px-3 py-2 text-[11px] text-muted-foreground" aria-label={t("legend")}>
            {(["added", "removed", "modified", "relations", "unchanged"] as const).map((status) => <span key={status} className={cn("structure-legend", statusClasses[status])}><span aria-hidden className="structure-dot" />{t(`status.${status}`)}</span>)}
            <span className="ml-auto">{t(selectedId === undefined ? "selectRelations" : "relationLegend")}</span>
          </div>
        </div>
      </ResizablePanel>
    </ResizablePanelGroup>
  </div>;
}
