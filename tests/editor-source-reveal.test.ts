import { afterEach, describe, expect, it, vi } from "vitest";
import * as monacoRuntime from "monaco-editor";
import type { MonacoDiffEditor } from "@monaco-editor/react";
import { installReviewSourceReveal, reviewEditorIdentityKey } from "@/review/editor-source-reveal";

afterEach(() => { vi.restoreAllMocks(); document.body.innerHTML = ""; });

/** 用可观察的模型、Diff 完成事件和真实删除 DOM 代替浏览器编辑器。 */
function editorFixture() {
  let callback: (() => void) | undefined;
  let frame: FrameRequestCallback | undefined;
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((next) => { frame = next; return 1; });
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => undefined);
  const root = document.createElement("div");
  document.body.append(root);
  let changes: ReturnType<MonacoDiffEditor["getLineChanges"]> = null;
  const sequence: string[] = [];
  /** 模型夹具保留真实换行和 UTF-16 列值；Monaco 范围检查本身由浏览器验收。 */
  const code = (text: string) => ({
    getModel: () => ({ getValue: () => text, getLineCount: () => text.split("\n").length, validateRange: (range: unknown) => range }),
    getDomNode: () => root,
    setSelection: vi.fn(() => sequence.push("selection")),
    revealRangeInCenter: vi.fn(() => sequence.push("reveal")),
    createDecorationsCollection: vi.fn(() => ({ clear: vi.fn() })),
    getScrollTop: () => 1000,
    getTopForLineNumber: () => 1000,
    getOption: () => 20,
    setScrollTop: vi.fn(),
    getLayoutInfo: () => ({ height: 400 }),
  });
  const before = code("首行\n删除🙂\n末行");
  const after = code("首行\n末行");
  const editor = {
    getOriginalEditor: () => before, getModifiedEditor: () => after,
    getLineChanges: () => changes,
    onDidUpdateDiff: (listener: () => void) => { callback = listener; return { dispose: vi.fn() }; },
  } as unknown as MonacoDiffEditor;
  // 使用 configureLocalMonaco 注入的同一个真实模块，不能手写枚举命名空间。
  const monaco = monacoRuntime;
  return { editor, monaco, before, after, root, sequence,
    frame: () => frame?.(0), ready: () => { changes = [{ originalStartLineNumber: 2, originalEndLineNumber: 2, modifiedStartLineNumber: 1, modifiedEndLineNumber: 0, charChanges: undefined }]; callback?.(); },
  };
}

describe("完整文件编辑器显式定位", () => {
  it("等待 Diff 就绪后才定位真实侧的 UTF-16 范围，并接受 CRLF 完整文本", () => {
    const fixture = editorFixture();
    const stop = installReviewSourceReveal(fixture.editor, fixture.monaco, {
      identity: { reviewId: "r", snapshotId: "s", sourceId: "unchanged" }, requestId: 1, side: "before",
      range: { startLine: 2, startColumn: 3, endLine: 2, endColumn: 5 },
    }, true, { before: "首行\r\n删除🙂\r\n末行", after: "首行\r\n末行" });
    fixture.frame();
    expect(fixture.before.revealRangeInCenter).not.toHaveBeenCalled();
    fixture.ready(); fixture.frame();
    expect(fixture.sequence).toEqual(["selection", "reveal"]);
    expect(fixture.before.revealRangeInCenter).toHaveBeenCalledWith(expect.objectContaining({ startLineNumber: 2, startColumn: 3, endColumn: 5 }), 1);
    expect(fixture.after.revealRangeInCenter).not.toHaveBeenCalled();
    stop();
  });

  it("窄窗口离屏旧侧删除行按真实 view-zone 布局定位，并在取消后移除高亮", () => {
    const fixture = editorFixture();
    fixture.root.innerHTML = '<div class="view-zones"><div class="line-delete"><div class="view-line">删除🙂</div></div></div>';
    const row = fixture.root.querySelector<HTMLElement>(".view-line")!;
    vi.spyOn(row, "getBoundingClientRect").mockReturnValue({ top: 0 } as DOMRect);
    vi.spyOn(fixture.root, "getBoundingClientRect").mockReturnValue({ top: 100 } as DOMRect);
    const stop = installReviewSourceReveal(fixture.editor, fixture.monaco, {
      identity: { reviewId: "r", snapshotId: "s", sourceId: "deleted" }, requestId: 2, side: "before",
      range: { startLine: 2, startColumn: 1, endLine: 2, endColumn: 5 },
    }, false, { before: "首行\n删除🙂\n末行", after: "首行\n末行" });
    fixture.ready(); fixture.frame();
    expect(row).toHaveClass("structure-peek-deleted-highlight");
    expect(fixture.after.setScrollTop).toHaveBeenCalledWith(800);
    expect(fixture.before.revealRangeInCenter).not.toHaveBeenCalled();
    stop();
    expect(row).not.toHaveClass("structure-peek-deleted-highlight");
  });

  it("来源身份不与同名 File Change 的编辑器缓存碰撞", () => {
    expect(reviewEditorIdentityKey({ reviewId: "r", snapshotId: "s", sourceId: "x" }))
      .not.toBe(reviewEditorIdentityKey({ reviewId: "r", snapshotId: "s", path: "x" }));
  });

  it("旧侧范围跨越删除时分别映射首尾行，不把旧终点套用起点偏移", () => {
    const fixture = editorFixture();
    const stop = installReviewSourceReveal(fixture.editor, fixture.monaco, {
      identity: { reviewId: "r", snapshotId: "s", sourceId: "context" }, requestId: 3, side: "before",
      range: { startLine: 1, startColumn: 1, endLine: 3, endColumn: 3 },
    }, false, { before: "首行\n删除🙂\n末行", after: "首行\n末行" });
    fixture.ready(); fixture.frame();
    expect(fixture.after.revealRangeInCenter).toHaveBeenCalledWith(expect.objectContaining({ startLineNumber: 1, endLineNumber: 2 }), 1);
    stop();
  });
});
