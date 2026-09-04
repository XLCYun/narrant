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
let diffPending = false;
let notifyDiff: (() => void) | undefined;

/** 在 jsdom 中模拟 Monaco view-zone API，让测试验证可见行和展开行为。 */
  vi.mock("@monaco-editor/react", () => ({
  loader: { config: () => undefined, init: () => Promise.resolve({}) },
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
        getLineChanges: () => diffPending ? null : fakeLineChanges,
        onDidUpdateDiff: (callback: () => void) => { notifyDiff = callback; return { dispose: () => undefined }; },
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
    regenerate: vi.fn(),
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
    view: "file-changes",
    setView: vi.fn(),
    diffMode: "side-by-side",
    setDiffMode: vi.fn(),
    treeWidth: 25,
    setTreeWidth: vi.fn(),
    collapsedDirectories: [],
    toggleDirectory: vi.fn(),
    scrollPositions: { overview: 0, fileTree: 0, content: 0 },
    setScrollPosition: vi.fn(),
    fileViewState: null,
    setFileViewState: vi.fn(),
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
  diffPending = false;
  notifyDiff = undefined;
  await i18n.changeLanguage("zh-CN");
  fakeDecorations.original = [];
  fakeDecorations.modified = [];
  fakeLineChanges = [
    { originalStartLineNumber: 2, originalEndLineNumber: 2, modifiedStartLineNumber: 2, modifiedEndLineNumber: 2 },
  ];
});
afterEach(() => cleanup());

describe("Babysit Review Reader presentation", () => {
  it("等待当前 Monaco diff 就绪并传递原始范围", () => {
    diffPending = true;
    const review = makeReview({ kind: "idle" });
    render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));
    expect(review.babysit?.explain).not.toHaveBeenCalled();
    act(() => { diffPending = false; notifyDiff?.(); });
    expect(review.babysit?.explain).toHaveBeenCalledWith([
      { side: "before", startLineNumber: 2, endLineNumber: 2 },
      { side: "after", startLineNumber: 2, endLineNumber: 2 },
    ]);
  });

  it("Monaco 返回空范围时仍提交零 targets 请求", () => {
    fakeLineChanges = [];
    const review = makeReview({ kind: "idle" });
    render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));
    expect(review.babysit?.explain).toHaveBeenCalledWith([]);
  });

  it("中等宽度 side-by-side 禁用 Monaco 自动 inline 回退", () => {
    const review = makeReview({ kind: "idle" });
    render(<ReviewReader review={review} appearance={{ ...appearance, narrow: false, diffMode: "side-by-side" }} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    expect(screen.getByTestId("mock-diff-editor")).toHaveAttribute("data-layout", "side-by-side");
    expect(screen.getByTestId("mock-diff-editor")).toHaveAttribute("data-inline-breakpoint", "0");
  });

  it("显示全部返回注释且没有展示范围按钮", () => {
    const review = makeReview({
      kind: "completed",
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path },
      explanations: {
        before: { "1": "声明旧值。", "2": "返回旧值。" },
        after: { "1": "声明新值。", "2": "返回新值。" },
      },
    });
    render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    const babyIcon = screen.getByRole("button", { name: "宝宝模式" }).querySelector("img");
    expect(babyIcon).toHaveAttribute("src", "/icons/babysit-baby.svg");
    expect(babyIcon).toHaveClass("-translate-y-px");
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));

    expect(review.babysit?.explain).not.toHaveBeenCalled();
    expect(screen.queryByRole("button", { name: "仅改变行" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "全部非空行" })).not.toBeInTheDocument();
    expect(fakeDecorations.original.map((item) => item.range.startLineNumber)).toEqual([1, 2]);
    expect(fakeDecorations.modified.map((item) => item.range.startLineNumber)).toEqual([1, 2]);
  });

  it("在关闭宝宝模式右侧提供独立的重新生成动作", () => {
    const review = makeReview({
      kind: "completed",
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path },
      explanations: { before: { "2": "旧解释" }, after: { "2": "旧解释" } },
    });
    render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));

    const closeButton = screen.getByRole("button", { name: "关闭宝宝模式" });
    const regenerateButton = screen.getByRole("button", { name: "重新生成逐行解释" });
    expect(regenerateButton).toBeEnabled();
    expect(closeButton.parentElement).toBe(regenerateButton.parentElement);

    fireEvent.click(regenerateButton);
    expect(review.babysit?.regenerate).toHaveBeenCalledTimes(1);
    expect(review.babysit?.cancel).not.toHaveBeenCalled();
    expect(closeButton).toHaveAttribute("aria-pressed", "true");
  });

  it("added file 双栏不在空的 before pane 重复插入补偿区", () => {
    fakeLineChanges = [
      { originalStartLineNumber: 0, originalEndLineNumber: 0, modifiedStartLineNumber: 1, modifiedEndLineNumber: 2 },
    ];
    const added = { ...content, status: "added" as const, before: null, beforeSize: null };
    const review = makeReview({
      kind: "completed",
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: added.path },
      explanations: { before: {}, after: { "1": "声明新值。", "2": "返回新值。" } },
    });
    review.fileChange = { kind: "ready", path: added.path, content: added };

    render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));

    expect(fakeDecorations.original).toEqual([]);
    expect(fakeDecorations.modified.map((item) => item.range.startLineNumber)).toEqual([1, 2]);
  });

  it("宝宝模式保持打开时，locale 变化不会重新调用当前文件分析", async () => {
    const review = makeReview({ kind: "idle" });
    render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));
    expect(review.babysit?.explain).toHaveBeenCalledTimes(1);

    await act(async () => {
      await i18n.changeLanguage("en");
    });
    expect(review.babysit?.explain).toHaveBeenCalledTimes(1);
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
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path },
    });
    render(<ReviewReader review={runningReview} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));
    expect(screen.getByTestId("babysit-footer")).toHaveTextContent("正在修复逐行解释（第 2 次）");
    expect(screen.getByRole("button", { name: "重新生成逐行解释" })).toBeDisabled();
    const fileChangesPanel = screen.getByRole("tabpanel", { name: "文件改动" });
    expect(fileChangesPanel).toHaveClass("flex", "min-h-0", "flex-col");
    expect(fileChangesPanel.querySelector("[data-slot='resizable-panel-group']")).toHaveClass("h-auto!", "min-h-0", "flex-1");
    fireEvent.click(screen.getByRole("button", { name: "取消宝宝模式" }));
    expect(runningReview.babysit?.cancel).toHaveBeenCalledTimes(1);

    cleanup();
    const failedReview = makeReview({
      kind: "failed",
      retryable: true,
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path },
      error: { code: "repair_exhausted", detail: "/Users/alice/repo/src/example.ts source=const secret='token'; stderr=raw provider output" },
    });
    render(<ReviewReader review={failedReview} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));
    const footer = screen.getByTestId("babysit-footer");
    expect(screen.getByRole("button", { name: "重新生成逐行解释" })).toBeDisabled();
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

  it("复制 Custom transport 失败时保留脱敏协议分类", () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    const failedReview = makeReview({
      kind: "failed",
      retryable: true,
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path },
      error: {
        code: "provider",
        detail: "不得进入剪贴板的 provider 原文",
        diagnostic: {
          providerId: "custom",
          protocol: "openai_chat_completions",
          phase: "model",
          status: "client",
          reason: "reasoning_tools_incompatible",
          errorCategory: "transport",
        },
      },
    });

    render(<ReviewReader review={failedReview} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));
    fireEvent.click(within(screen.getByTestId("babysit-footer")).getByRole("button", { name: "复制诊断" }));

    expect(writeText).toHaveBeenCalledWith(JSON.stringify({
      category: "babysit",
      code: "provider",
      provider: {
        providerId: "custom",
        protocol: "openai_chat_completions",
        phase: "model",
        status: "client",
        reason: "reasoning_tools_incompatible",
        errorCategory: "transport",
      },
    }));
    expect(writeText.mock.calls[0]?.[0]).not.toContain("provider 原文");
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

  it("Codex 缺失时引导前往设置且不再显示临时选择器", () => {
    const review = makeReview({
      kind: "failed",
      retryable: true,
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path },
      error: {
        code: "provider_not_found",
        detail: "/Users/alice/.local/bin/codex",
        providerId: "codex",
      },
    });
    render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));
    const footer = screen.getByTestId("babysit-footer");
    expect(footer).toHaveTextContent("请前往应用设置的宝宝模式分区配置");
    expect(within(footer).queryByRole("button", { name: "选择 Codex 可执行文件" })).not.toBeInTheDocument();
    expect(footer).not.toHaveTextContent("/Users/alice");
  });

  it("Claude Code 缺失时显示 provider-aware 恢复动作且不建议回退 Codex", () => {
    const openApplicationSettings = vi.fn();
    const review = makeReview({
      kind: "failed",
      retryable: true,
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path },
      error: {
        code: "provider_not_found",
        detail: "C:\\Users\\alice\\.local\\bin\\claude.exe",
        providerId: "claude-code",
        recovery: "open_application_settings",
      },
    });
    render(<ReviewReader review={review} appearance={{ ...appearance, onOpenApplicationSettings: openApplicationSettings }} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));
    const footer = screen.getByTestId("babysit-footer");
    expect(footer).toHaveTextContent("请前往应用设置的宝宝模式分区配置");
    fireEvent.click(within(footer).getByRole("button", { name: "打开应用设置" }));
    expect(openApplicationSettings).toHaveBeenCalledOnce();
    expect(within(footer).queryByRole("button", { name: /选择.*可执行文件/ })).not.toBeInTheDocument();
    expect(footer).not.toHaveTextContent("Codex");
    expect(footer).not.toHaveTextContent("alice");
  });

  it("Custom 未配置时引导应用设置且不回退到已安装 provider", () => {
    const openApplicationSettings = vi.fn();
    const review = makeReview({
      kind: "failed",
      retryable: true,
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path },
      error: {
        code: "provider_not_found",
        detail: "Custom provider is not configured",
        providerId: "custom",
        recovery: "open_application_settings",
      },
    });
    render(<ReviewReader review={review} appearance={{ ...appearance, onOpenApplicationSettings: openApplicationSettings }} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));

    const footer = screen.getByTestId("babysit-footer");
    expect(footer).toHaveTextContent("逐行解释失败");
    expect(within(footer).getByRole("button", { name: "打开应用设置" })).toBeInTheDocument();
    expect(footer).not.toHaveTextContent("Codex");
    expect(footer).not.toHaveTextContent("Claude Code");
    expect(footer).not.toHaveTextContent("Custom provider is not configured");
    fireEvent.click(within(footer).getByRole("button", { name: "打开应用设置" }));
    expect(openApplicationSettings).toHaveBeenCalledOnce();
  });

  it("把解释直接注入代码行尾且不包含变更前后行号标签", () => {
    const longExplanation = "这是一段足够长的解释，用来说明当前代码如何读取配置、校验输入并把结果交给后续流程。".repeat(8).slice(0, 240);
    const review = makeReview({
      kind: "completed",
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path },
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

  it("切换文件后忽略旧 diff 回调并等待新文件范围", () => {
    const review = makeReview({ kind: "idle" });
    const rendered = render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));
    const staleCallback = notifyDiff;
    vi.mocked(review.babysit!.explain).mockClear();
    diffPending = true;
    const nextContent = { ...content, path: "src/next.ts" };
    rendered.rerender(<ReviewReader review={{ ...review, fileChange: { kind: "ready", path: nextContent.path, content: nextContent } }} appearance={appearance} />);
    expect(review.babysit?.explain).not.toHaveBeenCalled();
    act(() => { diffPending = false; staleCallback?.(); });
    expect(review.babysit?.explain).not.toHaveBeenCalled();
    act(() => { fakeLineChanges = []; notifyDiff?.(); });
    expect(review.babysit?.explain).toHaveBeenCalledWith([]);
  });

  it("同一 Review 切换文件时仍没有范围按钮", () => {
    const state: BabysitBinding["state"] = {
      kind: "completed",
      request: { reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, path: content.path },
      explanations: { before: { "2": "旧" }, after: { "2": "新" } },
    };
    const review = makeReview(state);
    const rendered = render(<ReviewReader review={review} appearance={appearance} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    fireEvent.click(screen.getByRole("button", { name: "宝宝模式" }));

    const nextContent = { ...content, path: "src/next.ts" };
    rendered.rerender(<ReviewReader review={{ ...review, fileChange: { kind: "ready", path: nextContent.path, content: nextContent } }} appearance={appearance} />);
    expect(screen.queryByRole("button", { name: "仅改变行" })).not.toBeInTheDocument();
  });
});
