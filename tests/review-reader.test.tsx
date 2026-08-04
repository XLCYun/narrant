import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { i18n } from "@/lib/i18n";
import { ReviewReader } from "@/components/review-reader";
import type { ReviewReaderBinding } from "@/review/use-review-lifecycle";

const snapshot = {
  reviewId: "review-1", snapshotId: "snapshot-1", baseRef: "refs/heads/main", baseRevision: "1111111111111111111111111111111111111111",
  comparisonBase: "0000000000000000000000000000000000000000", headRef: "refs/heads/topic", headRevision: "2222222222222222222222222222222222222222",
  fileChanges: [{ path: "src/example.ts", oldPath: null, status: "modified" as const }],
  pullRequest: { owner: "octo-org", repository: "narrant", number: 42, title: "Review tabs", description: "Review context", author: "octocat", state: "open" as const, draft: false, url: "https://github.com/octo-org/narrant/pull/42", baseRef: "main", headRef: "topic", synchronizedAt: 1 },
};

/** 构造 Review Reader 测试所需的稳定 lifecycle binding。 */
function reviewBinding(overrides: Partial<ReviewReaderBinding> = {}): ReviewReaderBinding {
  return {
    snapshot,
    transition: { kind: "stable" },
    fileChange: { kind: "empty" },
    update: { kind: "current" },
    selectFileChange: vi.fn(),
    retryFileChange: vi.fn(),
    refresh: vi.fn(async () => ({ kind: "unchanged" as const })),
    close: vi.fn(async () => ({ kind: "closed" as const })),
    ...overrides,
  };
}

const appearance = {
  diffMode: "inline" as const,
  narrow: false,
  editorTheme: "vs",
  initialTreeWidth: 25,
  onDiffModeChange: vi.fn(),
  onTreeWidthChange: vi.fn(),
  onOpenExternal: vi.fn(),
};

beforeEach(async () => { await i18n.changeLanguage("zh-CN"); });
afterEach(() => cleanup());

describe("ReviewReader", () => {
  it("默认展示概览，并通过标签切换到文件改动", () => {
    render(<ReviewReader review={reviewBinding()} appearance={appearance} />);

    expect(screen.getByRole("tab", { name: "审核概览" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("Review context")).toBeVisible();
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    expect(screen.getByRole("tab", { name: "文件改动" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("navigation", { name: "文件树" })).toBeVisible();
  });

  it("只用一条审核工具栏承载视图标签和右侧操作", () => {
    render(<ReviewReader review={reviewBinding({ update: { kind: "available", update: { currentBaseRevision: "base-1", currentHeadRevision: "head-1", availableBaseRevision: "base-2", availableHeadRevision: "head-2" } } })} appearance={appearance} />);

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
    const { rerender } = render(<ReviewReader review={reviewBinding()} appearance={{ ...appearance, diffMode: "side-by-side" }} />);
    fireEvent.click(screen.getByRole("tab", { name: "文件改动" }));
    expect(screen.getByRole("button", { name: "切换为单栏" })).toHaveTextContent("单栏");

    rerender(<ReviewReader review={reviewBinding()} appearance={appearance} />);
    expect(screen.getByRole("button", { name: "切换为双栏" })).toHaveTextContent("双栏");
  });
});
