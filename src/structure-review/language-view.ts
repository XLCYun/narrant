import type { CodeGraph, CodeReference, StructureComparison } from "./types";

/** 从两侧对象收集语言，保留仅有删除对象的语言，不根据文件后缀猜测分析能力。 */
export function structureLanguages(comparison: StructureComparison): string[] {
  return [...new Set([...comparison.before.nodes, ...comparison.after.nodes].map((node) => node.language))].sort();
}

/** 生成单语言展示投影；不改写分析结果，包含与引用只连接当前语言中的对象。 */
export function comparisonForLanguage(comparison: StructureComparison, language: string | undefined): StructureComparison {
  if (language === undefined) return comparison;
  /** 每侧独立过滤目标，避免在某一侧保留只存在于另一侧的悬空关系。 */
  const graph = (source: CodeGraph): CodeGraph => {
    const selected = source.nodes.filter((node) => node.language === language);
    const ids = new Set(selected.map((node) => node.id));
    const nodes = selected.map((node) => ({
      ...node,
      contains: node.contains.filter((id) => ids.has(id)),
      references: node.references.filter((reference) => keepsReference(reference, ids)),
    }));
    const children = new Set(nodes.flatMap((node) => node.contains));
    return { ...source, nodes, root: nodes.filter((node) => !children.has(node.id)).map((node) => node.id) };
  };
  const before = graph(comparison.before);
  const after = graph(comparison.after);
  const ids = new Set([...before.nodes, ...after.nodes].map((node) => node.id));
  const nodes = comparison.diff.nodes.filter((node) => ids.has(node.id)).map((node) => ({
    ...node,
    contains: node.contains.filter((change) => ids.has(change.value)),
    references: node.references.filter((change) => keepsReference(change.value, ids)),
  })).filter((node) => node.content !== null || node.contains.length > 0 || node.references.length > 0);
  return { ...comparison, before, after, diff: { nodes } };
}

/** 图外与未解析目标继续由现有显示规则处理，语言筛选只裁剪图内的跨语言关系。 */
function keepsReference(reference: CodeReference, ids: ReadonlySet<string>): boolean {
  return reference.target.kind !== "node" || ids.has(reference.target.id);
}
