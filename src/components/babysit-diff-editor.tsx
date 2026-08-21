import { useCallback, useEffect, useMemo, useState } from "react";
import { DiffEditor, type DiffOnMount, type Monaco, type MonacoDiffEditor } from "@monaco-editor/react";

import type { BabysitFileExplanations } from "@/babysit/types";

/** 控制当前 File Change 显示改变行还是完整解释集合。 */
export type BabysitExplanationFilter = "changed" | "all";

type ExplanationSide = keyof BabysitFileExplanations;
type LineChange = NonNullable<ReturnType<MonacoDiffEditor["getLineChanges"]>>[number];

/** 同一 diff hunk 内的一对 before/after 解释槽；缺失侧只保留语义配对信息。 */
export type BabysitZoneRow = {
  before?: [number, string];
  after?: [number, string];
  beforeAnchor: number;
  afterAnchor: number;
};

/** 幂等设置一个解释 key，重复处理同一次原生 click 也不会反转结果。 */
export function setBabysitExpansion(
  current: ReadonlySet<string>,
  key: string,
  expanded: boolean,
): Set<string> {
  const next = new Set(current);
  if (expanded) next.add(key); else next.delete(key);
  return next;
}

/** Babysit Diff Editor 的受控输入；解释状态由 Review Reader 持有。 */
type BabysitDiffEditorProps = {
  original: string;
  modified: string;
  language: string;
  theme: string;
  height: string;
  renderSideBySide: boolean;
  explanations?: BabysitFileExplanations;
  filter: BabysitExplanationFilter;
};

/** Monaco Diff 的单行改变范围，使用统一 before/after 字段避免 UI 依赖 provider。 */
export function isChangedLine(
  side: ExplanationSide,
  lineNumber: number,
  changes: readonly LineChange[],
): boolean {
  return changes.some((change) => {
    const start = side === "before" ? change.originalStartLineNumber : change.modifiedStartLineNumber;
    const end = side === "before" ? change.originalEndLineNumber : change.modifiedEndLineNumber;
    return start > 0 && end >= start && lineNumber >= start && lineNumber <= end;
  });
}

/** 将完整解释结果按 Monaco 自己计算的行变化筛选，切换筛选不会重新请求 Agent。 */
export function selectBabysitExplanations(
  side: ExplanationSide,
  explanations: Record<string, string>,
  filter: BabysitExplanationFilter,
  changes: readonly LineChange[],
): Array<[number, string]> {
  return Object.entries(explanations)
    .map(([line, explanation]) => [Number(line), explanation] as [number, string])
    .filter(([line]) => Number.isInteger(line) && line > 0 && (filter === "all" || isChangedLine(side, line, changes)))
    .sort(([left], [right]) => left - right);
}

/** 按 diff hunk 配对两侧解释；渲染时只为实际存在的解释创建 zone。 */
export function buildBabysitZoneRows(
  before: ReadonlyArray<[number, string]>,
  after: ReadonlyArray<[number, string]>,
  changes: readonly LineChange[],
): BabysitZoneRow[] {
  const usedBefore = new Set<number>();
  const usedAfter = new Set<number>();
  const rows: BabysitZoneRow[] = [];
  const append = (left: ReadonlyArray<[number, string]>, right: ReadonlyArray<[number, string]>, beforeAnchor: number, afterAnchor: number) => {
    for (let index = 0; index < Math.max(left.length, right.length); index += 1) {
      const beforeEntry = left[index];
      const afterEntry = right[index];
      if (beforeEntry) usedBefore.add(beforeEntry[0]);
      if (afterEntry) usedAfter.add(afterEntry[0]);
      rows.push({
        before: beforeEntry,
        after: afterEntry,
        beforeAnchor: beforeEntry?.[0] ?? beforeAnchor,
        afterAnchor: afterEntry?.[0] ?? afterAnchor,
      });
    }
  };
  for (const change of changes) {
    const left = before.filter(([line]) => line >= change.originalStartLineNumber && line <= change.originalEndLineNumber);
    const right = after.filter(([line]) => line >= change.modifiedStartLineNumber && line <= change.modifiedEndLineNumber);
    append(left, right, Math.max(1, change.originalStartLineNumber), Math.max(1, change.modifiedStartLineNumber));
  }
  const remainingBefore = before.filter(([line]) => !usedBefore.has(line));
  const remainingAfter = after.filter(([line]) => !usedAfter.has(line));
  append(remainingBefore, remainingAfter, remainingBefore[0]?.[0] ?? 1, remainingAfter[0]?.[0] ?? 1);
  return rows;
}

