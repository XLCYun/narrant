import { useState } from "react";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { i18n } from "@/lib/i18n";
import { ReviewReader } from "@/components/review-reader";
import type { ReviewReaderBinding } from "@/review/use-review-lifecycle";

const snapshot = {
  reviewId: "review-1", snapshotId: "snapshot-1", baseRef: "refs/heads/main", baseRevision: "1111111111111111111111111111111111111111",
  comparisonBase: "0000000000000000000000000000000000000000", headRef: "refs/heads/topic", headRevision: "2222222222222222222222222222222222222222",
  fileChanges: [{ path: "src/example.ts", oldPath: null, status: "modified" as const }],
  pullRequest: { owner: "octo-org", repository: "narrant", number: 42, title: "Review tabs", description: "Review context", authorUsername: "octocat", state: "open" as const, isDraft: false, webUrl: "https://github.com/octo-org/narrant/pull/42", baseRef: "main", headRef: "topic", synchronizedAtMillis: 1 },
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
    selectFileChange: vi.fn(),
    retryFileChange: vi.fn(),
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

beforeEach(async () => { await i18n.changeLanguage("zh-CN"); });
afterEach(() => cleanup());

describe("ReviewReader", () => {
  it("默认展示概览，并通过标签切换到文件改动", () => {
    render(<ReviewReaderHarness />);

    expect(screen.getByRole("tab", { name: "审核概览" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("Review context")).toBeVisible();
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    expect(screen.getByRole("tab", { name: "文件改动" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("navigation", { name: "文件树" })).toBeVisible();
  });

  it("只用一条审核工具栏承载视图标签和右侧操作", () => {
    render(<ReviewReaderHarness overrides={{ update: { kind: "available", update: { currentBaseRevision: "base-1", currentHeadRevision: "head-1", availableBaseRevision: "base-2", availableHeadRevision: "head-2" } } }} />);

    expect(screen.queryByRole("heading", { name: "#42 Review tabs" })).not.toBeInTheDocument();
    const toolbar = screen.getByRole("toolbar", { name: "审核工具栏" });
    expect(within(toolbar).getByRole("tab", { name: "审核概览" })).toBeVisible();
    expect(within(toolbar).getByRole("button", { name: "刷新审核" })).toBeVisible();
    expect(within(toolbar).getByRole("button", { name: "退出审核" })).toBeVisible();
    expect(within(toolbar).queryByRole("button", { name: "切换为双栏" })).not.toBeInTheDocument();

    fireEvent.click(within(toolbar).getByRole("tab", { name: "文件改动" }));
    expect(within(toolbar).getByRole("button", { name: "切换为双栏" })).toBeVisible();
    expect(within(toolbar).getByRole("button", { name: "切换为双栏" })).toHaveTextContent("双栏");
  });

  it("布局按钮展示点击后将切换到的目标布局", () => {
    render(<ReviewReaderHarness overrides={{ diffMode: "side-by-side" }} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    const switchToInline = screen.getByRole("button", { name: "切换为单栏" });
    expect(switchToInline).toHaveTextContent("单栏");

    fireEvent.click(switchToInline);
    expect(screen.getByRole("button", { name: "切换为双栏" })).toHaveTextContent("双栏");
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
});
