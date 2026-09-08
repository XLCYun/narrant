import type { StructureCategory, StructureChangeLabel, StructureLocation, StructureNodeId, StructureProgress, StructureResult } from "@/review/structure-types";

/** 图相机由当前 Review 持有；Peek 开关不会重置相机。 */
export type StructureGraphViewport = { x: number; y: number; zoom: number };

/** 只保存展示偏好；刷新 Snapshot 时可独立保留这一部分。 */
export type StructureGraphFilters = {
  category: StructureCategory | "all";
  change: StructureChangeLabel | "all" | "context" | "incomplete";
  calls: "selected" | "graph" | "hidden";
  inheritance: boolean;
  implementations: boolean;
  typeReferences: boolean;
  imports: boolean;
  candidates: boolean;
  external: boolean;
};

/** 完全受控的图展示状态，身份字段仅适用于对应 Snapshot。 */
export type StructureGraphPresentation = {
  viewport: StructureGraphViewport | null;
  /** auto 跟随可见画布适配，manual 保留用户平移/缩放；旧状态缺省按已有相机保守处理。 */
  viewportMode?: "auto" | "manual";
  selectedNodeId: StructureNodeId | null;
  selectedRelationshipId: string | null;
  expandedIds: string[];
  revealedNodeIds: StructureNodeId[];
  query: string;
  displayLimit: number;
  filters: StructureGraphFilters;
};

/** 导航意图交给 Peek；发生位置与目标定义是两个独立操作。 */
export type StructureGraphNavigation =
  | { kind: "node"; nodeId: string }
  | { kind: "relationship"; relationshipId: string }
  | { kind: "target-definition"; nodeId: string; relationshipId: string }
  | { kind: "implementation"; groupId: string }
  | { kind: "location"; location: StructureLocation };

/** 后台分析状态不由图组件推断，部分检查点可与任意状态同时展示。 */
export type StructureGraphAnalysis = {
  status: "idle" | "running" | "completed" | "cancelled" | "failed";
  result: StructureResult | null;
  progress?: StructureProgress | null;
  error?: string | null;
};
