import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { DiffEditor, type DiffOnMount, type Monaco, type MonacoDiffEditor } from "@monaco-editor/react";

import type { BabysitChangedRange, BabysitFileExplanations } from "@/babysit/types";
import { configureLocalMonaco } from "@/infrastructure/local-monaco";
import type { ReviewFileViewState } from "@/review/types";
import { installReviewSourceReveal, reviewEditorIdentityKey, type ReviewEditorIdentity, type ReviewEditorReveal } from "@/review/editor-source-reveal";
import "@/review/editor-source-reveal.css";
import { readReviewEditorZoomLevel, saveReviewEditorZoomLevel } from "@/review/editor-zoom";
import { registerReviewEditorThemes, REVIEW_DARK_THEME, REVIEW_LIGHT_THEME } from "@/review/editor-theme";

configureLocalMonaco();

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
type BabysitDiffEditorProps<Identity extends ReviewEditorIdentity> = {
  original: string;
  modified: string;
  language: string;
  theme: string;
  height: string;
  renderSideBySide: boolean;
  explanations?: BabysitFileExplanations;
  onDiffChange?(identity: Identity, ranges: BabysitChangedRange[] | null): void;
  identity: Identity;
  reveal?: ReviewEditorReveal;
  onRevealApplied?(request: ReviewEditorReveal): void;
  viewState: ReviewFileViewState | null;
  onSaveViewState(identity: Identity, state: ReviewFileViewState): void;
};

