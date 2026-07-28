import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { i18n } from "@/lib/i18n";
import { ReviewOverview } from "@/components/review-overview";

const pullRequestReview = {
  baseRef: "main",
  baseRevision: "1111111111111111111111111111111111111111",
  headRef: "feature/safe-markdown",
  headRevision: "2222222222222222222222222222222222222222",
  pullRequest: {
    owner: "octo-org",
    repository: "narrant",
    number: 43,
    title: "Safe review overview",
    description: "Hello <script>alert('no')</script>\n\n![tracker](https://example.com/pixel.png)\n\n[Read more](https://example.com/details)",
    author: "octocat",
    state: "open" as const,
    draft: false,
    url: "https://github.com/octo-org/narrant/pull/43",
    baseRef: "main",
    headRef: "feature/safe-markdown",
    synchronizedAt: 1,
  },
};

beforeEach(async () => {
  await i18n.changeLanguage("zh-CN");
});

afterEach(() => cleanup());

describe("ReviewOverview", () => {
  it("默认关闭，并为 Pull Request 展示完整背景，关闭后恢复触发控件焦点", async () => {
    render(<ReviewOverview review={pullRequestReview} onOpenExternal={vi.fn()} />);

    const trigger = screen.getByRole("button", { name: "打开审核概览" });
    expect(screen.queryByRole("dialog", { name: "审核概览" })).not.toBeInTheDocument();

    fireEvent.click(trigger);
    const overview = await screen.findByRole("dialog", { name: "审核概览" });
    expect(overview).toHaveTextContent("Safe review overview");
    expect(overview).toHaveTextContent("#43");
    expect(overview).toHaveTextContent("octocat");
    expect(overview).toHaveTextContent("开放");
    expect(overview).toHaveTextContent(pullRequestReview.baseRevision);
    expect(overview).toHaveTextContent(pullRequestReview.headRevision);
    expect(screen.getByRole("link", { name: "在 GitHub 查看" })).toHaveAttribute(
      "href",
      pullRequestReview.pullRequest.url,
    );

    fireEvent.click(screen.getByRole("button", { name: "关闭审核概览" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "审核概览" })).not.toBeInTheDocument());
    expect(trigger).toHaveFocus();
  });

  it("为本地分支 Review 复用概览并展示完整 revisions", async () => {
    render(<ReviewOverview review={{
      baseRef: "refs/heads/main",
      baseRevision: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
      headRef: "refs/heads/topic",
      headRevision: "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
    }} onOpenExternal={vi.fn()} />);

    fireEvent.click(screen.getByRole("button", { name: "打开审核概览" }));
    const overview = await screen.findByRole("dialog", { name: "审核概览" });
    expect(overview).toHaveTextContent("本地分支审核");
    expect(overview).toHaveTextContent("refs/heads/main");
    expect(overview).toHaveTextContent("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    expect(overview).toHaveTextContent("refs/heads/topic");
    expect(overview).toHaveTextContent("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
    expect(screen.queryByRole("link", { name: "在 GitHub 查看" })).not.toBeInTheDocument();
  });

  it("跳过原始 HTML，并以可翻译占位说明替代远程图片", async () => {
    render(<ReviewOverview review={pullRequestReview} onOpenExternal={vi.fn()} />);
    fireEvent.click(screen.getByRole("button", { name: "打开审核概览" }));
    await screen.findByRole("dialog", { name: "审核概览" });

    expect(document.querySelector("script")).not.toBeInTheDocument();
    expect(screen.queryByRole("img", { name: "tracker" })).not.toBeInTheDocument();
    expect(screen.getByText("远程图片“tracker”已阻止加载")).toBeInTheDocument();

    await i18n.changeLanguage("en");
    expect(screen.getByText('Remote image "tracker" was blocked')).toBeInTheDocument();
  });

  it("仅将 HTTP/HTTPS 外部链接交给系统默认浏览器", async () => {
    const onOpenExternal = vi.fn();
    render(<ReviewOverview review={pullRequestReview} onOpenExternal={onOpenExternal} />);
    fireEvent.click(screen.getByRole("button", { name: "打开审核概览" }));
    await screen.findByRole("dialog", { name: "审核概览" });

    fireEvent.click(screen.getByRole("link", { name: "Read more" }));
    expect(onOpenExternal).toHaveBeenCalledWith("https://example.com/details");
    fireEvent.click(screen.getByRole("link", { name: "在 GitHub 查看" }));
    expect(onOpenExternal).toHaveBeenLastCalledWith(pullRequestReview.pullRequest.url);
  });
});
