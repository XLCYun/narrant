import { useState } from "react";
import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const toast = vi.hoisted(() => ({ success: vi.fn(), error: vi.fn(), warning: vi.fn() }));
vi.mock("sonner", () => ({ toast }));

import { i18n } from "@/lib/i18n";
import { ReviewReader } from "@/components/review-reader";
import type { ReviewReaderBinding } from "@/review/use-review-lifecycle";

const monacoEditors = vi.hoisted(() => [] as Array<{
  saveViewState: ReturnType<typeof vi.fn>;
  restoreViewState: ReturnType<typeof vi.fn>;
}>);

vi.mock("@monaco-editor/react", async () => {
  const React = await import("react");
  return {
    loader: { config: () => undefined, init: () => Promise.resolve({}) },
    DiffEditor: ({ onMount, language, originalLanguage, modifiedLanguage }: { onMount?: (editor: unknown) => void; language?: string; originalLanguage?: string; modifiedLanguage?: string }) => {
      const [editor] = React.useState(() => {
        const next = { saveViewState: vi.fn(), restoreViewState: vi.fn() };
        monacoEditors.push(next);
        return next;
      });
      React.useLayoutEffect(() => { onMount?.(editor); }, [editor, onMount]);
      return <div data-testid="mock-diff-editor" data-language={language} data-original-language={originalLanguage} data-modified-language={modifiedLanguage} />;
    },
  };
});

const snapshot = {
  reviewId: "review-1", snapshotId: "snapshot-1", baseRef: "refs/heads/main", baseRevision: "1111111111111111111111111111111111111111",
  comparisonBase: "0000000000000000000000000000000000000000", headRef: "refs/heads/topic", headRevision: "2222222222222222222222222222222222222222",
  fileChanges: [{ path: "src/example.ts", oldPath: null, status: "modified" as const }],
  pullRequest: { owner: "octo-org", repository: "narrant", number: 42, title: "Review tabs", description: "Review context", authorUsername: "octocat", state: "open" as const, isDraft: false, webUrl: "https://github.com/octo-org/narrant/pull/42", baseRef: "main", headRef: "topic", synchronizedAtMillis: 1 },
};

const availableUpdate = {
  currentBaseRevision: "base-1", currentHeadRevision: "head-1",
  availableBaseRevision: "base-2", availableHeadRevision: "head-2",
};

/** 构造 Review Reader 测试所需的稳定 lifecycle binding。 */
function reviewBinding(overrides: Partial<ReviewReaderBinding> = {}): ReviewReaderBinding {
  return {
    snapshot,
    transition: { kind: "stable" },
    fileChange: { kind: "empty" },
    update: { kind: "current" },
    view: "overview",
    setView: vi.fn(),
    diffMode: "inline",
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
    checkForUpdate: vi.fn(async () => false),
    refresh: vi.fn(async () => ({ kind: "unchanged" as const })),
    close: vi.fn(async () => ({ kind: "closed" as const })),
    ...overrides,
  };
}

const appearance = {
  narrow: false,
  editorTheme: "vs",
  onOpenExternal: vi.fn(),
};

/** 用真实 controlled state 驱动 ReviewReader，避免测试依赖生产中的 fallback。 */
function ReviewReaderHarness({ overrides = {} }: { overrides?: Partial<ReviewReaderBinding> }) {
  const [view, setView] = useState(overrides.view ?? "overview");
  const [diffMode, setDiffMode] = useState(overrides.diffMode ?? "inline");
  return <ReviewReader review={reviewBinding({ ...overrides, view, setView, diffMode, setDiffMode })} appearance={appearance} />;
}

beforeEach(async () => { vi.clearAllMocks(); await i18n.changeLanguage("zh-CN"); });
afterEach(() => { cleanup(); monacoEditors.length = 0; });