/** 将后端返回的全部解释按有效行号排序。 */
function sortedExplanations(
  explanations: Record<string, string>,
): Array<[number, string]> {
  return Object.entries(explanations)
    .map(([line, explanation]) => [Number(line), explanation] as [number, string])
    .filter(([line]) => Number.isInteger(line) && line > 0)
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
export function BabysitDiffEditor<Identity extends ReviewEditorIdentity>({
  original,
  modified,
  language,
  theme,
  height,
  renderSideBySide,
  explanations,
  onDiffChange,
  identity,
  viewState,
  onSaveViewState,
  reveal,
  onRevealApplied,
}: BabysitDiffEditorProps<Identity>) {
  const editorRef = useRef<MonacoDiffEditor>();
  const [mounted, setMounted] = useState<{ editor: MonacoDiffEditor; monaco: Monaco }>();
  const [lineChanges, setLineChanges] = useState<readonly LineChange[]>([]);

  /** 记录 Monaco 实例，供后续 diff 范围捕获和解释布局使用。 */
  const onMount = useCallback<DiffOnMount>((editor, monaco) => {
    editorRef.current = editor;
    if (viewState) editor.restoreViewState?.(viewState as NonNullable<ReturnType<MonacoDiffEditor["saveViewState"]>>);
    setMounted({ editor, monaco });
  }, [viewState]);
  const identityKey = reviewEditorIdentityKey(identity);
  // 同一身份的对象重建不触发卸载保存，也不会覆盖显式源码定位。
  // identityKey 完整编码联合身份；忽略父组件仅对象引用变化。
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const stableIdentity = useMemo(() => identity, [identityKey]);
  const viewStateOwner = useRef({ identity: stableIdentity, save: onSaveViewState });
  /** 保存回调变化只更新接收方，不得清理仍在阅读的 Monaco 实例。 */
  useLayoutEffect(() => {
    viewStateOwner.current = { identity: stableIdentity, save: onSaveViewState };
  }, [onSaveViewState, stableIdentity]);
  /** 在 wrapper 的被动卸载之前保存状态，并按 Monaco 契约先解绑再释放文件模型。 */
  useLayoutEffect(() => () => {
    const editor = editorRef.current;
    const state = editor?.saveViewState?.();
    if (state) viewStateOwner.current.save(viewStateOwner.current.identity, state);
    const models = editor?.getModel?.();
    if (models) {
      editor!.setModel(null);
      models.original.dispose();
      if (models.modified !== models.original) models.modified.dispose();
    }
    editorRef.current = undefined;
  }, []);

  /** 监听 Monaco 的 diff 计算完成事件，确保 changed-only 不依赖字符串猜测。 */
  useEffect(() => {
    if (!mounted) return;
    if (typeof mounted.editor.getOriginalEditor !== "function" || typeof mounted.editor.getModifiedEditor !== "function") return;
    const refresh = () => {
      const changes = mounted.editor.getLineChanges?.() ?? null;
      setLineChanges(changes ?? []);
      const ranges: BabysitChangedRange[] | null = changes === null ? null : changes.flatMap((change) => {
        const result: BabysitChangedRange[] = [];
        if (change.originalEndLineNumber >= change.originalStartLineNumber && change.originalStartLineNumber > 0) {
          result.push({ side: "before", startLineNumber: change.originalStartLineNumber, endLineNumber: change.originalEndLineNumber });
        }
        if (change.modifiedEndLineNumber >= change.modifiedStartLineNumber && change.modifiedStartLineNumber > 0) {
          result.push({ side: "after", startLineNumber: change.modifiedStartLineNumber, endLineNumber: change.modifiedEndLineNumber });
        }
        return result;
      });
      onDiffChange?.(stableIdentity, ranges);
    };
    refresh();
    const subscription = mounted.editor.onDidUpdateDiff?.(refresh);
    return () => subscription?.dispose();
  }, [mounted, original, modified, onDiffChange, stableIdentity]);

  /** 源码定位在恢复 viewState 之后排队，并且只能命中当前模型身份。 */
  useEffect(() => {
    if (!mounted || !reveal || reviewEditorIdentityKey(reveal.identity) !== identityKey) return;
    return installReviewSourceReveal(mounted.editor, mounted.monaco, reveal, renderSideBySide, { before: original, after: modified }, onRevealApplied);
  }, [mounted, reveal, identityKey, renderSideBySide, original, modified, onRevealApplied]);

  /** Review Reader 可见时接管字号快捷键，避免 WebView 同时缩放整个应用界面。 */
  useEffect(() => {
    if (!mounted) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if ((!event.ctrlKey && !event.metaKey) || event.altKey) return;
      const action = event.code === "Equal" || event.code === "NumpadAdd"
        ? "editor.action.fontZoomIn"
        : event.code === "Minus" || event.code === "NumpadSubtract"
          ? "editor.action.fontZoomOut"
          : event.code === "Digit0" || event.code === "Numpad0"
            ? "editor.action.fontZoomReset"
            : event.code === "KeyP"
              ? "editor.action.quickCommand"
              : undefined;
      if (!action) return;
      event.preventDefault();
      const originalEditor = mounted.editor.getOriginalEditor();
      const modifiedEditor = mounted.editor.getModifiedEditor();
      const target = originalEditor.hasWidgetFocus() ? originalEditor : modifiedEditor;
      if (action === "editor.action.quickCommand" && !target.hasWidgetFocus()) target.focus();
      target.trigger("narrant", action, undefined);
    };
    window.addEventListener("keydown", onKeyDown, { capture: true });
    return () => window.removeEventListener("keydown", onKeyDown, { capture: true });
  }, [mounted]);

  /** 从设备偏好恢复 Monaco 全局缩放，并持久化后续内置字号命令产生的变化。 */
  useEffect(() => {
    if (!mounted) return;
    const editorZoom = mounted.monaco?.editor?.EditorZoom;
    if (!editorZoom) return;
    editorZoom.setZoomLevel(readReviewEditorZoomLevel());
    const subscription = editorZoom.onDidChangeZoomLevel((level: number) => {
      saveReviewEditorZoomLevel(level);
    });
    return () => subscription.dispose();
  }, [mounted]);

  const selectedEntries = useMemo(() => {
    if (!explanations) return { before: [], after: [] } as const;
    return {
      before: sortedExplanations(explanations.before),
      after: sortedExplanations(explanations.after),
    } as const;
  }, [explanations]);

  /** 每次结果改变时重建两侧行尾解释 decoration。 */
  useEffect(() => {
    if (!mounted) return;
    if (typeof mounted.editor.getOriginalEditor !== "function" || typeof mounted.editor.getModifiedEditor !== "function") return;
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
    if (typeof mounted.editor.getModifiedEditor !== "function") return;
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
    theme={theme === "light" ? REVIEW_LIGHT_THEME : theme === "vs-dark" ? REVIEW_DARK_THEME : theme}
    beforeMount={registerReviewEditorThemes}
    height={height}
    language={language}
    original={original}
    modified={modified}
    keepCurrentOriginalModel
    keepCurrentModifiedModel
    onMount={onMount}
    options={{
      readOnly: true,
      renderSideBySide,
      // 通过底色表达增删，隐藏行号旁的加减号。
      renderIndicators: false,
      // 产品层已经根据窗口决定 side-by-side/inline；禁用 Monaco 约 900px 的自动回退，
      // 避免 UI 仍创建双 pane zones 时 original pane 实际只剩 gutter。
      renderSideBySideInlineBreakpoint: 0,
      ignoreTrimWhitespace: false,
      ...(reveal ? { wordWrap: "off" as const, diffWordWrap: "off" as const, experimental: { useTrueInlineView: false } } : {}),
      fontFamily: "'JetBrains Mono Variable', monospace",
      padding: { top: 4 },
    }}
    />
  </>;
}
