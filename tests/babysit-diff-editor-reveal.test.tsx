import { useEffect } from "react";
import * as monacoRuntime from "monaco-editor";
import "@/lib/i18n";
import { act, cleanup, render } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { BabysitDiffEditor } from "@/components/babysit-diff-editor";
import { StructurePeek } from "@/components/code-structure/structure-peek";
import type { StructurePeekState } from "@/review/structure-peek";
import type { StructurePeekSource } from "@/review/structure-types";

const fixture = vi.hoisted(() => ({ events: [] as string[], ready: false, callbacks: new Set<() => void>(), frame: undefined as FrameRequestCallback | undefined }));

/** 在公开 Monaco mount 边界观察恢复、Diff 就绪和重复导航顺序。 */
function codeEditor(side: string) {
  return {
    getModel: () => ({ getValue: () => side, validateRange: (value: unknown) => value, getLineCount: () => 1 }),
    getDomNode: () => null,
    setSelection: () => fixture.events.push(`select:${side}`),
    revealRangeInCenter: () => fixture.events.push(`reveal:${side}`),
    createDecorationsCollection: () => ({ clear: () => undefined }),
  };
}

vi.mock("@monaco-editor/react", () => ({
  loader: { config: vi.fn(), init: () => Promise.resolve({}) },
  DiffEditor: ({ onMount, keepCurrentOriginalModel = false, keepCurrentModifiedModel = false }: { onMount: (editor: unknown, monaco: unknown) => void; keepCurrentOriginalModel?: boolean; keepCurrentModifiedModel?: boolean }) => {
    useEffect(() => {
      let models: { original: { dispose(): void }; modified: { dispose(): void } } | null = {
        original: { dispose: () => fixture.events.push("dispose-original-model") },
        modified: { dispose: () => fixture.events.push("dispose-modified-model") },
      };
      onMount({
        getOriginalEditor: () => codeEditor("before"), getModifiedEditor: () => codeEditor("after"),
        getModel: () => models,
        setModel: (value: null) => { models = value; fixture.events.push("detach-models"); },
        getLineChanges: () => fixture.ready ? [] : null,
        restoreViewState: () => fixture.events.push("restore"), saveViewState: () => ({ scrollTop: 50 }),
        onDidUpdateDiff: (callback: () => void) => { fixture.callbacks.add(callback); return { dispose: () => fixture.callbacks.delete(callback) }; },
      }, monacoRuntime);
      // 复现安装版 React wrapper 的公开 keepCurrent*Model 配置与清理顺序。
      return () => {
        if (!keepCurrentOriginalModel) models?.original.dispose();
        if (!keepCurrentModifiedModel) models?.modified.dispose();
        fixture.events.push("dispose-editor");
      };
    }, [onMount, keepCurrentOriginalModel, keepCurrentModifiedModel]);
    return <div />;
  },
  Editor: ({ onMount }: { onMount: (editor: unknown, monaco: unknown) => void }) => {
    useEffect(() => {
      onMount({ ...codeEditor("after"), restoreViewState: () => fixture.events.push("restore-single"), saveViewState: () => ({ scrollTop: 50 }) }, monacoRuntime);
    }, [onMount]);
    return <div />;
  },
}));

beforeEach(() => {
  fixture.events = []; fixture.ready = false; fixture.callbacks.clear(); fixture.frame = undefined;
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) => { fixture.frame = callback; return 1; });
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => undefined);
});
afterEach(() => { cleanup(); vi.restoreAllMocks(); });

it("恢复 Peek 状态后等待 Diff，再执行定位；重复请求有效，其他来源请求无效", () => {
  const identity = { reviewId: "review", snapshotId: "snapshot", sourceId: "context.ts" };
  const onSaveViewState = vi.fn();
  const reveal = { identity, requestId: 1, side: "after" as const, range: { startLine: 1, startColumn: 1, endLine: 1, endColumn: 2 } };
  const props = { identity, original: "before", modified: "after", language: "typescript", theme: "light", height: "400px", renderSideBySide: true, viewState: { scrollTop: 500 }, onSaveViewState };
  const rendered = render(<BabysitDiffEditor {...props} reveal={reveal} />);
  act(() => { fixture.frame?.(0); });
  expect(fixture.events).toEqual(["restore"]);
  act(() => { fixture.ready = true; fixture.callbacks.forEach((callback) => callback()); fixture.frame?.(0); });
  expect(fixture.events).toEqual(["restore", "select:after", "reveal:after"]);
  rendered.rerender(<BabysitDiffEditor {...props} reveal={{ ...reveal, requestId: 2 }} />);
  act(() => { fixture.frame?.(0); });
  expect(fixture.events.filter((item) => item === "reveal:after")).toHaveLength(2);
  rendered.rerender(<BabysitDiffEditor {...props} reveal={{ ...reveal, requestId: 3, identity: { ...identity, sourceId: "other.ts" } }} />);
  act(() => { fixture.frame?.(0); });
  expect(fixture.events.filter((item) => item === "reveal:after")).toHaveLength(2);
  rendered.unmount();
  expect(onSaveViewState).toHaveBeenCalledWith(identity, { scrollTop: 50 });
});

