import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { DiffEditor, type DiffOnMount, type MonacoDiffEditor } from "@monaco-editor/react";

import type { BabysitFileExplanations } from "@/babysit/types";

/** 控制当前 File Change 显示改变行还是完整解释集合。 */
export type BabysitExplanationFilter = "changed" | "all";

type ExplanationSide = keyof BabysitFileExplanations;
type LineChange = NonNullable<ReturnType<MonacoDiffEditor["getLineChanges"]>>[number];

type BabysitDiffEditorProps = {
  original: string;
  modified: string;
  language: string;
  theme: string;
  height: string;
  renderSideBySide: boolean;
  explanations?: BabysitFileExplanations;
  filter: BabysitExplanationFilter;
  expandedKeys: ReadonlySet<string>;
  onToggleExpanded: (key: string) => void;
  labels: {
    before: string;
    after: string;
    expand: string;
    collapse: string;
    explanation: string;
  };
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

/** 根据文本长度估算一个共享 view-zone 高度，让 before/after 编辑器保持垂直节奏。 */
function explanationZoneHeight(
  entries: ReadonlyArray<[number, string]>,
  expandedKeys: ReadonlySet<string>,
  side: ExplanationSide,
): number {
  const heights = entries.map(([line, text]) => {
    const expanded = expandedKeys.has(`${side}:${line}`);
    const lines = expanded ? Math.min(6, Math.max(2, Math.ceil(text.length / 62))) : 2;
    return 28 + lines * 16;
  });
  return Math.max(42, ...heights);
}

/** 创建插入 Monaco 行上方的解释卡片，并保持可键盘操作的展开按钮。 */
function createExplanationNode(input: {
  side: ExplanationSide;
  line: number;
  explanation: string;
  expanded: boolean;
  zoneHeight: number;
  labels: BabysitDiffEditorProps["labels"];
  onToggleExpanded: (key: string) => void;
}) {
  const { side, line, explanation, expanded, zoneHeight, labels, onToggleExpanded } = input;
  const key = `${side}:${line}`;
  const node = document.createElement("div");
  node.dataset.babysitZone = side;
  node.dataset.babysitLine = String(line);
  node.className = "flex h-full min-w-0 items-start gap-2 border-y border-primary/15 bg-primary/5 px-3 py-1 text-xs leading-4 text-foreground";
  node.style.minHeight = `${zoneHeight}px`;
  node.setAttribute("role", "note");
  node.setAttribute("aria-label", `${labels.explanation} ${line}`);

  const marker = document.createElement("span");
  marker.className = "shrink-0 rounded bg-primary/10 px-1.5 py-0.5 font-mono text-[10px] font-medium text-primary";
  marker.textContent = `${side === "before" ? labels.before : labels.after} · ${line}`;

  const text = document.createElement("span");
  text.className = "min-w-0 flex-1 overflow-hidden break-words text-muted-foreground";
  text.textContent = explanation;
  text.style.display = "-webkit-box";
  text.style.webkitBoxOrient = "vertical";
  text.style.webkitLineClamp = expanded ? "unset" : "2";

  node.append(marker, text);
  if (explanation.length > 120) {
    const toggle = document.createElement("button");
    toggle.type = "button";
    toggle.className = "shrink-0 rounded px-1.5 py-0.5 font-medium text-primary underline-offset-2 hover:underline focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";
    toggle.textContent = expanded ? labels.collapse : labels.expand;
    toggle.setAttribute("aria-expanded", String(expanded));
    toggle.addEventListener("click", () => onToggleExpanded(key));
    node.append(toggle);
  }
  return node;
}

/** 将 AI Line Explanations 投影为 Monaco view zones，避免修改 Review Snapshot 源文本。 */
export function BabysitDiffEditor({
  original,
  modified,
  language,
  theme,
  height,
  renderSideBySide,
  explanations,
  filter,
  expandedKeys,
  onToggleExpanded,
  labels,
}: BabysitDiffEditorProps) {
  const [mountedEditor, setMountedEditor] = useState<MonacoDiffEditor>();
  const [lineChanges, setLineChanges] = useState<readonly LineChange[]>([]);
  const zoneIds = useRef<{ original: string[]; modified: string[] }>({ original: [], modified: [] });

  /** 记录 Monaco 实例并让后续 diff 更新重新计算 changed-only 过滤。 */
  const onMount = useCallback<DiffOnMount>((nextEditor) => setMountedEditor(nextEditor), []);

  /** 清理两侧编辑器遗留的 view zones，避免切换 File Change 后解释串到下一份 Diff。 */
  const clearZones = useCallback(() => {
    if (!mountedEditor) return;
    const originalEditor = mountedEditor.getOriginalEditor();
    const modifiedEditor = mountedEditor.getModifiedEditor();
    const originalIds = zoneIds.current.original;
    const modifiedIds = zoneIds.current.modified;
    try {
      originalEditor.changeViewZones((accessor) => originalIds.forEach((id) => accessor.removeZone(id)));
      modifiedEditor.changeViewZones((accessor) => modifiedIds.forEach((id) => accessor.removeZone(id)));
    } catch {
      // Monaco 可能在 model 切换时先行销毁 zone；此时没有需要向用户暴露的错误。
    }
    zoneIds.current = { original: [], modified: [] };
  }, [mountedEditor]);

  /** 监听 Monaco 的 diff 计算完成事件，确保 changed-only 不依赖字符串猜测。 */
  useEffect(() => {
    if (!mountedEditor) return;
    const refresh = () => setLineChanges(mountedEditor.getLineChanges() ?? []);
    refresh();
    const subscription = mountedEditor.onDidUpdateDiff(refresh);
    return () => subscription.dispose();
  }, [mountedEditor, original, modified]);

  const selectedEntries = useMemo(() => {
    if (!explanations) return { before: [], after: [] } as const;
    return {
      before: selectBabysitExplanations("before", explanations.before, filter, lineChanges),
      after: selectBabysitExplanations("after", explanations.after, filter, lineChanges),
    } as const;
  }, [explanations, filter, lineChanges]);

  /** 每次结果、筛选或展开状态改变时重建两侧 view zones，并共享高度基准。 */
  useEffect(() => {
    if (!mountedEditor) return;
    clearZones();
    if (!explanations) return;
    const originalEditor = mountedEditor.getOriginalEditor();
    const modifiedEditor = mountedEditor.getModifiedEditor();
    const originalHeight = explanationZoneHeight(selectedEntries.before, expandedKeys, "before");
    const modifiedHeight = explanationZoneHeight(selectedEntries.after, expandedKeys, "after");
    const sharedHeight = Math.max(originalHeight, modifiedHeight);
    const nextIds = { original: [] as string[], modified: [] as string[] };

    originalEditor.changeViewZones((accessor) => {
      for (const [line, explanation] of selectedEntries.before) {
        nextIds.original.push(accessor.addZone({
          afterLineNumber: Math.max(0, line - 1),
          heightInPx: sharedHeight,
          domNode: createExplanationNode({
            side: "before", line, explanation, expanded: expandedKeys.has(`before:${line}`), zoneHeight: sharedHeight, labels, onToggleExpanded,
          }),
        }));
      }
    });
    modifiedEditor.changeViewZones((accessor) => {
      for (const [line, explanation] of selectedEntries.after) {
        nextIds.modified.push(accessor.addZone({
          afterLineNumber: Math.max(0, line - 1),
          heightInPx: sharedHeight,
          domNode: createExplanationNode({
            side: "after", line, explanation, expanded: expandedKeys.has(`after:${line}`), zoneHeight: sharedHeight, labels, onToggleExpanded,
          }),
        }));
      }
    });
    zoneIds.current = nextIds;
    return clearZones;
  }, [clearZones, expandedKeys, explanations, labels, lineChanges, mountedEditor, onToggleExpanded, selectedEntries]);

  return <DiffEditor
    theme={theme}
    height={height}
    language={language}
    original={original}
    modified={modified}
    onMount={onMount}
    options={{
      readOnly: true,
      renderSideBySide,
      ignoreTrimWhitespace: false,
      fontFamily: "'JetBrains Mono Variable', monospace",
      padding: { top: 4 },
    }}
  />;
}
