import { createContext, useContext, useEffect, useId, useMemo, useState } from "react";
import { Background, BaseEdge, Controls, Handle, Position, ReactFlow, getSmoothStepPath, useUpdateNodeInternals, type EdgeProps, type NodeProps } from "@xyflow/react";
import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { StructureDetails } from "./structure-details";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { DropdownMenu, DropdownMenuCheckboxItem, DropdownMenuContent, DropdownMenuItem, DropdownMenuSeparator, DropdownMenuTrigger } from "@/components/ui/dropdown-menu";
import { cn } from "@/lib/utils";
import { collapseStructureLayout, layoutStructureComparison, relayoutStructureLayout, selectStructureEdges, type ObjectNode, type ObjectStatus, type StructureEdge, type StructureLayout, type StructureSelection } from "@/structure-review/graph-presentation";
import type { StructureBinding, StructureComparison } from "@/structure-review/types";
import { comparisonForLanguage, structureLanguages } from "@/structure-review/language-view";
import "@xyflow/react/dist/style.css";
import "./structure-changes.css";

const ObjectActions = createContext<{
  selectedId?: string; selectedDirectory?: string; connectedPorts: ReadonlySet<string>;
  expand(id: string): void; toggleCollapse(id: string): void; open(id: string): void;
  toggleDirectory(path: string): void; openDirectory(path: string): void;
}>({ connectedPorts: new Set(), expand: () => {}, toggleCollapse: () => {}, open: () => {}, toggleDirectory: () => {}, openDirectory: () => {} });
const statusClasses: Record<ObjectStatus, string> = {
  added: "structure-added", removed: "structure-removed", modified: "structure-modified",
  relations: "structure-relations", unchanged: "structure-unchanged",
};