/** 把 original 行号按 diff 顺序映射到 Monaco 单栏模式生成的红色删除行。 */
export function syncInlineDeletedExplanations(
  root: HTMLElement,
  entries: ReadonlyArray<[number, string]>,
  changes: readonly LineChange[],
): void {
  const explanations = new Map(entries);
  const originalLines = changes.flatMap((change) => {
    if (change.originalStartLineNumber <= 0 || change.originalEndLineNumber < change.originalStartLineNumber) return [];
    return Array.from(
      { length: change.originalEndLineNumber - change.originalStartLineNumber + 1 },
      (_, index) => change.originalStartLineNumber + index,
    );
  });
  const deletedRows = root.querySelectorAll<HTMLElement>(".view-zones .line-delete .view-line");
  deletedRows.forEach((row, index) => {
    const explanation = explanations.get(originalLines[index] ?? -1);
    const existing = row.querySelector<HTMLElement>("[data-babysit-inline-deleted]");
    if (!explanation) {
      existing?.remove();
      return;
    }
    if (existing) {
      if (existing.textContent !== explanation) existing.textContent = explanation;
      existing.title = explanation;
      return;
    }
    const tag = document.createElement("span");
    tag.className = "babysit-inline-explanation";
    tag.dataset.babysitInlineDeleted = "true";
    tag.textContent = explanation;
    tag.title = explanation;
    row.firstElementChild?.append(tag);
  });
}

/** 将 AI Line Explanations 作为行尾 injected text 渲染，不修改 Review Snapshot 源文本。 */
export function BabysitDiffEditor({
  original,
  modified,
  language,
  theme,
  height,
  renderSideBySide,
  explanations,
  filter,
}: BabysitDiffEditorProps) {
  const [mounted, setMounted] = useState<{ editor: MonacoDiffEditor; monaco: Monaco }>();
  const [lineChanges, setLineChanges] = useState<readonly LineChange[]>([]);

  /** 记录 Monaco 实例并让后续 diff 更新重新计算 changed-only 过滤。 */
  const onMount = useCallback<DiffOnMount>((editor, monaco) => setMounted({ editor, monaco }), []);

  /** 监听 Monaco 的 diff 计算完成事件，确保 changed-only 不依赖字符串猜测。 */
  useEffect(() => {
    if (!mounted) return;
    const refresh = () => setLineChanges(mounted.editor.getLineChanges() ?? []);
    refresh();
    const subscription = mounted.editor.onDidUpdateDiff(refresh);
    return () => subscription.dispose();
  }, [mounted, original, modified]);

  const selectedEntries = useMemo(() => {
    if (!explanations) return { before: [], after: [] } as const;
    return {
      before: selectBabysitExplanations("before", explanations.before, filter, lineChanges),
      after: selectBabysitExplanations("after", explanations.after, filter, lineChanges),
    } as const;
  }, [explanations, filter, lineChanges]);

  /** 每次结果或筛选改变时重建两侧行尾解释 decoration。 */
  useEffect(() => {
    if (!mounted) return;
    const apply = (side: ExplanationSide, entries: ReadonlyArray<[number, string]>) => {
      const editor = side === "before" ? mounted.editor.getOriginalEditor() : mounted.editor.getModifiedEditor();
      const model = editor.getModel();
      if (!model) return { clear: () => undefined };
      const decorations = entries
        .filter(([line]) => line <= model.getLineCount())
        .map(([line, explanation]) => {
          const column = model.getLineMaxColumn(line);
          return {
            range: new mounted.monaco.Range(line, Math.max(1, column - 1), line, column),
            options: {
              description: `Babysit ${side} line ${line}`,
              hoverMessage: { value: explanation },
              after: { content: explanation, inlineClassName: "babysit-inline-explanation" },
            },
          };
        });
      const collection = editor.createDecorationsCollection(decorations);
      return { clear: () => collection.clear() };
    };
    const before = apply("before", selectedEntries.before);
    const after = apply("after", selectedEntries.after);
    return () => { before.clear(); after.clear(); };
  }, [mounted, renderSideBySide, selectedEntries]);

  /** Monaco 单栏把删除行复制进 modified view-zone；同步 original 解释到这些红色虚拟行。 */
  useEffect(() => {
    if (!mounted || renderSideBySide) return;
    const root = mounted.editor.getModifiedEditor().getDomNode();
    if (!root) return;
    const sync = () => syncInlineDeletedExplanations(root, selectedEntries.before, lineChanges);
    sync();
    const observer = new MutationObserver(sync);
    observer.observe(root, { childList: true, subtree: true });
    return () => {
      observer.disconnect();
      root.querySelectorAll("[data-babysit-inline-deleted]").forEach((node) => node.remove());
    };
  }, [lineChanges, mounted, renderSideBySide, selectedEntries.before]);

  return <>
    <DiffEditor
    theme={theme}
    height={height}
    language={language}
    original={original}
    modified={modified}
    onMount={onMount}
    options={{
      readOnly: true,
      renderSideBySide,
      // 产品层已经根据窗口决定 side-by-side/inline；禁用 Monaco 约 900px 的自动回退，
      // 避免 UI 仍创建双 pane zones 时 original pane 实际只剩 gutter。
      renderSideBySideInlineBreakpoint: 0,
      ignoreTrimWhitespace: false,
      fontFamily: "'JetBrains Mono Variable', monospace",
      padding: { top: 4 },
    }}
    />
  </>;
}
