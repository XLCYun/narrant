import { useEffect } from "react";
import { cleanup, fireEvent, render } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BabysitDiffEditor } from "@/components/babysit-diff-editor";

const originalTrigger = vi.fn();
const modifiedTrigger = vi.fn();
const modifiedFocus = vi.fn();
const setZoomLevel = vi.fn((level: number) => { zoomLevel = level; });
let originalFocused = false;
let modifiedFocused = false;
let zoomLevel = 0;
const zoomListeners = new Set<(level: number) => void>();

/** 构造可观察焦点和 Monaco action 的编辑器测试替身。 */
function fakeCodeEditor(side: "original" | "modified") {
  return {
    createDecorationsCollection: () => ({ clear: () => undefined }),
    focus: side === "modified" ? modifiedFocus : vi.fn(),
    getDomNode: () => document.createElement("div"),
    getModel: () => null,
    hasWidgetFocus: () => side === "original" ? originalFocused : modifiedFocused,
    trigger: side === "original" ? originalTrigger : modifiedTrigger,
  };
}

vi.mock("@monaco-editor/react", () => ({
  loader: { config: () => undefined, init: () => Promise.resolve({}) },
  DiffEditor: ({ onMount }: { onMount?: (editor: unknown, monaco: unknown) => void }) => {
    useEffect(() => {
      onMount?.({
        getLineChanges: () => [],
        getOriginalEditor: () => fakeCodeEditor("original"),
        getModifiedEditor: () => fakeCodeEditor("modified"),
        onDidUpdateDiff: () => ({ dispose: () => undefined }),
        saveViewState: () => null,
      }, {
        Range: class Range {},
        editor: {
          EditorZoom: {
            getZoomLevel: () => zoomLevel,
            onDidChangeZoomLevel: (listener: (level: number) => void) => {
              zoomListeners.add(listener);
              return { dispose: () => zoomListeners.delete(listener) };
            },
            setZoomLevel,
          },
        },
      });
    }, [onMount]);
    return <div data-testid="diff-editor" />;
  },
}));

const props = {
  original: "old",
  modified: "new",
  language: "typescript",
  theme: "vs",
  height: "400px",
  renderSideBySide: true,
  filter: "changed" as const,
  identity: { reviewId: "review", snapshotId: "snapshot", path: "src/a.ts" },
  viewState: null,
  onSaveViewState: vi.fn(),
};

beforeEach(() => {
  originalTrigger.mockReset();
  modifiedTrigger.mockReset();
  modifiedFocus.mockReset();
  setZoomLevel.mockClear();
  originalFocused = false;
  modifiedFocused = false;
  zoomLevel = 0;
  zoomListeners.clear();
  localStorage.clear();
});

afterEach(() => cleanup());

describe("BabysitDiffEditor keyboard commands", () => {
  it("Reader 内按主修饰键加号时增大 Monaco 字体而不缩放页面", () => {
    render(<BabysitDiffEditor {...props} />);

    const event = new KeyboardEvent("keydown", { key: "=", code: "Equal", ctrlKey: true, bubbles: true, cancelable: true });
    window.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(true);
    expect(modifiedTrigger).toHaveBeenCalledWith("narrant", "editor.action.fontZoomIn", undefined);
  });

  it.each([
    [{ key: "-", code: "Minus", metaKey: true }, "editor.action.fontZoomOut"],
    [{ key: "0", code: "Digit0", ctrlKey: true }, "editor.action.fontZoomReset"],
    [{ key: "p", code: "KeyP", ctrlKey: true }, "editor.action.quickCommand"],
  ])("把 Reader 快捷键映射到 Monaco 内置命令", (init, action) => {
    render(<BabysitDiffEditor {...props} />);

    const event = new KeyboardEvent("keydown", { ...init, bubbles: true, cancelable: true });
    window.dispatchEvent(event);

    expect(event.defaultPrevented).toBe(true);
    expect(modifiedTrigger).toHaveBeenCalledWith("narrant", action, undefined);
  });

  it("优先在当前聚焦的 original pane 执行命令", () => {
    originalFocused = true;
    render(<BabysitDiffEditor {...props} />);

    fireEvent.keyDown(window, { key: "p", code: "KeyP", metaKey: true });

    expect(originalTrigger).toHaveBeenCalledWith("narrant", "editor.action.quickCommand", undefined);
    expect(modifiedTrigger).not.toHaveBeenCalled();
    expect(modifiedFocus).not.toHaveBeenCalled();
  });

  it("没有 pane 获得焦点时先聚焦 modified pane 再打开命令面板", () => {
    render(<BabysitDiffEditor {...props} />);

    fireEvent.keyDown(window, { key: "p", code: "KeyP", ctrlKey: true });

    expect(modifiedFocus).toHaveBeenCalledOnce();
    expect(modifiedTrigger).toHaveBeenCalledWith("narrant", "editor.action.quickCommand", undefined);
  });

  it("恢复并保存所有 Review 共享的 Monaco 字体缩放级别", () => {
    localStorage.setItem("narrant.review.editorZoomLevel", "3");
    render(<BabysitDiffEditor {...props} />);

    expect(setZoomLevel).toHaveBeenCalledWith(3);
    zoomListeners.forEach((listener) => listener(4));
    expect(localStorage.getItem("narrant.review.editorZoomLevel")).toBe("4");
  });
});
