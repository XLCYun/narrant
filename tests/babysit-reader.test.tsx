import { useEffect } from "react";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { i18n } from "@/lib/i18n";
import { ReviewReader } from "@/components/review-reader";
import type { BabysitBinding } from "@/babysit/types";
import type { ReviewReaderBinding } from "@/review/use-review-lifecycle";

type FakeDecoration = { range: { startLineNumber: number }; options: { hoverMessage?: { value: string }; after?: { content: string; inlineClassName: string } } };
const fakeDecorations: Record<"original" | "modified", FakeDecoration[]> = { original: [], modified: [] };
let fakeLineChanges = [
  { originalStartLineNumber: 2, originalEndLineNumber: 2, modifiedStartLineNumber: 2, modifiedEndLineNumber: 2 },
];

/** 在 jsdom 中模拟 Monaco view-zone API，让测试验证可见行和展开行为。 */
  vi.mock("@monaco-editor/react", () => ({
  DiffEditor: (props: { onMount?: (editor: unknown, monaco: unknown) => void; options?: { renderSideBySide?: boolean; renderSideBySideInlineBreakpoint?: number } }) => {
    const { onMount, options } = props;
    useEffect(() => {
      const createEditor = (side: "original" | "modified") => ({
        getModel: () => ({ getLineCount: () => 100, getLineMaxColumn: () => 20 }),
        createDecorationsCollection(decorations: FakeDecoration[]) {
          fakeDecorations[side] = decorations;
          return { clear: () => { if (fakeDecorations[side] === decorations) fakeDecorations[side] = []; } };
        },
      });
      const fakeEditor = {
        getLineChanges: () => fakeLineChanges,
        onDidUpdateDiff: () => ({ dispose: () => undefined }),
        getOriginalEditor: () => createEditor("original"),
        getModifiedEditor: () => createEditor("modified"),
      };
      class Range {
        startLineNumber: number;
        constructor(startLineNumber: number) { this.startLineNumber = startLineNumber; }
      }
      onMount?.(fakeEditor, { Range });
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
    selectExecutable: vi.fn(),
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
  fakeDecorations.original = [];
  fakeDecorations.modified = [];
  fakeLineChanges = [
    { originalStartLineNumber: 2, originalEndLineNumber: 2, modifiedStartLineNumber: 2, modifiedEndLineNumber: 2 },
  ];
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
    expect(screen.queryByRole("button", { name: "仅改变行" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "全部非空行" })).toBeInTheDocument();
    expect(fakeDecorations.original.map((item) => item.range.startLineNumber)).toEqual([2]);
    expect(fakeDecorations.modified.map((item) => item.range.startLineNumber)).toEqual([2]);

    fireEvent.click(screen.getByRole("button", { name: "全部非空行" }));
    expect(screen.queryByRole("button", { name: "全部非空行" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "仅改变行" })).toBeInTheDocument();
    expect(fakeDecorations.original.map((item) => item.range.startLineNumber)).toEqual([1, 2]);
    expect(fakeDecorations.modified.map((item) => item.range.startLineNumber)).toEqual([1, 2]);
  });

  it("added file 双栏不在空的 before pane 重复插入补偿区", () => {
    fakeLineChanges = [
      { originalStartLineNumber: 0, originalEndLineNumber: 0, modifiedStartLineNumber: 1, modifiedEndLineNumber: 2 },
    ];
    const added = { ...content, status: "added" as const, before: null, beforeSize: null };
    const review = makeReview({
      kind: "completed",
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: added.path, locale: "zh-CN" },
      explanations: { before: {}, after: { "1": "声明新值。", "2": "返回新值。" } },
    });
    review.fileChange = { kind: "ready", path: added.path, content: added };

    render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));

    expect(fakeDecorations.original).toEqual([]);
    expect(fakeDecorations.modified.map((item) => item.range.startLineNumber)).toEqual([1, 2]);
  });

  it("宝宝模式保持打开时，locale 变化会重新 arm 当前文件分析", async () => {
    const review = makeReview({ kind: "idle" });
    render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));
    expect(review.babysit?.explain).toHaveBeenCalledTimes(1);

    await act(async () => {
      await i18n.changeLanguage("en");
    });
    expect(review.babysit?.explain).toHaveBeenCalledTimes(2);
  });

  it("首次打开在点击事件内立即启动当前文件分析", () => {
    const review = makeReview({ kind: "idle" });
    render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));

    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));

    expect(review.babysit?.explain).toHaveBeenCalledTimes(1);
    expect(screen.getByRole("button", { name: "关闭宝宝模式" })).toHaveAttribute("aria-pressed", "true");
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
    const fileChangesPanel = screen.getByRole("tabpanel", { name: "文件改动" });
    expect(fileChangesPanel).toHaveClass("flex", "min-h-0", "flex-col");
    expect(fileChangesPanel.querySelector("[data-slot='resizable-panel-group']")).toHaveClass("h-auto!", "min-h-0", "flex-1");
    fireEvent.click(screen.getByRole("button", { name: "取消宝宝模式" }));
    expect(runningReview.babysit?.cancel).toHaveBeenCalledTimes(1);

    cleanup();
    const failedReview = makeReview({
      kind: "failed",
      retryable: true,
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path, locale: "zh-CN" },
      error: { code: "repair_exhausted", detail: "/Users/alice/repo/src/example.ts source=const secret='token'; stderr=raw provider output" },
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
    expect(writeText.mock.calls[0]?.[0]).not.toContain("/Users/alice");
    expect(writeText.mock.calls[0]?.[0]).not.toContain("secret");
    expect(writeText.mock.calls[0]?.[0]).not.toContain("token");
    expect(writeText.mock.calls[0]?.[0]).not.toContain("raw provider output");
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
    expect(button).toHaveClass("cursor-not-allowed", "opacity-50");
  });

  it("Codex 缺失时显示 typed 选择器恢复动作且不渲染路径", () => {
    const selectExecutable = vi.fn();
    const review = makeReview({
      kind: "failed",
      retryable: true,
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path, locale: "zh-CN" },
      error: { code: "codex_not_found", detail: "/Users/alice/.local/bin/codex", recovery: "select_codex_executable" },
    }, { selectExecutable });
    render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));
    const footer = screen.getByTestId("babysit-footer");
    expect(footer).toHaveTextContent("未找到可用的 Codex，请选择可执行文件");
    expect(within(footer).getByRole("button", { name: "选择 Codex 可执行文件" })).toBeInTheDocument();
    expect(footer).not.toHaveTextContent("/Users/alice");
    fireEvent.click(within(footer).getByRole("button", { name: "选择 Codex 可执行文件" }));
    expect(selectExecutable).toHaveBeenCalledTimes(1);
  });

  it("把解释直接注入代码行尾且不包含变更前后行号标签", () => {
    const longExplanation = "这是一段足够长的解释，用来说明当前代码如何读取配置、校验输入并把结果交给后续流程。".repeat(8).slice(0, 240);
    const review = makeReview({
      kind: "completed",
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path, locale: "zh-CN" },
      explanations: { before: { "2": longExplanation }, after: { "2": longExplanation } },
    });
    render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));
    for (const side of ["original", "modified"] as const) {
      const injected = fakeDecorations[side][0]?.options.after;
      expect(injected).toEqual({ content: longExplanation, inlineClassName: "babysit-inline-explanation" });
      expect(fakeDecorations[side][0]?.options.hoverMessage?.value).toBe(longExplanation);
      expect(injected?.content).not.toContain("变更前");
      expect(injected?.content).not.toContain("变更后");
      expect(injected?.content).not.toContain("· 2");
    }
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
    expect(screen.getByRole("button", { name: "仅改变行" })).toBeInTheDocument();
  });
});
