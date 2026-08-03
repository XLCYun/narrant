import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { i18n } from "@/lib/i18n";
import { ReviewReader } from "@/components/review-reader";

const snapshot = {
  reviewId: "review-1", snapshotId: "snapshot-1", baseRef: "refs/heads/main", baseRevision: "1111111111111111111111111111111111111111",
  comparisonBase: "0000000000000000000000000000000000000000", headRef: "refs/heads/topic", headRevision: "2222222222222222222222222222222222222222",
  fileChanges: [{ path: "src/example.ts", oldPath: null, status: "modified" as const }],
  pullRequest: { owner: "octo-org", repository: "narrant", number: 42, title: "Review tabs", description: "Review context", author: "octocat", state: "open" as const, draft: false, url: "https://github.com/octo-org/narrant/pull/42", baseRef: "main", headRef: "topic", synchronizedAt: 1 },
};

beforeEach(async () => { await i18n.changeLanguage("zh-CN"); });
afterEach(() => cleanup());

describe("ReviewReader", () => {
  it("默认展示概览，并通过标签切换到文件改动", () => {
    render(<ReviewReader snapshot={snapshot} diffMode="inline" narrow={false} editorTheme="vs" initialTreeWidth={25} onSelectFile={vi.fn()} onRetryFile={vi.fn()} onDiffModeChange={vi.fn()} onTreeWidthChange={vi.fn()} onOpenExternal={vi.fn()} onClose={vi.fn()} />);

    expect(screen.getByRole("tab", { name: "审核概览" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByText("Review context")).toBeVisible();
    fireEvent.click(screen.getByRole("tab", { name: "File Changes" }));
    expect(screen.getByRole("tab", { name: "File Changes" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("navigation", { name: "文件树" })).toBeVisible();
  });

  it("只用一条审核工具栏承载视图标签和右侧操作", () => {
    render(<ReviewReader snapshot={snapshot} diffMode="inline" narrow={false} editorTheme="vs" initialTreeWidth={25} updateAction={<button type="button">刷新审核</button>} onSelectFile={vi.fn()} onRetryFile={vi.fn()} onDiffModeChange={vi.fn()} onTreeWidthChange={vi.fn()} onOpenExternal={vi.fn()} onClose={vi.fn()} />);

    expect(screen.queryByRole("heading", { name: "#42 Review tabs" })).not.toBeInTheDocument();
    const toolbar = screen.getByRole("toolbar", { name: "审核工具栏" });
    expect(within(toolbar).getByRole("tab", { name: "审核概览" })).toBeVisible();
    expect(within(toolbar).getByRole("button", { name: "刷新审核" })).toBeVisible();
    expect(within(toolbar).getByRole("button", { name: "退出审核" })).toBeVisible();
    expect(within(toolbar).queryByRole("button", { name: "切换为双栏" })).not.toBeInTheDocument();

    fireEvent.click(within(toolbar).getByRole("tab", { name: "File Changes" }));
    expect(within(toolbar).getByRole("button", { name: "切换为双栏" })).toBeVisible();
  });
});
