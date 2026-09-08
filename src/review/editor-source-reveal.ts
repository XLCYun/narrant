import type { Monaco, MonacoDiffEditor } from "@monaco-editor/react";
import type { ReviewFileIdentity } from "@/review/types";
import type { StructurePeekIdentity, StructureRange, StructureSide } from "@/review/structure-types";

/** 文件改动和 Peek 各自保留身份，不为未修改文件伪造 File Change。 */
export type ReviewEditorIdentity = ReviewFileIdentity | StructurePeekIdentity;

/** requestId 允许对相同源码位置再次显式定位；范围使用真实文件 UTF-16 坐标。 */
export type ReviewEditorReveal = {
  identity: ReviewEditorIdentity;
  requestId: number;
  /** 恢复同一导航时只重建高亮，不覆盖用户的滚动与光标。 */
  preserveScroll?: boolean;
  side: StructureSide;
  range: StructureRange;
};

/** 编辑器缓存 key 带上身份种类，避免 path 和 sourceId 偶然相同导致串位。 */
export function reviewEditorIdentityKey(identity: ReviewEditorIdentity): string {
  return JSON.stringify([identity.reviewId, identity.snapshotId, "sourceId" in identity ? "peek" : "file-change", "sourceId" in identity ? identity.sourceId : identity.path]);
}

/** 分别映射范围端点；范围内部的增删不能把旧侧终点当作同偏移的新侧行号。 */
function modifiedLineForOriginal(line: number, changes: NonNullable<ReturnType<MonacoDiffEditor["getLineChanges"]>>): number {
  let delta = 0;
  for (const change of changes) {
    if (change.originalEndLineNumber > 0 && line >= change.originalStartLineNumber && line <= change.originalEndLineNumber) {
      return change.modifiedEndLineNumber === 0 ? change.modifiedStartLineNumber + 1 : change.modifiedStartLineNumber;
    }
    if ((change.originalEndLineNumber || change.originalStartLineNumber) < line) {
      const oldCount = change.originalEndLineNumber === 0 ? 0 : change.originalEndLineNumber - change.originalStartLineNumber + 1;
      const newCount = change.modifiedEndLineNumber === 0 ? 0 : change.modifiedEndLineNumber - change.modifiedStartLineNumber + 1;
      delta += newCount - oldCount;
    }
  }
  return line + delta;
}