/** 类图卡片用紧凑行展示字段和方法；每行保留原对象的详情入口与独立端口。 */
function CodeObjectNode({ id, data }: NodeProps<ObjectNode>) {
  const { t } = useTranslation("structure");
  const actions = useContext(ObjectActions);
  const updateNodeInternals = useUpdateNodeInternals();
  useEffect(() => { updateNodeInternals(id); }, [data.ports, id, updateNodeInternals]);
  const selected = data.directory ? actions.selectedDirectory === data.directory.path : data.objectId !== undefined && actions.selectedId === data.objectId;
  const title = data.filePath ?? data.directory?.path ?? data.label;
  const markers: Record<ObjectStatus, string> = { added: "+", removed: "−", modified: "~", relations: "↔", unchanged: "·" };
  return <div className={cn("structure-object", statusClasses[data.status], data.group && "structure-group", data.directory && "structure-directory", data.table && "structure-member-table", selected && "structure-selected")}
    data-object-id={data.objectId} data-directory-path={data.directory?.path} data-object-status={data.directory ? undefined : data.status}>
    {!data.table && <div className="structure-object-header" style={{ height: data.headerHeight }}>
    <div className="flex items-center justify-between gap-2 text-[10px]">
      <span className="min-w-0 truncate text-muted-foreground">{data.directory ? t("directory") : data.filePath ? t("file") : data.nativeKind}</span>
      {!data.directory && <span className="structure-status shrink-0">{t(`status.${data.status}`)}</span>}
    </div>
    <div className="mt-0.5 flex min-w-0 items-center gap-1">
    {data.collapse && (data.objectId !== undefined || data.directory) && <Button variant="ghost" size="icon" className="nodrag nopan size-6 shrink-0 text-sm"
      aria-expanded={!data.collapse.collapsed} aria-label={t(data.collapse.collapsed ? "expandObject" : "collapseObject", { name: title })}
      title={t(data.collapse.collapsed ? "expandObject" : "collapseObject", { name: title })}
      onClick={(event) => { event.stopPropagation(); if (data.directory) actions.toggleDirectory(data.directory.path); else actions.toggleCollapse(data.objectId!); }}><span aria-hidden>{data.collapse.collapsed ? "▸" : "▾"}</span></Button>}
    {data.objectId !== undefined || data.directory ? <Button variant="ghost" className="nodrag nopan h-6 min-w-0 flex-1 justify-start overflow-hidden px-0 font-mono text-sm font-semibold hover:bg-transparent hover:underline"
      title={title} aria-label={title} aria-pressed={selected} onClick={(event) => { event.stopPropagation(); if (data.directory) actions.openDirectory(data.directory.path); else actions.open(data.objectId!); }}><span className="truncate">{data.label}</span></Button>
      : <div className="mt-2 truncate font-mono text-sm font-medium" title={data.label}>{data.label}</div>}
    </div>
    {data.collapse?.collapsed && <div className="mt-1 flex min-w-0 items-center gap-2 text-[10px] text-muted-foreground" data-testid="collapse-summary">
      <span className="truncate">{data.directory ? t("collapsedDirectorySummary", { count: data.directory.files.length }) : data.category === "type" && data.collapse.values + data.collapse.callables > 0
        ? t("collapsedTypeSummary", { values: data.collapse.values, callables: data.collapse.callables })
        : t("collapsedSummary", { count: data.collapse.count })}</span>
      {data.collapse.changed > 0 && <span className="shrink-0 font-medium text-foreground">{t("collapsedChanges", { count: data.collapse.changed })}</span>}
    </div>}
    {!data.collapse?.collapsed && data.hiddenChildren > 0 && data.objectId && <Button variant="ghost" size="sm" className="nodrag nopan mt-1 h-6 px-1 text-[10px]"
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
  const { t } = useTranslation("structure");
  const fallback = props.data?.route ? undefined : getSmoothStepPath(props);
  const route = props.data?.route ?? { path: fallback![0], labelX: fallback![1], labelY: fallback![2] };
  const names = props.data?.summary ? t("summarizedRelations", props.data.summary)
    : [props.data?.collapsedSource, props.data?.collapsedTarget].filter(Boolean).join(" → ");
  return <g data-collapsed-endpoints={names || undefined}>
    {names && <title>{names}</title>}
    <BaseEdge id={props.id} path={route.path} markerEnd={props.markerEnd} style={props.style}
      label={props.label} labelX={route.labelX} labelY={route.labelY} labelStyle={props.labelStyle} labelBgStyle={props.labelBgStyle} />
  </g>;
}
const edgeTypes = { routedRelation: RoutedRelationEdge };
const languageNames: Record<string, string> = { rust: "Rust", typescript: "TypeScript", javascript: "JavaScript" };

/** 分析范围开关在加载、失败和空图时同样可用，切换由应用层重新分析两侧。 */
function TestCodeSwitch({ binding }: { binding: StructureBinding }) {
  const { t } = useTranslation("structure");
  const id = useId();
  return <div className="flex shrink-0 items-center gap-2 text-xs" title={t("includeTestsDescription")}>
    <Switch id={id} size="sm" checked={binding.options.includeTests}
      onCheckedChange={(includeTests) => binding.setOptions({ ...binding.options, includeTests })}
      aria-description={t("includeTestsDescription")} />
    <label htmlFor={id} className="cursor-pointer">{t("includeTests")}</label>
  </div>;
}

/** loading、失败、取消与已证明的空 diff 使用不同状态，避免把分析失败当作无变化。 */
export default function StructureChangesView({ binding, dark }: { binding?: StructureBinding; dark: boolean }) {
  const { t } = useTranslation(["structure", "common"]);
  const state = binding?.state;
  if (state?.kind === "ready" && binding) return <ComparisonView key={JSON.stringify([state.comparison.reviewId, state.comparison.snapshotId, binding.options.includeTests])} comparison={state.comparison} binding={binding} dark={dark} />;
  const loading = state?.kind === "loading" || state?.kind === "idle";
  return <div className="flex min-h-0 flex-1 flex-col">
    {binding && <div className="flex shrink-0 justify-end border-b px-3 py-3"><TestCodeSwitch binding={binding} /></div>}
    <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-3 overflow-auto p-6 text-center" role={state?.kind === "failed" ? "alert" : "status"}>
    {loading && <Spinner className="size-6" />}
    <h3 className="font-medium">{t(!state ? "unavailable" : loading ? "loading" : state.kind === "cancelled" ? "cancelled" : "failed")}</h3>
    {!loading && <>
      <p className="max-w-xl whitespace-pre-wrap break-words text-sm text-muted-foreground">{state?.kind === "failed" ? state.detail : t("retryDescription")}</p>
      {binding && <Button variant="outline" onClick={binding.retry}>{t("common:retry")}</Button>}
    </>}
    </div>
  </div>;
}

/** 只读 combined graph，布局、展开和选中状态留在展示层。 */
function ComparisonView({ comparison: fullComparison, binding, dark }: { comparison: StructureComparison; binding: StructureBinding; dark: boolean }) {
  const { t } = useTranslation("structure");
  const languages = useMemo(() => structureLanguages(fullComparison), [fullComparison]);
  const [preferredLanguage, setPreferredLanguage] = useState<string>();
  const language = preferredLanguage && languages.includes(preferredLanguage) ? preferredLanguage : languages[0];
  const comparison = useMemo(() => comparisonForLanguage(fullComparison, language), [fullComparison, language]);
  const [showAll, setShowAll] = useState(false);
  const [showUnchangedReferences, setShowUnchangedReferences] = useState(false);
  const [showImports, setShowImports] = useState(false);
  const [showDetailedRelations, setShowDetailedRelations] = useState(false);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [collapsedDirectories, setCollapsedDirectories] = useState<ReadonlySet<string>>(new Set());
  const [relayoutOnCollapse, setRelayoutOnCollapse] = useState(false);
  const [selection, setSelection] = useState<StructureSelection>();
  const selectedId = selection?.kind === "object" ? selection.id : undefined;
  const selectedDirectory = selection?.kind === "directory" ? selection.path : undefined;
  const [layoutResult, setLayoutResult] = useState<{ comparison: StructureComparison; layout: StructureLayout }>();
  const expandedLayout = layoutResult?.comparison === comparison ? layoutResult.layout : undefined;
  const foldedLayout = useMemo(() => expandedLayout && collapseStructureLayout(expandedLayout, collapsed, collapsedDirectories), [expandedLayout, collapsed, collapsedDirectories]);
  const [compactResult, setCompactResult] = useState<{ input: StructureLayout; layout?: StructureLayout; error?: string }>();
  const needsCompactLayout = relayoutOnCollapse && foldedLayout !== undefined && foldedLayout !== expandedLayout;
  const currentCompact = compactResult?.input === foldedLayout ? compactResult : undefined;
  const layout = needsCompactLayout ? currentCompact?.layout ?? foldedLayout : foldedLayout;
  const directory = expandedLayout?.nodes.find((node) => node.data.directory?.path === selectedDirectory)?.data.directory;
  const [layoutError, setLayoutError] = useState<string>();
  const [layingOut, setLayingOut] = useState(true);
  const visibleLayoutError = layoutError ?? (needsCompactLayout ? currentCompact?.error : undefined);
  const arranging = layingOut || (needsCompactLayout && !currentCompact);
  // 容器按全展开的展示层级继承后代关系；叶成员仍只显示自身的双向关系。
  const visibleEdges = useMemo(() => {
    if (!layout || !expandedLayout) return [];
    return selectStructureEdges(expandedLayout, layout, selection, showDetailedRelations ? "detailed" : "compact");
  }, [expandedLayout, layout, selection, showDetailedRelations]);
  const connectedPorts = useMemo(() => new Set(visibleEdges.flatMap((edge) => [edge.sourceHandle, edge.targetHandle]).filter((id): id is string => typeof id === "string")), [visibleEdges]);
  const options = useMemo(() => ({ showAll, showUnchangedReferences, showImports, expanded }), [expanded, showAll, showUnchangedReferences, showImports]);
  /** 折叠按钮始终选中被操作对象；只更新自己的状态，保留后代的收起选择。 */
  const toggleCollapse = (id: string) => {
    setSelection({ kind: "object", id });
    setCollapsed((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id); else next.add(id);
      return next;
    });
  };
  /** 目录与文件各自保留折叠选择，布局方式由显示选项统一控制。 */
  const toggleDirectory = (path: string) => {
    setSelection({ kind: "directory", path });
    setCollapsedDirectories((current) => {
      const next = new Set(current);
      if (next.has(path)) next.delete(path); else next.add(path);
      return next;
    });
  };
  useEffect(() => {
    let current = true;
    const abort = new AbortController();
    setLayingOut(true);
    setLayoutError(undefined);
    void layoutStructureComparison(comparison, options, abort.signal).then((result) => {
      if (current) { setLayoutResult({ comparison, layout: result }); setLayingOut(false); }
    }).catch((reason: unknown) => {
      if (current) { setLayoutError(String(reason)); setLayingOut(false); }
    });
    return () => { current = false; abort.abort(); };
  }, [comparison, options]);
  // 每次折叠都从全展开缓存生成可见图；取消过期布局，避免快速切换时旧结果覆盖新状态。
  useEffect(() => {
    if (!needsCompactLayout || !foldedLayout) return;
    let current = true;
    const abort = new AbortController();
    void relayoutStructureLayout(foldedLayout, abort.signal).then((result) => {
      if (current) setCompactResult({ input: foldedLayout, layout: result });
    }).catch((reason: unknown) => {
      if (current) setCompactResult({ input: foldedLayout, error: String(reason) });
    });
    return () => { current = false; abort.abort(); };
  }, [foldedLayout, needsCompactLayout]);
  const analyzed = comparison.coverage.some((file) => file.before === "analyzed" || file.after === "analyzed");
  const emptyState = !analyzed && comparison.coverage.length > 0 ? "unsupported"
    : comparison.diff.nodes.length > 0 ? "filtered" : "empty";
  return <div className="structure-view flex min-h-0 flex-1 flex-col" aria-label={t("title")}>
    <div className="flex shrink-0 flex-wrap items-center justify-end gap-x-4 gap-y-2 border-b px-3 py-2">
      <div className="mr-auto min-w-0">
        <Select value={language ?? null} disabled={languages.length === 0} onValueChange={(value) => {
          if (!value || value === language) return;
          setPreferredLanguage(value);
          setSelection(undefined);
          setExpanded(new Set());
          setCollapsed(new Set());
          setCollapsedDirectories(new Set());
          setLayoutError(undefined);
          setLayingOut(true);
        }}>
          <SelectTrigger size="sm" className="h-7 w-40 max-w-full text-xs" aria-label={t("graphLanguage")}>
            <SelectValue>{language ? languageNames[language] ?? language : t("noGraphLanguages")}</SelectValue>
          </SelectTrigger>
          <SelectContent align="start" alignItemWithTrigger={false}>
            {languages.map((value) => <SelectItem key={value} value={value}>{languageNames[value] ?? value}</SelectItem>)}
          </SelectContent>
        </Select>
      </div>
      <TestCodeSwitch binding={binding} />
      <Button variant="ghost" size="sm" className="h-7 text-xs" onClick={binding.retry}>{t("reanalyze")}</Button>
      <DropdownMenu>
        <DropdownMenuTrigger render={<Button variant="ghost" size="sm" className="h-7 text-xs" />}>{t("displayOptions")}</DropdownMenuTrigger>
        <DropdownMenuContent align="end" className="w-64 max-w-[calc(100vw-2rem)]">
          <DropdownMenuCheckboxItem checked={showAll} onCheckedChange={setShowAll}>{t("showAll")}</DropdownMenuCheckboxItem>
          <DropdownMenuCheckboxItem checked={showDetailedRelations} onCheckedChange={setShowDetailedRelations}>{t("showDetailedRelations")}</DropdownMenuCheckboxItem>
          <DropdownMenuCheckboxItem checked={showUnchangedReferences} onCheckedChange={setShowUnchangedReferences}>{t("showUnchangedReferences")}</DropdownMenuCheckboxItem>
          <DropdownMenuSeparator />
          <DropdownMenuCheckboxItem checked={showImports} onCheckedChange={setShowImports}>{t("showImports")}</DropdownMenuCheckboxItem>
          <DropdownMenuSeparator />
          <DropdownMenuCheckboxItem checked={relayoutOnCollapse} onCheckedChange={setRelayoutOnCollapse}>{t("relayoutOnCollapse")}</DropdownMenuCheckboxItem>
          <DropdownMenuItem disabled={!layout?.collapsibleIds.length && !layout?.directoryPaths.length} onClick={() => {
            setSelection(undefined);
            setCollapsed(new Set(layout?.collapsibleIds));
            setCollapsedDirectories(new Set(layout?.directoryPaths));
          }}>{t("collapseAll")}</DropdownMenuItem>
          <DropdownMenuItem disabled={collapsed.size === 0 && collapsedDirectories.size === 0} onClick={() => {
            setCollapsed(new Set());
            setCollapsedDirectories(new Set());
          }}>{t("expandAll")}</DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
    </div>
    <ResizablePanelGroup orientation="horizontal" className="h-auto! min-h-0 flex-1">
      <ResizablePanel id="structure-details" defaultSize="320px" minSize="180px" maxSize="55%" groupResizeBehavior="preserve-pixel-size">
        <StructureDetails comparison={comparison} selectedId={selectedId} directory={directory} showImports={showImports} dark={dark} />
      </ResizablePanel>
      <ResizableHandle withHandle aria-label={t("resizeDetails")} />
      <ResizablePanel id="structure-graph" minSize="45%">
        <div className="relative flex h-full min-h-0 min-w-0 flex-col">
          <div className="relative min-h-0 flex-1" data-testid="structure-canvas">
            {visibleLayoutError ? <div role="alert" className="flex h-full flex-col items-center justify-center gap-3 p-6 text-center">
              <h3>{t("layoutFailed")}</h3><p className="max-w-xl break-words text-xs text-muted-foreground">{visibleLayoutError}</p>
              <Button variant="outline" onClick={() => setExpanded(new Set(expanded))}>{t("retryLayout")}</Button>
            </div> : layout && layout.nodes.length > 0 ? <ObjectActions.Provider value={{ selectedId, selectedDirectory, connectedPorts, toggleCollapse, toggleDirectory,
              expand: (id) => setExpanded((current) => new Set([...current, id])), open: (id) => setSelection({ kind: "object", id }), openDirectory: (path) => setSelection({ kind: "directory", path }) }}>
              <ReactFlow<ObjectNode, StructureEdge> key={language} nodes={layout.nodes} edges={visibleEdges} nodeTypes={nodeTypes} edgeTypes={edgeTypes}
                colorMode={dark ? "dark" : "light"} fitView fitViewOptions={{ padding: 0.16, maxZoom: 1 }} minZoom={0.08} maxZoom={2}
                nodesDraggable={false} nodesConnectable={false} nodesFocusable={false} edgesReconnectable={false} deleteKeyCode={null}
                onNodeClick={(_, node) => { if (node.data.directory) setSelection({ kind: "directory", path: node.data.directory.path }); else if (node.data.objectId) setSelection({ kind: "object", id: node.data.objectId }); }}
                onPaneClick={() => setSelection(undefined)}
                ariaLabelConfig={{ "controls.zoomIn.ariaLabel": t("zoomIn"), "controls.zoomOut.ariaLabel": t("zoomOut"), "controls.fitView.ariaLabel": t("fitView") }}>
                <Background gap={20} size={1} />
                <Controls showInteractive={false} />
              </ReactFlow>
            </ObjectActions.Provider> : !arranging && <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center" role="status">
              <h3 className="font-medium">{t(`${emptyState}Title`)}</h3>
              <p className="max-w-md text-sm text-muted-foreground">{t(`${emptyState}Description`)}</p>
            </div>}
            {arranging && <div className="absolute inset-0 z-10 flex items-center justify-center gap-2 bg-background/80 text-sm" role="status"><Spinner />{t("layoutLoading")}</div>}
          </div>
          <div className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-1 border-t px-3 py-2 text-[11px] text-muted-foreground" aria-label={t("legend")}>
            {(["added", "removed", "modified", "relations", "unchanged"] as const).map((status) => <span key={status} className={cn("structure-legend", statusClasses[status])}><span aria-hidden className="structure-dot" />{t(`status.${status}`)}</span>)}
            <span className="ml-auto">{t(selection === undefined ? "selectRelations" : "relationLegend")}</span>
          </div>
        </div>
      </ResizablePanel>
    </ResizablePanelGroup>
  </div>;
}