it("切回已有 Review 时恢复 Peek 手动滚动，新的显式导航才再次定位", () => {
  fixture.ready = true;
  const source: StructurePeekSource = { id: "context", before: { state: "present", path: "context.ts", revision: "base" }, after: { state: "present", path: "context.ts", revision: "head" }, classification: { kind: "unchanged", path: "context.ts" } };
  const state: StructurePeekState = {
    selection: { reviewId: "r", snapshotId: "s", title: "Context", locations: [{ sourceId: source.id, side: "after", role: "declaration", range: { startLine: 1, endLine: 1, startColumn: 1, endColumn: 2 } }], sources: [source], targetDefinition: null, unavailableReason: null },
    locationIndex: 0, requestId: 7,
    load: { status: "ready", content: { reviewId: "r", snapshotId: "s", sourceId: source.id, source, before: { state: "available", path: "context.ts", revision: "base", objectId: "base-blob", text: "before", size: 6 }, after: { state: "available", path: "context.ts", revision: "head", objectId: "head-blob", text: "after", size: 5 } } },
  };
  const props = { narrow: false, diffMode: "side-by-side" as const, editorTheme: "light", viewState: { kind: "diff", value: { scrollTop: 500 }, revealedRequestId: 7 }, onSaveViewState: vi.fn(), onSelectLocation: vi.fn(), onNavigate: vi.fn(), onRetry: vi.fn(), onClose: vi.fn(), onOpenFileChange: vi.fn() };
  const rendered = render(<StructurePeek {...props} state={state} />);
  act(() => { fixture.frame?.(0); });
  expect(fixture.events).toEqual(["restore"]);
  rendered.rerender(<StructurePeek {...props} state={{ ...state, requestId: 8 }} />);
  act(() => { fixture.frame?.(0); });
  expect(fixture.events).toEqual(["restore", "select:after", "reveal:after"]);
  rendered.unmount();
  expect(props.onSaveViewState).toHaveBeenCalledWith({ reviewId: "r", snapshotId: "s", sourceId: "context" }, { kind: "diff", value: { scrollTop: 50 }, revealedRequestId: 8 });
});

it("单侧不可用时通过真实 Monaco API 定位另一侧完整阅读器", () => {
  const source: StructurePeekSource = { id: "readable", before: { state: "present", path: "readable.ts", revision: "base" }, after: { state: "present", path: "readable.ts", revision: "head" }, classification: { kind: "unchanged", path: "readable.ts" } };
  const state: StructurePeekState = {
    selection: { reviewId: "r", snapshotId: "s", title: "Readable", locations: [{ sourceId: source.id, side: "after", role: "declaration", range: { startLine: 1, endLine: 1, startColumn: 1, endColumn: 3 } }], sources: [source], targetDefinition: null, unavailableReason: null },
    locationIndex: 0, requestId: 1,
    load: { status: "ready", content: { reviewId: "r", snapshotId: "s", sourceId: source.id, source, before: { state: "unavailable", reason: "git-object-unavailable", message: "Missing object", retryable: true }, after: { state: "available", path: "readable.ts", revision: "head", objectId: "head-blob", text: "after", size: 5 } } },
  };
  render(<StructurePeek state={state} narrow={false} diffMode="side-by-side" editorTheme="light" viewState={null} onSaveViewState={vi.fn()} onSelectLocation={vi.fn()} onNavigate={vi.fn()} onRetry={vi.fn()} onClose={vi.fn()} onOpenFileChange={vi.fn()} />);
  expect(fixture.events).toEqual(["select:after", "reveal:after"]);
});

it("切换源码时先脱离 Diff 模型再释放；更新保存回调不会销毁正在阅读的模型", () => {
  const identity = { reviewId: "review", snapshotId: "snapshot", sourceId: "context.ts" };
  const previousSave = vi.fn();
  const currentSave = vi.fn();
  const props = { identity, original: "before", modified: "after", language: "typescript", theme: "light", height: "400px", renderSideBySide: true, viewState: null };
  const rendered = render(<BabysitDiffEditor {...props} onSaveViewState={previousSave} />);
  fixture.events.length = 0;
  rendered.rerender(<BabysitDiffEditor {...props} onSaveViewState={currentSave} />);
  expect(fixture.events).toEqual([]);
  rendered.unmount();
  expect(fixture.events).toEqual(["detach-models", "dispose-original-model", "dispose-modified-model", "dispose-editor"]);
  expect(previousSave).not.toHaveBeenCalled();
  expect(currentSave).toHaveBeenCalledWith(identity, { scrollTop: 50 });
});
