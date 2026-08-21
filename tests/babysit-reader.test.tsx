import { useEffect } from "react";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { i18n } from "@/lib/i18n";
import { ReviewReader } from "@/components/review-reader";
import type { BabysitBinding } from "@/babysit/types";
import type { ReviewReaderBinding } from "@/review/use-review-lifecycle";

const fakeZones = {
  original: new Map<string, HTMLElement>(),
  modified: new Map<string, HTMLElement>(),
};
let nextZoneId = 0;
const removedZoneNodes: HTMLElement[] = [];

/** 在 jsdom 中模拟 Monaco view-zone API，让测试验证可见行和展开行为。 */
  vi.mock("@monaco-editor/react", () => ({
  DiffEditor: (props: { onMount?: (editor: unknown, monaco: unknown) => void; options?: { renderSideBySide?: boolean; renderSideBySideInlineBreakpoint?: number } }) => {
    const { onMount, options } = props;
    useEffect(() => {
      const createEditor = (side: "original" | "modified") => ({
        changeViewZones(callback: (accessor: { addZone(zone: { domNode: HTMLElement }): string; removeZone(id: string): void }) => void) {
          callback({
            addZone(zone) {
              const id = `${side}-${++nextZoneId}`;
              // React root 必须拥有 Monaco 接收的同一个稳定容器。
              const hostedNode = zone.domNode;
              document.body.append(hostedNode);
              fakeZones[side].set(id, hostedNode);
              return id;
            },
            removeZone(id) {
              const node = fakeZones[side].get(id);
              if (node) removedZoneNodes.push(node);
              node?.remove();
              fakeZones[side].delete(id);
            },
          });
        },
      });
      const fakeEditor = {
        getLineChanges: () => [
          { originalStartLineNumber: 2, originalEndLineNumber: 2, modifiedStartLineNumber: 2, modifiedEndLineNumber: 2 },
        ],
        onDidUpdateDiff: () => ({ dispose: () => undefined }),
        getOriginalEditor: () => createEditor("original"),
        getModifiedEditor: () => createEditor("modified"),
      };
      onMount?.(fakeEditor, {});
    }, [onMount]);
    return <div data-testid="mock-diff-editor" data-layout={options?.renderSideBySide ? "side-by-side" : "inline"} data-inline-breakpoint={options?.renderSideBySideInlineBreakpoint} />;
  },
}));

const snapshot = {
  reviewId: "review-1", snapshotId: "snapshot-1", baseRef: "main", baseRevision: "base", comparisonBase: "merge-base", headRef: "topic", headRevision: "head",
  fileChanges: [{ path: "src/example.ts", oldPath: null, status: "modified" as const }],
};

const content = {
  path: "src/example.ts", oldPath: null, status: "modified" as const, contentKind: "text" as const,
  beforeSize: 28, afterSize: 31, before: "const oldValue = 1;\nreturn oldValue;\n", after: "const newValue = 2;\nreturn newValue;\n",
};

/** 创建带指定 Babysit 状态的 Review Reader binding。 */
function makeReview(state: BabysitBinding["state"], overrides: Partial<BabysitBinding> = {}): ReviewReaderBinding {
  const babysit: BabysitBinding = {
    eligibility: { kind: "resolved", result: { eligible: true, reason: null } },
    state,
    explain: vi.fn(),
    cancel: vi.fn(),
    retry: vi.fn(),
    ...overrides,
  };
  return {
    snapshot,
    transition: { kind: "stable" },
    fileChange: { kind: "ready", path: content.path, content },
    babysit,
    update: { kind: "current" },
    selectFileChange: vi.fn(),
    retryFileChange: vi.fn(),
    refresh: vi.fn(async () => ({ kind: "unchanged" as const })),
    close: vi.fn(async () => ({ kind: "closed" as const })),
  };
}

const appearance = {
  diffMode: "side-by-side" as const,
  narrow: false,
  editorTheme: "vs",
  initialTreeWidth: 25,
  onDiffModeChange: vi.fn(),
  onTreeWidthChange: vi.fn(),
  onOpenExternal: vi.fn(),
};

beforeEach(async () => {
  await i18n.changeLanguage("zh-CN");
  fakeZones.original.clear();
  fakeZones.modified.clear();
  nextZoneId = 0;
  removedZoneNodes.length = 0;
});
afterEach(() => cleanup());