describe("ReviewReader", () => {
  it("单击图标先检查再刷新，后台发现更新不会自行刷新", async () => {
    const checkForUpdate = vi.fn(async () => true);
    const refresh = vi.fn(async () => ({ kind: "refreshed" as const, reviewId: "review-1", snapshotId: "snapshot-2" }));
    render(<ReviewReader review={reviewBinding({ update: { kind: "available", update: availableUpdate }, checkForUpdate, refresh })} appearance={appearance} />);
    expect(refresh).not.toHaveBeenCalled();
    const check = screen.getByRole("button", { name: "检查更新" });
    expect(check.textContent).toBe("");
    fireEvent.click(check);
    await vi.waitFor(() => expect(refresh).toHaveBeenCalledOnce());
    expect(checkForUpdate).toHaveBeenCalledOnce();
    expect(toast.success).toHaveBeenCalledWith("Review 已刷新");
    expect(screen.queryByText("有更新")).not.toBeInTheDocument();
  });

  it("检查期间阻止重复点击，无更新只通过通知反馈", async () => {
    let finish!: (available: boolean) => void;
    const checkForUpdate = vi.fn(() => new Promise<boolean>((resolve) => { finish = resolve; }));
    const refresh = vi.fn(async () => ({ kind: "unchanged" as const }));
    render(<ReviewReader review={reviewBinding({ checkForUpdate, refresh })} appearance={appearance} />);
    const button = screen.getByRole("button", { name: "检查更新" });
    fireEvent.click(button);
    fireEvent.click(button);
    expect(button).toBeDisabled();
    expect(button).toHaveAttribute("aria-busy", "true");
    expect(checkForUpdate).toHaveBeenCalledOnce();
    await act(async () => { finish(false); });
    expect(refresh).not.toHaveBeenCalled();
    expect(toast.success).toHaveBeenCalledWith("已是最新");
    expect(button).toBeEnabled();
    expect(screen.queryByText("已是最新")).not.toBeInTheDocument();
  });

  it("失败或取消的检查不会继续刷新或提示已是最新", async () => {
    const refresh = vi.fn(async () => ({ kind: "unchanged" as const }));
    render(<ReviewReader review={reviewBinding({ checkForUpdate: vi.fn(async () => undefined), refresh })} appearance={appearance} />);
    await act(async () => { fireEvent.click(screen.getByRole("button", { name: "检查更新" })); });
    expect(refresh).not.toHaveBeenCalled();
    expect(toast.success).not.toHaveBeenCalled();
  });

  it("离开当前审核后忽略迟到的手动检查结果", async () => {
    let finish!: (available: boolean) => void;
    const refresh = vi.fn(async () => ({ kind: "unchanged" as const }));
    const { rerender } = render(<ReviewReader review={reviewBinding({ checkForUpdate: () => new Promise((resolve) => { finish = resolve; }), refresh })} appearance={appearance} />);
    fireEvent.click(screen.getByRole("button", { name: "检查更新" }));
    rerender(<ReviewReader review={reviewBinding({ snapshot: { ...snapshot, reviewId: "review-2" } })} appearance={appearance} />);
    await act(async () => { finish(true); });
    expect(refresh).not.toHaveBeenCalled();
    expect(toast.success).not.toHaveBeenCalled();
  });

  it("窄窗口禁用双栏，并保留图标退出按钮的关闭行为", () => {
    const close = vi.fn(async () => ({ kind: "closed" as const }));
    render(<ReviewReader review={reviewBinding({ view: "file-changes", diffMode: "side-by-side", close })} appearance={{ ...appearance, narrow: true }} />);
    const toggle = screen.getByRole("button", { name: "双栏" });
    expect(toggle).toBeDisabled();
    expect(toggle).toHaveAttribute("aria-pressed", "false");
    const exit = screen.getByRole("button", { name: "退出审核" });
    expect(exit.textContent).toBe("");
    fireEvent.click(exit);
    expect(close).toHaveBeenCalledOnce();
  });

  it("用 toast 反馈失败与刷新成功，同时把恢复动作留在主按钮", async () => {
    const refresh = vi.fn(async () => ({ kind: "refreshed" as const, reviewId: "review-1", snapshotId: "snapshot-2" }));
    const failure = { kind: "operation-failed" as const, detail: "network down", retryable: true };
    const { rerender } = render(<ReviewReader review={reviewBinding({ update: { kind: "failed", failure } })} appearance={appearance} />);
    expect(toast.error).toHaveBeenCalledWith("检查更新失败", { description: "network down" });
    expect(screen.getByRole("button", { name: "检查更新" })).toBeEnabled();

    rerender(<ReviewReader review={reviewBinding({ update: { kind: "refresh-failed", update: availableUpdate, failure }, checkForUpdate: vi.fn(async () => true), refresh })} appearance={appearance} />);
    expect(toast.error).toHaveBeenCalledWith("刷新失败，当前版本未改变", { description: "network down" });
    fireEvent.click(screen.getByRole("button", { name: "检查更新" }));
    await vi.waitFor(() => expect(toast.success).toHaveBeenCalledWith("Review 已刷新"));

    rerender(<ReviewReader review={reviewBinding({ update: { kind: "confirmation-stale", failure: { ...failure, kind: "update-confirmation-stale" } } })} appearance={appearance} />);
    expect(toast.warning).toHaveBeenCalledWith("上游版本已再次变化，请重新检查");
    expect(screen.getByRole("button", { name: "检查更新" })).toBeEnabled();
  });
  it("默认展示概览，并通过标签切换到文件改动", () => {
    render(<ReviewReaderHarness />);

    expect(screen.getByRole("tab", { name: "审核概览" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("Review context")).toBeVisible();
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    expect(screen.getByRole("tab", { name: "文件改动" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("navigation", { name: "文件树" })).toBeVisible();
  });

  it("没有变更时展示带图标和顶部留白的中文空状态", () => {
    const emptySnapshot = { ...snapshot, fileChanges: [] };
    const { container } = render(<ReviewReader review={reviewBinding({ snapshot: emptySnapshot, view: "file-changes" })} appearance={appearance} />);
    expect(screen.getByRole("heading", { name: "没有文件改动" })).toBeVisible();
    const emptyState = container.querySelector("[data-empty-state]");
    expect(emptyState).toHaveClass("pt-14", "sm:pt-20");
    expect(emptyState?.querySelector("svg[aria-hidden='true']")).toBeInTheDocument();
  });

  it("只用一条审核工具栏承载视图标签和右侧操作", () => {
    render(<ReviewReaderHarness overrides={{ update: { kind: "available", update: { currentBaseRevision: "base-1", currentHeadRevision: "head-1", availableBaseRevision: "base-2", availableHeadRevision: "head-2" } } }} />);

    expect(screen.queryByRole("heading", { name: "#42 Review tabs" })).not.toBeInTheDocument();
    const toolbar = screen.getByRole("toolbar", { name: "审核工具栏" });
    expect(within(toolbar).getByRole("tab", { name: "审核概览" })).toBeVisible();
    expect(within(toolbar).getByRole("button", { name: "检查更新" })).toBeVisible();
    expect(within(toolbar).getByRole("button", { name: "退出审核" })).toBeVisible();
    expect(within(toolbar).queryByRole("button", { name: "双栏" })).not.toBeInTheDocument();

    fireEvent.click(within(toolbar).getByRole("tab", { name: "文件改动" }));
    expect(within(toolbar).getByRole("button", { name: "双栏" })).toBeVisible();
    expect(within(toolbar).getByRole("button", { name: "双栏" })).toHaveAttribute("aria-pressed", "false");
  });

  it("固定双栏按钮通过按下状态表达实际布局", () => {
    render(<ReviewReaderHarness overrides={{ diffMode: "side-by-side" }} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    const switchToInline = screen.getByRole("button", { name: "双栏" });
    expect(switchToInline).toHaveAttribute("aria-pressed", "true");

    fireEvent.click(switchToInline);
    expect(screen.getByRole("button", { name: "双栏" })).toHaveAttribute("aria-pressed", "false");
  });

  it("为 CommonJS 的 cjs 与 cts 文件选择对应的 Monaco 语言", () => {
    const content = (path: string) => ({
      kind: "ready" as const,
      path,
      content: { path, oldPath: null, status: "modified" as const, contentKind: "text" as const, beforeSize: 3, afterSize: 3, before: "old", after: "new" },
    });
    const view = render(<ReviewReader review={reviewBinding({ snapshot: { ...snapshot, fileChanges: [{ path: "src/module.cjs", oldPath: null, status: "modified" }] }, view: "file-changes", fileChange: content("src/module.cjs") })} appearance={appearance} />);
    expect(screen.getByTestId("mock-diff-editor")).toHaveAttribute("data-language", "javascript");
    view.rerender(<ReviewReader review={reviewBinding({ snapshot: { ...snapshot, fileChanges: [{ path: "src/module.cts", oldPath: null, status: "modified" }] }, view: "file-changes", fileChange: content("src/module.cts") })} appearance={appearance} />);
    expect(screen.getByTestId("mock-diff-editor")).toHaveAttribute("data-language", "typescript");
  });

  it("把 Overview、文件树和外层内容区的真实滚动位置交给 lifecycle 并恢复", () => {
    const setScrollPosition = vi.fn();
    const readyFile = {
      kind: "ready" as const,
      path: "src/example.ts",
      content: {
        ...snapshot.fileChanges[0],
        contentKind: "text" as const,
        beforeSize: 3,
        afterSize: 3,
        before: "old",
        after: "new",
      },
    };
    const positions = { overview: 41, fileTree: 63, content: 87 };
    const binding = (reviewId: string, view: "overview" | "file-changes", scrollPositions: typeof positions) => reviewBinding({
      snapshot: { ...snapshot, reviewId, snapshotId: `${reviewId}-snapshot` },
      view,
      fileChange: readyFile,
      scrollPositions,
      setScrollPosition,
    });
    const { rerender } = render(<ReviewReader review={binding("review-a", "overview", { overview: 0, fileTree: 0, content: 0 })} appearance={appearance} />);
    const overview = document.querySelector<HTMLElement>('[data-review-scroll-region="overview"]')!;
    overview.scrollTop = positions.overview;
    fireEvent.scroll(overview);
    expect(setScrollPosition).toHaveBeenCalledWith("overview", positions.overview);

    rerender(<ReviewReader review={binding("review-a", "file-changes", { overview: positions.overview, fileTree: 0, content: 0 })} appearance={appearance} />);
    const fileTree = document.querySelector<HTMLElement>('[data-review-scroll-region="fileTree"]')!;
    const content = document.querySelector<HTMLElement>('[data-review-scroll-region="content"]')!;
    fileTree.scrollTop = positions.fileTree;
    content.scrollTop = positions.content;
    fireEvent.scroll(fileTree);
    fireEvent.scroll(content);
    expect(setScrollPosition).toHaveBeenCalledWith("fileTree", positions.fileTree);
    expect(setScrollPosition).toHaveBeenCalledWith("content", positions.content);

    rerender(<ReviewReader review={binding("review-b", "file-changes", { overview: 0, fileTree: 0, content: 0 })} appearance={appearance} />);
    expect(document.querySelector<HTMLElement>('[data-review-scroll-region="fileTree"]')?.scrollTop).toBe(0);
    expect(document.querySelector<HTMLElement>('[data-review-scroll-region="content"]')?.scrollTop).toBe(0);

    rerender(<ReviewReader review={binding("review-a", "file-changes", positions)} appearance={appearance} />);
    expect(document.querySelector<HTMLElement>('[data-review-scroll-region="fileTree"]')?.scrollTop).toBe(positions.fileTree);
    expect(document.querySelector<HTMLElement>('[data-review-scroll-region="content"]')?.scrollTop).toBe(positions.content);
    rerender(<ReviewReader review={binding("review-a", "overview", positions)} appearance={appearance} />);
    expect(document.querySelector<HTMLElement>('[data-review-scroll-region="overview"]')?.scrollTop).toBe(positions.overview);
  });

  it("按 Review、Snapshot 与 File path 隔离 Monaco view state", () => {
    const stateFileA = { original: null, modified: null, modelState: { marker: "file-a" } };
    const stateFileB = { original: null, modified: null, modelState: { marker: "file-b" } };
    const stateReviewB = { original: null, modified: null, modelState: { marker: "review-b" } };
    const saveA = vi.fn();
    const saveB = vi.fn();
    const readyFile = (path: string) => ({
      kind: "ready" as const,
      path,
      content: {
        ...snapshot.fileChanges[0], path,
        contentKind: "text" as const,
        beforeSize: 3,
        afterSize: 3,
        before: "old",
        after: "new",
      },
    });
    const binding = (reviewId: string, path: string, fileViewState: typeof stateFileA | null, setFileViewState: typeof saveA) => reviewBinding({
      snapshot: { ...snapshot, reviewId, snapshotId: `${reviewId}-snapshot`, fileChanges: [
        { ...snapshot.fileChanges[0], path: "src/a.ts" },
        { ...snapshot.fileChanges[0], path: "src/b.ts" },
      ] },
      view: "file-changes",
      fileChange: readyFile(path),
      fileViewState,
      setFileViewState,
    });
    const { rerender } = render(<ReviewReader review={binding("review-a", "src/a.ts", null, saveA)} appearance={appearance} />);
    monacoEditors.at(-1)!.saveViewState.mockReturnValue(stateFileA);

    rerender(<ReviewReader review={binding("review-a", "src/b.ts", null, saveA)} appearance={appearance} />);
    expect(saveA).toHaveBeenCalledWith({ reviewId: "review-a", snapshotId: "review-a-snapshot", path: "src/a.ts" }, stateFileA);
    expect(monacoEditors.at(-1)!.restoreViewState).not.toHaveBeenCalled();
    monacoEditors.at(-1)!.saveViewState.mockReturnValue(stateFileB);

    rerender(<ReviewReader review={binding("review-a", "src/a.ts", stateFileA, saveA)} appearance={appearance} />);
    expect(saveA).toHaveBeenCalledWith({ reviewId: "review-a", snapshotId: "review-a-snapshot", path: "src/b.ts" }, stateFileB);
    expect(monacoEditors.at(-1)!.restoreViewState).toHaveBeenCalledWith(stateFileA);
    monacoEditors.at(-1)!.saveViewState.mockReturnValue(stateFileA);

    rerender(<ReviewReader review={binding("review-b", "src/a.ts", null, saveB)} appearance={appearance} />);
    expect(monacoEditors.at(-1)!.restoreViewState).not.toHaveBeenCalled();
    monacoEditors.at(-1)!.saveViewState.mockReturnValue(stateReviewB);

    rerender(<ReviewReader review={binding("review-a", "src/a.ts", stateFileA, saveA)} appearance={appearance} />);
    expect(saveB).toHaveBeenCalledWith({ reviewId: "review-b", snapshotId: "review-b-snapshot", path: "src/a.ts" }, stateReviewB);
    expect(monacoEditors.at(-1)!.restoreViewState).toHaveBeenCalledWith(stateFileA);
  });
});
