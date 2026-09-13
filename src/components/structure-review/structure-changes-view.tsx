import { createContext, useContext, useEffect, useMemo, useState } from "react";
import { Background, BaseEdge, Controls, Handle, Position, ReactFlow, getSmoothStepPath, type EdgeProps, type NodeProps } from "@xyflow/react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Sheet, SheetContent, SheetDescription, SheetHeader, SheetTitle } from "@/components/ui/sheet";
import { Spinner } from "@/components/ui/spinner";
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import { isReferenceVisible, layoutStructureComparison, objectDetails, type ObjectNode, type ObjectStatus, type StructureEdge, type StructureLayout } from "@/structure-review/graph-presentation";
import type { CodeNode, CodeReference, StructureBinding, StructureComparison } from "@/structure-review/types";
import "@xyflow/react/dist/style.css";
import "./structure-changes.css";

const ObjectActions = createContext<{ expand(id: string): void; open(id: string): void }>({ expand: () => {}, open: () => {} });
const statusClasses: Record<ObjectStatus, string> = {
  added: "structure-added", removed: "structure-removed", modified: "structure-modified",
  relations: "structure-relations", unchanged: "structure-unchanged",
};

/** 类图卡片用紧凑行展示字段和方法；每行保留原对象的详情入口与独立端口。 */
function CodeObjectNode({ data, selected }: NodeProps<ObjectNode>) {
  const { t } = useTranslation("structure");
  const actions = useContext(ObjectActions);
  const markers: Record<ObjectStatus, string> = { added: "+", removed: "−", modified: "~", relations: "↔", unchanged: "·" };
  return <div className={cn("structure-object", statusClasses[data.status], data.group && "structure-group", data.table && "structure-member-table", selected && "structure-selected")}
    data-object-id={data.objectId} data-object-status={data.status}>
    {!data.table && <div className="structure-object-header" style={{ height: data.headerHeight }}>
    <div className="flex items-center justify-between gap-2 text-[10px]">
      <span className="min-w-0 truncate text-muted-foreground">{data.nativeKind}</span>
      <span className="structure-status shrink-0">{t(`status.${data.status}`)}</span>
    </div>
    {data.objectId ? <Button variant="ghost" className="nodrag nopan mt-0.5 h-6 w-full justify-start overflow-hidden px-0 font-mono text-sm font-semibold hover:bg-transparent hover:underline"
      title={data.label} onClick={(event) => { event.stopPropagation(); actions.open(data.objectId!); }}><span className="truncate">{data.label}</span></Button>
      : <div className="mt-2 truncate font-mono text-sm font-medium" title={data.label}>{data.label}</div>}
    {data.hiddenChildren > 0 && data.objectId && <Button variant="ghost" size="sm" className="nodrag nopan mt-1 h-6 px-1 text-[10px]"
      onClick={(event) => { event.stopPropagation(); actions.expand(data.objectId!); }}>{t("expandChildren", { count: data.hiddenChildren })}</Button>}
    </div>}
    {data.rows.map((row, index) => row.kind === "section"
      ? <div key={`section-${index}`} className="structure-member-section" style={{ top: row.top, height: row.height }}>
        {t(`memberSections.${row.section === "values" ? data.category === "type" ? "attributes" : "values" : data.category === "type" ? "methods" : "functions"}`)}
      </div>
      : <div key={row.objectId} className={cn("structure-member-row", statusClasses[row.status], row.kind === "implementation" && "structure-implementation")}
        style={{ top: row.top, height: row.height }} data-object-id={row.objectId} data-object-status={row.status}>
        <Button variant="ghost" className="nodrag nopan structure-member-button" title={`${row.label} · ${t(`status.${row.status}`)}`}
          aria-label={`${row.label} · ${t(`status.${row.status}`)}`}
          onClick={(event) => { event.stopPropagation(); actions.open(row.objectId!); }}>
          <span className="structure-member-marker" aria-hidden>{markers[row.status]}</span><span className="truncate">{row.label}</span>
        </Button>
        {row.hiddenChildren > 0 && <Button variant="ghost" className="nodrag nopan h-6 shrink-0 px-2 text-[10px]"
          aria-label={t("expandChildren", { count: row.hiddenChildren })}
          onClick={(event) => { event.stopPropagation(); actions.expand(row.objectId!); }}>+{row.hiddenChildren}</Button>}
      </div>)}
    {data.ports.map((port) => <Handle key={port.id} id={port.id} type={port.side} position={port.side === "source" ? Position.Right : Position.Left}
      style={{ top: port.y, opacity: port.connected ? 1 : 0 }} isConnectable={false} />)}
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
  const [selection, setSelection] = useState<{ kind: "object"; id: string } | { kind: "coverage" }>();
  const [layout, setLayout] = useState<StructureLayout>();
  const [layoutVersion, setLayoutVersion] = useState(0);
  const [layoutError, setLayoutError] = useState<string>();
  const [layingOut, setLayingOut] = useState(true);
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
  const details = selection?.kind === "object" ? objectDetails(comparison, selection.id) : undefined;
  const selected = details?.after ?? details?.before;
  const referenceChanges = details?.difference?.references.filter((change) => isReferenceVisible(change.value, options)) ?? [];
  const selectedReferences = selected?.references.filter((reference) => isReferenceVisible(reference, options)) ?? [];
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
    <div className="relative min-h-0 flex-1" data-testid="structure-canvas">
      {layoutError ? <div role="alert" className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
        <h3>{t("layoutFailed")}</h3><p className="max-w-xl break-words text-xs text-muted-foreground">{layoutError}</p>
        <Button variant="outline" onClick={() => setExpanded(new Set(expanded))}>{t("retryLayout")}</Button>
      </div> : layout && layout.nodes.length > 0 ? <ObjectActions.Provider value={{ expand: (id) => setExpanded((current) => new Set([...current, id])), open: (id) => setSelection({ kind: "object", id }) }}>
        <ReactFlow<ObjectNode, StructureEdge> key={layoutVersion} nodes={layout.nodes} edges={layout.edges} nodeTypes={nodeTypes} edgeTypes={edgeTypes}
          colorMode={dark ? "dark" : "light"} fitView fitViewOptions={{ padding: 0.16, maxZoom: 1 }} minZoom={0.08} maxZoom={2}
          nodesDraggable={false} nodesConnectable={false} nodesFocusable={false} edgesReconnectable={false} deleteKeyCode={null}
          onNodeClick={(_, node) => { if (node.data.objectId) setSelection({ kind: "object", id: node.data.objectId }); }}
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
      <span className="ml-auto">{t("relationLegend")}</span>
    </div>
    <Sheet open={Boolean(selection)} onOpenChange={(open) => { if (!open) setSelection(undefined); }}>
      <SheetContent className="w-full! sm:max-w-2xl!">
        <SheetHeader className="shrink-0 pr-14">
          <SheetTitle>{t(selection?.kind === "coverage" ? "coverageTitle" : "detailsTitle")}</SheetTitle>
          <SheetDescription>{t(selection?.kind === "coverage" ? "coverageDescription" : "detailsDescription")}</SheetDescription>
        </SheetHeader>
        <div className="min-h-0 flex-1 overflow-auto px-6 pb-6">
          {selection?.kind === "coverage" ? <ul className="space-y-3">{comparison.coverage.map((file) => <li key={file.path} className="rounded-md border p-3 text-xs">
            <p className="break-all font-mono">{file.oldPath && file.oldPath !== file.path ? `${file.oldPath} → ${file.path}` : file.path}</p>
            <p className="mt-2 text-muted-foreground">{t("before")} · {t(`coverageKind.${file.before}`)}<br />{t("after")} · {t(`coverageKind.${file.after}`)}</p>
          </li>)}</ul> : selected && details && <>
            <h3 className="break-words font-mono text-sm font-medium">{selected.name ?? selected.signature ?? selected.nativeKind}</h3>
            <p className="mt-1 text-xs text-muted-foreground">{selected.language} · {selected.nativeKind} · {t(`status.${details.difference?.content ?? (details.difference ? "relations" : "unchanged")}`)}</p>
            <div className="mt-5 grid gap-4 sm:grid-cols-2">
              <DeclarationSide title={t("before")} node={details.before} />
              <DeclarationSide title={t("after")} node={details.after} />
            </div>
            <h4 className="mb-2 mt-6 text-sm font-medium">{t("relationshipChanges")}</h4>
            {(details.difference?.contains.length ?? 0) + referenceChanges.length === 0
              ? <p className="text-xs text-muted-foreground">{t("noRelationshipChanges")}</p>
              : <ul className="space-y-2 text-xs">{details.difference?.contains.map((change) => <li key={`${change.kind}-${change.value}`} className="break-words"><span className="font-medium">{t(`status.${change.kind}`)}</span> · contains → {objectName(comparison, change.value)}</li>)}
                {referenceChanges.map((change, index) => <li key={index} className="break-words"><span className="font-medium">{t(`status.${change.kind}`)}</span> · {referenceLabel(comparison, change.value)}</li>)}
              </ul>}
            <h4 className="mb-2 mt-6 text-sm font-medium">{t("referencesTitle")}</h4>
            {selectedReferences.length ? <ul className="space-y-2 break-words text-xs text-muted-foreground">{selectedReferences.map((reference, index) => <li key={index}>{referenceLabel(comparison, reference)}</li>)}</ul> : <p className="text-xs text-muted-foreground">{t("noVisibleReferences")}</p>}
          </>}
        </div>
      </SheetContent>
    </Sheet>
  </div>;
}

/** 分侧显示源码；不存在的节点与没有实现内容的声明使用不同提示。 */
function DeclarationSide({ title, node }: { title: string; node?: CodeNode }) {
  const { t } = useTranslation("structure");
  return <section className="min-w-0"><h4 className="mb-2 text-xs font-medium">{title}</h4>
    {!node ? <p className="text-xs text-muted-foreground">{t("absent")}</p> : <>
      <p className="mb-2 break-words text-xs text-muted-foreground">{node.name ?? node.nativeKind} · {node.language} · {node.nativeKind}</p>
      <pre className="overflow-auto rounded-md border bg-muted/30 p-3 text-[11px] leading-relaxed">{node.signature ?? t("noSignature")}</pre>
      <h5 className="mb-2 mt-3 text-xs text-muted-foreground">{t("ownCode")}</h5>
      <pre className="max-h-80 overflow-auto rounded-md border bg-muted/30 p-3 text-[11px] leading-relaxed">{node.code ?? t("noCode")}</pre>
      {Object.keys(node.extensions).length > 0 && <><h5 className="mb-2 mt-3 text-xs text-muted-foreground">{t("languageDetails")}</h5><pre className="max-h-60 overflow-auto rounded-md border bg-muted/30 p-3 text-[11px]">{JSON.stringify(node.extensions, null, 2)}</pre></>}
    </>}
  </section>;
}

/** 按节点身份读取显示名，不从 ID 编码制造源文件位置。 */
function objectName(comparison: StructureComparison, id: string): string {
  const node = comparison.after.nodes.find((item) => item.id === id) ?? comparison.before.nodes.find((item) => item.id === id);
  return node?.name ?? node?.signature ?? node?.nativeKind ?? id;
}

/** 详情只格式化已经通过展示过滤的图内引用。 */
function referenceLabel(comparison: StructureComparison, reference: CodeReference): string {
  const target = reference.target;
  return target.kind === "node" ? `${reference.semantic} → ${objectName(comparison, target.id)}` : "";
}