describe("Babysit Review Reader presentation", () => {
  it("中等宽度 side-by-side 禁用 Monaco 自动 inline 回退", () => {
    const review = makeReview({ kind: "idle" });
    render(<ReviewReader review={review} appearance={{ ...appearance, narrow: false, diffMode: "side-by-side" }} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    expect(screen.getByTestId("mock-diff-editor")).toHaveAttribute("data-layout", "side-by-side");
    expect(screen.getByTestId("mock-diff-editor")).toHaveAttribute("data-inline-breakpoint", "0");
  });

  it("默认只显示改变行，并可切换到全部非空行而不重新请求", () => {
    const review = makeReview({
      kind: "completed",
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path, locale: "zh-CN" },
      explanations: {
        before: { "1": "声明旧值。", "2": "返回旧值。" },
        after: { "1": "声明新值。", "2": "返回新值。" },
      },
    });
    render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));

    expect(review.babysit?.explain).not.toHaveBeenCalled();
    expect([...fakeZones.original.values()].map((node) => node.dataset.babysitLine)).toEqual(["2"]);
    expect([...fakeZones.modified.values()].map((node) => node.dataset.babysitLine)).toEqual(["2"]);

    fireEvent.click(screen.getByRole("button", { name: "全部非空行" }));
    expect([...fakeZones.original.values()].map((node) => node.dataset.babysitLine).sort()).toEqual(["1", "2"]);
    expect([...fakeZones.modified.values()].map((node) => node.dataset.babysitLine).sort()).toEqual(["1", "2"]);
  });

  it("在生成、修复和失败状态显示文件级 sticky footer 操作", () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    const runningReview = makeReview({
      kind: "repairing",
      attempt: 2,
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path, locale: "zh-CN" },
    });
    render(<ReviewReader review={runningReview} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));
    expect(screen.getByTestId("babysit-footer")).toHaveTextContent("正在修复逐行解释（第 2 次）");
    fireEvent.click(screen.getByRole("button", { name: "取消宝宝模式" }));
    expect(runningReview.babysit?.cancel).toHaveBeenCalledTimes(1);

    cleanup();
    const failedReview = makeReview({
      kind: "failed",
      retryable: true,
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path, locale: "zh-CN" },
      error: { code: "repair_exhausted", detail: "三轮修复后仍缺少第 2 行" },
    });
    render(<ReviewReader review={failedReview} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));
    const footer = screen.getByTestId("babysit-footer");
    expect(footer).toHaveTextContent("逐行解释失败");
    fireEvent.click(within(footer).getByRole("button", { name: "重试" }));
    expect(failedReview.babysit?.retry).toHaveBeenCalledTimes(1);
    fireEvent.click(within(footer).getByRole("button", { name: "复制诊断" }));
    expect(within(footer).getByRole("button", { name: "已复制诊断" })).toBeInTheDocument();
    expect(writeText).toHaveBeenCalledWith('{"category":"babysit","code":"repair_exhausted"}');
    expect(writeText.mock.calls[0]?.[0]).not.toContain("三轮修复");
  });

  it("禁用不支持的文件并在 Tooltip 触发器上保留具体原因", () => {
    const unsupported = { ...content, path: "pnpm-lock.yaml" };
    const review = makeReview({ kind: "idle" }, { eligibility: { kind: "resolved", result: { eligible: false, reason: "lock_minified_or_snapshot" } } });
    review.fileChange = { kind: "ready", path: unsupported.path, content: unsupported };
    render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    const button = screen.getByRole("button", { name: "宝宝模式" });
    expect(button).toHaveAttribute("aria-disabled", "true");
    expect(button).toHaveAttribute("tabindex", "0");
    expect(button).toHaveAttribute("aria-describedby");
  });

  it("长解释默认截断，并能在 view zone 内展开", () => {
    const longExplanation = "这是一段足够长的解释，用来说明当前代码如何读取配置、校验输入并把结果交给后续流程。".repeat(8).slice(0, 240);
    const review = makeReview({
      kind: "completed",
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path, locale: "zh-CN" },
      explanations: { before: { "2": longExplanation }, after: { "2": longExplanation } },
    });
    render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));
    const original = [...fakeZones.original.values()][0];
    const expand = within(original).getByRole("button", { name: "展开" });
    expect(original.style.pointerEvents).toBe("auto");
    expect(original).toHaveStyle({ width: "100%", maxWidth: "100%", overflow: "hidden" });
    expect(expand.style.pointerEvents).toBe("auto");
    expect(expand).toHaveClass("cursor-pointer", "pointer-events-auto");
    const card = within(original).getByRole("note");
    expect(card).toHaveClass("w-full", "max-w-full", "overflow-hidden");
    expect(expand.nextElementSibling).toHaveTextContent("变更前 · 2");
    expect(card.firstElementChild).toBe(expand);
    expect(expand).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(expand);
    expect(removedZoneNodes.length).toBeGreaterThan(0);
    expect(removedZoneNodes.every((node) => !node.isConnected && node.childElementCount === 0)).toBe(true);
    const expanded = [...fakeZones.original.values()][0];
    expect(within(expanded).getByRole("button", { name: "收起" })).toHaveAttribute("aria-expanded", "true");
    expect(expanded.style.minHeight).toBe("220px");
    const expandedText = [...expanded.querySelectorAll("span")].find((node) => node.textContent === longExplanation);
    expect(expandedText).toHaveStyle({ display: "block" });
    expect(expandedText).toHaveClass("overflow-visible");
    expect(expandedText).toHaveTextContent(longExplanation);

    fireEvent.click(within(expanded).getByRole("button", { name: "收起" }));
    const collapsedAgain = [...fakeZones.original.values()][0];
    expect(within(collapsedAgain).getByRole("button", { name: "展开" })).toHaveAttribute("aria-expanded", "false");
    expect(collapsedAgain.style.minHeight).toBe("60px");
    const collapsedText = [...collapsedAgain.querySelectorAll("span")].find((node) => node.textContent === longExplanation);
    expect(collapsedText).toHaveStyle({ display: "-webkit-box" });
    expect(collapsedText).toHaveClass("overflow-hidden");
  });

  it("同一 Review 切换文件时保留 changed/all 过滤选择", () => {
    const state: BabysitBinding["state"] = {
      kind: "completed",
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path, locale: "zh-CN" },
      explanations: { before: { "2": "旧" }, after: { "2": "新" } },
    };
    const review = makeReview(state);
    const rendered = render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));
    fireEvent.click(screen.getByRole("button", { name: "全部非空行" }));

    const nextContent = { ...content, path: "src/next.ts" };
    rendered.rerender(<ReviewReader review={{ ...review, fileChange: { kind: "ready", path: nextContent.path, content: nextContent } }} appearance={appearance} />);
    expect(screen.getByRole("button", { name: "全部非空行" })).toHaveAttribute("aria-pressed", "true");
  });
});
