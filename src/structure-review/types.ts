import type { ReviewIdentity } from "@/review/types";

/** 通用 Code Graph 的语义分类；保留 nativeKind 中的原语言区别。 */
export type CodeCategory = "container" | "type" | "callable" | "value" | "executionRegion" | "extension";
/** 节点自身声明或实现内容的差异类别。 */
export type ChangeKind = "added" | "removed" | "modified";
/** 包含或引用集合中增删的具体元素。 */
export type SetChange<T> = { kind: "added" | "removed"; value: T };
/** 图内、已解析图外与未解析名称具有不同的语义。 */
export type ReferenceTarget =
  | { kind: "node"; id: string }
  | { kind: "external"; symbol: string }
  | { kind: "unresolved"; name: string };

/** 来源由所属节点确定，包含关系另存 contains。 */
export type CodeReference = { semantic: string; target: ReferenceTarget };

/** 不透明身份关联完整图，不向通用模型写入 React Flow 布局。 */
export type CodeNode = {
  id: string;
  category: CodeCategory;
  language: string;
  nativeKind: string;
  name: string | null;
  signature: string | null;
  code: string | null;
  contains: string[];
  references: CodeReference[];
  extensions: Record<string, unknown>;
};
/** 完整图按不透明身份连接节点，root 只提供遍历入口。 */
export type CodeGraph = { root: string[]; nodes: CodeNode[] };
/** 自身内容与两类关系的变化独立传输。 */
export type NodeDifference = {
  id: string;
  content: ChangeKind | null;
  contains: SetChange<string>[];
  references: SetChange<CodeReference>[];
};
/** 一侧源码进入分析、缺失或不受支持的原因。 */
export type CoverageKind = "analyzed" | "absent" | "unsupported-kind" | "unsupported-language";

/** 一次固定 Snapshot 的完整分析结果，与布局和展示筛选相互独立。 */
export type StructureComparison = ReviewIdentity & {
  comparisonBase: string;
  headRevision: string;
  before: CodeGraph;
  after: CodeGraph;
  diff: { nodes: NodeDifference[] };
  coverage: { path: string; oldPath: string | null; before: CoverageKind; after: CoverageKind }[];
};

/** 调用者只提供 Snapshot 身份；仓库准备、分析和回收由后端拥有。 */
export interface StructureReviewAdapter {
  analyze(identity: ReviewIdentity, signal: AbortSignal): Promise<StructureComparison>;
}
/** 未分析、失败和取消都不等价于已经证明的空差异。 */
export type StructureState =
  | { kind: "idle" | "loading" | "cancelled" }
  | { kind: "failed"; detail: string }
  | { kind: "ready"; comparison: StructureComparison };

/** 结构视图只消费状态和用户意图，不自行发出 Tauri 命令。 */
export type StructureBinding = { state: StructureState; retry(): void; cancel(): void };
