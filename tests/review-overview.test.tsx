import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { i18n } from "@/lib/i18n";
import { ReviewOverview } from "@/components/review-overview";

const writeText = vi.fn();
const review = {
  baseRef: "refs/heads/main", baseRevision: "1111111111111111111111111111111111111111",
  headRef: "refs/heads/feature/safe-markdown", headRevision: "2222222222222222222222222222222222222222",
  pullRequest: {
    owner: "octo-org", repository: "narrant", number: 43, title: "Safe review overview",
    description: "# Context\n\n## User impact\n\n- Easier review\n\nHello <script>alert('no')</script>\n\n![tracker](https://example.com/pixel.png)\n\n[Read more](https://example.com/details)\n\n[Unsafe](javascript:alert('no'))",
    author: "octocat", state: "open" as const, draft: false, url: "https://github.com/octo-org/narrant/pull/43",
    baseRef: "main", headRef: "feature/safe-markdown", synchronizedAt: 1,
  },
};

beforeEach(async () => {
  await i18n.changeLanguage("zh-CN");
  writeText.mockReset();
  vi.stubGlobal("navigator", { clipboard: { writeText } });
});
afterEach(() => cleanup());

describe("ReviewOverview", () => {
  it("用紧凑 revision、完整 Markdown 和安全外链呈现 PR 概览", () => {
    const onOpenExternal = vi.fn();
    render(<ReviewOverview review={review} onOpenExternal={onOpenExternal} />);

    expect(screen.getByRole("heading", { name: "Safe review overview" })).toBeVisible();
    expect(screen.getByText("1111111")).toBeVisible();
    expect(screen.getByRole("heading", { name: "Context" })).toBeVisible();
    expect(screen.getByRole("heading", { name: "Context", level: 1 })).toBeVisible();
    expect(screen.getByRole("heading", { name: "User impact", level: 2 })).toBeVisible();
    expect(screen.getByRole("list")).toBeVisible();
    expect(document.querySelector("script")).not.toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Unsafe" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("link", { name: "Read more" }));
    expect(onOpenExternal).toHaveBeenCalledWith("https://example.com/details");
    fireEvent.click(screen.getByRole("button", { name: "远程图片“tracker”已阻止加载" }));
    expect(onOpenExternal).toHaveBeenLastCalledWith("https://example.com/pixel.png");
  });

  it("复制分支显示值与完整 revision", async () => {
    render(<ReviewOverview review={review} onOpenExternal={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "main" }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith("main"));
    fireEvent.click(screen.getByRole("button", { name: "1111111" }));
    await waitFor(() => expect(writeText).toHaveBeenLastCalledWith(review.baseRevision));
  });

  it("用句子式复合标签表达 Head 合入 Base，并保留独立复制目标", () => {
    render(<ReviewOverview review={review} onOpenExternal={vi.fn()} />);

    const relation = screen.getByLabelText("将 Head 分支 feature/safe-markdown 合入 Base 分支 main");
    expect(relation).toHaveTextContent("将feature/safe-markdown2222222合入main1111111");
    expect(within(relation).getByRole("button", { name: "feature/safe-markdown" })).toBeVisible();
    expect(within(relation).getByRole("button", { name: "2222222" })).toBeVisible();
    expect(within(relation).getByRole("button", { name: "main" })).toBeVisible();
    expect(within(relation).getByRole("button", { name: "1111111" })).toBeVisible();
  });
});