/** 等待模型和 Diff 就绪后执行一次定位；清理时取消延后工作及独立高亮。 */
export function installReviewSourceReveal(
  editor: MonacoDiffEditor,
  monaco: Monaco,
  request: ReviewEditorReveal,
  sideBySide: boolean,
  expected: { before: string; after: string },
  onApplied?: (request: ReviewEditorReveal) => void,
): () => void {
  let frame = 0;
  let applied = false;
  let disposed = false;
  let decoration: ReturnType<ReturnType<MonacoDiffEditor["getModifiedEditor"]>["createDecorationsCollection"]> | undefined;
  const root = editor.getModifiedEditor().getDomNode();
  let observer: MutationObserver | undefined;
  /** 清理此请求独占的删除行高亮，不干扰 diff 和 Babysit 装饰。 */
  const clearRows = () => root?.querySelectorAll(".structure-peek-deleted-highlight").forEach((row) => row.classList.remove("structure-peek-deleted-highlight"));
  /** Diff 的删除内容是 modified view-zone，必须滚动并标记真正可见的旧侧行。 */
  const apply = () => {
    if (disposed) return;
    const before = editor.getOriginalEditor();
    const after = editor.getModifiedEditor();
    const models = { before: before.getModel(), after: after.getModel() };
    const changes = editor.getLineChanges();
    if (!models.before || !models.after || changes === null) return;
    if (!applied && (
      models.before.getValue() !== expected.before.replace(/\r\n/g, "\n") && models.before.getValue() !== expected.before
      || models.after.getValue() !== expected.after.replace(/\r\n/g, "\n") && models.after.getValue() !== expected.after
    )) return;
    const target = request.side === "before" ? before : after;
    const model = models[request.side]!;
    const range = model.validateRange(new monaco.Range(request.range.startLine, request.range.startColumn, request.range.endLine, request.range.endColumn));
    const deletedLines = changes.flatMap((change) => change.originalEndLineNumber > 0
      ? Array.from({ length: change.originalEndLineNumber - change.originalStartLineNumber + 1 }, (_, index) => change.originalStartLineNumber + index) : []);
    const deletedIndex = deletedLines.indexOf(range.startLineNumber);
    const lastLine = range.endColumn === 1 && range.endLineNumber > range.startLineNumber ? range.endLineNumber - 1 : range.endLineNumber;
    const hasDeletedLines = !sideBySide && request.side === "before" && deletedLines.some((line) => line >= range.startLineNumber && line <= lastLine);
    if (hasDeletedLines && root) {
      clearRows();
      root.querySelectorAll<HTMLElement>(".view-zones .line-delete .view-line").forEach((row, index) => {
        if (deletedLines[index] >= range.startLineNumber && deletedLines[index] <= lastLine) row.classList.add("structure-peek-deleted-highlight");
      });
    }
    if (!sideBySide && request.side === "before" && deletedIndex >= 0) {
      if (!root) return;
      if (!applied && !request.preserveScroll) {
        const hunk = changes.find((change) => range.startLineNumber >= change.originalStartLineNumber && range.startLineNumber <= change.originalEndLineNumber)!;
        const anchor = hunk.modifiedEndLineNumber === 0 ? hunk.modifiedStartLineNumber + 1 : hunk.modifiedStartLineNumber;
        const lineHeight = after.getOption(monaco.editor.EditorOption.lineHeight);
        // 虚拟区离开视口时 DOM rect 可为零；使用 Monaco 的公开布局坐标计算真实删除行位置。
        const zoneTop = anchor > models.after.getLineCount()
          ? after.getTopForLineNumber(models.after.getLineCount()) + lineHeight
          : after.getTopForLineNumber(Math.max(1, anchor), true);
        const rowTop = zoneTop + (range.startLineNumber - hunk.originalStartLineNumber) * lineHeight;
        after.setScrollTop(Math.max(0, rowTop - after.getLayoutInfo().height / 2));
      }
      const rows = root?.querySelectorAll<HTMLElement>(".view-zones .line-delete .view-line");
      const row = rows?.[deletedIndex];
      // Diff event 或离屏虚拟区尚未挂载时，滚动已发起，等待 DOM 变化再补高亮。
      if (!row) return;
    } else {
      if (applied) return;
      if (!request.preserveScroll) target.setSelection(range);
      if (!sideBySide && request.side === "before") {
        // 未删除的旧侧行映射到可见的新侧；不会把旧侧行号直接当作新侧行号。
        const visibleRange = models.after.validateRange(new monaco.Range(modifiedLineForOriginal(range.startLineNumber, changes), range.startColumn, modifiedLineForOriginal(range.endLineNumber, changes), range.endColumn));
        if (!request.preserveScroll) after.revealRangeInCenter(visibleRange, monaco.editor.ScrollType.Immediate);
        decoration = after.createDecorationsCollection([{ range: visibleRange, options: { isWholeLine: true, className: "structure-peek-line-highlight" } }]);
      } else {
        if (!request.preserveScroll) target.revealRangeInCenter(range, monaco.editor.ScrollType.Immediate);
        decoration = target.createDecorationsCollection([{ range, options: { isWholeLine: true, className: "structure-peek-line-highlight" } }]);
      }
    }
    if (!applied) onApplied?.(request);
    applied = true;
    // 删除区可能在异步语法着色后重建；继续同步高亮，但不再次夺取滚动位置。
    if (!hasDeletedLines) observer?.disconnect();
  };
  /** 下一帧在 Monaco 恢复 viewState 和构造 view-zone 之后定位。 */
  const schedule = () => { cancelAnimationFrame(frame); frame = requestAnimationFrame(apply); };
  const subscription = editor.onDidUpdateDiff(schedule);
  if (root) { observer = new MutationObserver(schedule); observer.observe(root, { childList: true, subtree: true }); }
  schedule();
  return () => { disposed = true; cancelAnimationFrame(frame); subscription.dispose(); observer?.disconnect(); decoration?.clear(); clearRows(); };
}
