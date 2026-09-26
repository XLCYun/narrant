import { act, cleanup, render } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import type { DiffEditorProps, DiffOnMount } from "@monaco-editor/react";
import { StructureCodeDiff } from "@/components/structure-review/structure-code-diff";
import "@/lib/i18n";

const mounted = vi.hoisted(() => ({ onMount: undefined as DiffOnMount | undefined }));
vi.mock("@/infrastructure/local-monaco", () => ({ configureLocalMonaco: vi.fn() }));
vi.mock("@monaco-editor/react", () => ({
  DiffEditor: ({ onMount }: DiffEditorProps) => { mounted.onMount = onMount; return null; },
}));

afterEach(() => { cleanup(); vi.restoreAllMocks(); vi.unstubAllGlobals(); });

it("keeps Monaco's viewport inside the visible sidebar when it shrinks", () => {
  let resize: ResizeObserverCallback = () => undefined;
  const observe = vi.fn();
  const disconnect = vi.fn();
  /** 用真实宿主的尺寸变化驱动观察回调，不依赖 jsdom 的缺失布局引擎。 */
  class HostResizeObserver {
    constructor(callback: ResizeObserverCallback) { resize = callback; }
    observe = observe;
    disconnect = disconnect;
  }
  vi.stubGlobal("ResizeObserver", HostResizeObserver);
  const { container } = render(<StructureCodeDiff original="" modified={"first\n".repeat(300) + "last"} originalLanguage="typescript" modifiedLanguage="typescript" dark={false} />);
  const host = container.querySelector<HTMLElement>("[data-structure-code-diff]")!;
  let height = 480;
  vi.spyOn(host, "clientWidth", "get").mockReturnValue(296);
  vi.spyOn(host, "clientHeight", "get").mockImplementation(() => height);
  const layout = vi.fn();
  let disposeEditor: () => void = () => undefined;
  const models = { original: { dispose: vi.fn() }, modified: { dispose: vi.fn() } };
  const editor = {
    layout,
    getModel: () => models,
    getModifiedEditor: () => ({ onDidDispose: (callback: () => void) => { disposeEditor = callback; } }),
  } as unknown as Parameters<DiffOnMount>[0];
  act(() => mounted.onMount!(editor, {} as Parameters<DiffOnMount>[1]));
  expect(layout).toHaveBeenLastCalledWith({ width: 296, height: 480 });
  expect(observe).toHaveBeenCalledWith(host);

  height = 440;
  act(() => resize([], {} as ResizeObserver));
  expect(layout).toHaveBeenLastCalledWith({ width: 296, height: 440 });
  act(() => disposeEditor());
  expect(disconnect).toHaveBeenCalledOnce();
});
