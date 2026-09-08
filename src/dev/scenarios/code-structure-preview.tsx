import { useMemo, useState } from "react";
import { CodeStructureView } from "@/components/code-structure/code-structure-view";
import { createStructureGraphPresentation } from "@/review/structure-graph";
import type { StructureGraphAnalysis, StructureGraphNavigation } from "@/review/structure-graph-types";
import { createStructurePreviewResult, type CodeStructureScenario } from "./structure-preview-data";
export type { CodeStructureScenario } from "./structure-preview-data";

/** 预览在本地持有受控状态，底部只回显导航意图，明确不冒充源码后端。 */
export function CodeStructurePreview({ scenario = "default" }: { scenario?: CodeStructureScenario }) {
  const result = useMemo(() => createStructurePreviewResult(scenario), [scenario]);
  const [presentation, setPresentation] = useState(() => {
    const initial = createStructureGraphPresentation(result);
    // 独立关系控制场景显式打开开关，便于同时验收候选虚线、import 容器边与保留关系。
    if (scenario === "multiple-occurrences") return { ...initial, selectedNodeId: "decl:rust:refresh", selectedRelationshipId: "relation:rust:refresh:record" };
    if (scenario === "multiple-impls") return { ...initial, selectedNodeId: "decl:rust:owner" };
    return scenario === "relationship-controls" ? { ...initial, selectedNodeId: "decl:welcome", filters: { ...initial.filters, calls: "graph" as const, imports: true, candidates: true } } : initial;
  });
  const [navigation, setNavigation] = useState<StructureGraphNavigation | null>(null);
  const [status, setStatus] = useState<StructureGraphAnalysis["status"]>(scenario === "loading" ? "running" : scenario === "error" ? "failed" : scenario === "cancelled" ? "cancelled" : "completed");
  return <div className="h-[calc(100vh-10rem)] min-h-[500px]">
    <CodeStructureView analysis={{ status, result: scenario === "loading" || scenario === "error" ? null : result, error: scenario === "error" ? "Preview: Git object unavailable" : null, progress: status === "running" ? { phase: "extraction", side: "after", processedFiles: 27, totalFiles: 375 } : null }} presentation={presentation} onPresentationChange={setPresentation} onNavigate={setNavigation} onCancel={() => setStatus("cancelled")} onRetry={() => setStatus("running")} colorMode={document.documentElement.classList.contains("dark") ? "dark" : "light"} />
    <output className="block truncate pt-2 font-mono text-xs" aria-label="Preview navigation">{navigation ? JSON.stringify(navigation) : "Preview: select a node or edge to inspect its navigation intent"}</output>
  </div>;
}
