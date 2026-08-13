import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { RepositoryOverview, type PullRequestGroup } from "@/components/repository-overview";
import { i18n } from "@/lib/i18n";
import type { RepositoryReviewSummary } from "@/lib/review-types";

const repository = { owner: "octo", name: "narrant" };
const pullRequest = {
  number: 40,
  title: "Repository overview",
  description: "",
  author: "octocat",
  state: "open",
  draft: false,
  url: "https://github.com/octo/narrant/pull/40",
  updatedAt: "2026-07-31T08:00:00Z",
  base: { ref: "main", sha: "base-new" },
  head: { ref: "feature", sha: "head-new" },
};
const saved: RepositoryReviewSummary = {
  reviewId: "review-40",
  selection: { kind: "pull-request", baseRef: "main", headRef: "feature", pullRequest: {
    owner: "octo",
    repository: "narrant",
    number: 40,
    title: "Cached title",
    description: "",
    author: "octocat",
    state: "open",
    draft: false,
    url: pullRequest.url,
    baseRef: "main",
    headRef: "feature",
    synchronizedAt: 1,
  } },
  currentSnapshot: { baseRevision: "base-old", comparisonBase: "merge-base-old", headRevision: "head-old" },
  lastOpenedAt: 1,
};
const historyReview: RepositoryReviewSummary = {
  reviewId: "review-history",
  selection: { kind: "branch", baseRef: "refs/heads/main", headRef: "refs/heads/topic" },
  currentSnapshot: { baseRevision: "base", comparisonBase: "merge-base", headRevision: "head" },
  lastOpenedAt: 1,
};

beforeEach(async () => { await i18n.changeLanguage("zh-CN"); });
afterEach(cleanup);

function renderOverview(options: {
  reviews?: RepositoryReviewSummary[];
  groups?: PullRequestGroup[];
  loading?: boolean;
  discoveryError?: string;
  connected?: boolean;
  openingReviewKey?: string;
  reviewNavigationBusy?: boolean;
} = {}) {
  const onOpenPullRequest = vi.fn();
  const onRetryDiscovery = vi.fn();
  const onConnectGitHub = vi.fn();
  const onOpenReview = vi.fn();
  render(<RepositoryOverview
    repositoryName="narrant"
    repositoryPath="/work/narrant"
    pullRequestGroups={options.groups ?? []}
    reviews={options.reviews ?? []}
    discoveryLoading={options.loading ?? false}
    discoveryError={options.discoveryError}
    githubConnected={options.connected ?? true}
    openingReviewKey={options.openingReviewKey}
    reviewNavigationBusy={options.reviewNavigationBusy}
    onRetryDiscovery={onRetryDiscovery}
    onConnectGitHub={onConnectGitHub}
    onOpenReview={onOpenReview}
    onOpenPullRequest={onOpenPullRequest}
    localBranches={<p>Local branch content</p>}
  />);
  return { onOpenReview, onOpenPullRequest, onRetryDiscovery, onConnectGitHub };
}

describe("RepositoryOverview", () => {
  it("默认显示 Pull Requests，并提供三个任务标签", () => {
    renderOverview();
    expect(screen.getByRole("tab", { name: "Pull Requests" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "本地分支" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "审核历史" })).toBeInTheDocument();
  });

  it("从审核历史整行继续 Review", () => {
    const { onOpenReview } = renderOverview({ reviews: [historyReview] });
    fireEvent.click(screen.getByRole("tab", { name: "审核历史" }));

    const reviewRow = screen.getByRole("button", { name: /继续审核.*topic.*main/ });
    fireEvent.click(reviewRow);

    expect(onOpenReview).toHaveBeenCalledWith(historyReview);
  });

  it("打开审核历史 Review 时保留行身份并阻止重复导航", () => {
    const { onOpenReview } = renderOverview({
      reviews: [historyReview],
      openingReviewKey: "review:review-history",
      reviewNavigationBusy: true,
    });
    fireEvent.click(screen.getByRole("tab", { name: "审核历史" }));

    const reviewRow = screen.getByRole("button", { name: /正在打开 Review.*topic.*main/ });
    expect(reviewRow).toBeDisabled();
    expect(reviewRow).toHaveAttribute("aria-busy", "true");
    expect(reviewRow).toHaveTextContent("topic");
    expect(reviewRow).toHaveTextContent("main");
    expect(reviewRow.querySelector(".animate-spin")).toBeInTheDocument();
    fireEvent.click(reviewRow);
    expect(onOpenReview).not.toHaveBeenCalled();
  });

  it("默认隐藏完整路径，并允许通过仓库名称的键盘焦点查看", async () => {
    renderOverview();
    expect(screen.queryByText("/work/narrant")).not.toBeInTheDocument();
    fireEvent.focus(screen.getByRole("button", { name: "narrant" }));
    expect(await screen.findByText("/work/narrant")).toBeVisible();
  });

  it("把多个 GitHub repository 合并为按更新时间排序的列表，并只在多仓库时显示来源", () => {
    const older = { ...pullRequest, number: 99, title: "Older", updatedAt: "2026-07-30T08:00:00Z" };
    const newer = { ...pullRequest, number: 2, title: "Newer", updatedAt: "2026-08-01T08:00:00Z" };
    renderOverview({ groups: [
      { repository, remoteNames: ["origin"], pullRequests: [older], error: null },
      { repository: { owner: "other", name: "fork" }, remoteNames: ["upstream"], pullRequests: [newer], error: null },
    ] });

    const rows = screen.getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent("#2 Newer");
    expect(rows[0]).toHaveTextContent("other/fork");
    expect(rows[1]).toHaveTextContent("octo/narrant");
    expect(screen.queryByText(/remotes:/)).not.toBeInTheDocument();

    cleanup();
    renderOverview({ groups: [{ repository, remoteNames: ["origin"], pullRequests: [older], error: null }] });
    expect(screen.queryByText("octo/narrant")).not.toBeInTheDocument();
  });

  it("后台 discovery 时在 Pull Requests 胶囊显示加载图标且不显示文字提示", () => {
    renderOverview({
      reviews: [saved],
      loading: true,
    });

    const pullRequestsTab = screen.getByRole("tab", { name: "Pull Requests" });
    expect(pullRequestsTab.querySelector(".animate-spin")).toBeInTheDocument();
    expect(screen.queryByText(/正在后台刷新 Pull Requests/)).not.toBeInTheDocument();
    expect(screen.queryByText(/在线元数据可能不是最新/)).not.toBeInTheDocument();
  });

  it("后台 discovery 失败后恢复原图标，保留缓存 Review 并提示元数据可能过期", () => {
    const { onOpenReview } = renderOverview({
      reviews: [saved],
      discoveryError: "network unavailable",
    });
    expect(screen.getByRole("tab", { name: "Pull Requests" }).querySelector(".animate-spin")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /继续审核/ }));
    expect(onOpenReview).toHaveBeenCalledWith(saved);
    expect(screen.getByText(/在线元数据可能不是最新/)).toBeInTheDocument();
    expect(screen.getByText(/network unavailable/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "本地分支" }));
    expect(screen.getByText("Local branch content")).toBeInTheDocument();
  });

  it("合并在线结果并区分开始、继续与有更新", () => {
    const group = { repository, remoteNames: ["origin"], pullRequests: [pullRequest], error: null };
    const { onOpenReview } = renderOverview({
      reviews: [saved],
      groups: [group],
    });
    expect(screen.getByText("有更新")).toBeInTheDocument();
    const continueButton = screen.getByRole("button", { name: /继续审核/ });
    expect(continueButton).toHaveTextContent(/^继续审核$/);
    fireEvent.click(continueButton);
    expect(onOpenReview).toHaveBeenCalledWith(saved);

    cleanup();
    const fresh = renderOverview({ groups: [group] });
    const startButton = screen.getByRole("button", { name: /开始审核/ });
    expect(startButton).toHaveTextContent(/^开始审核$/);
    fireEvent.click(startButton);
    expect(fresh.onOpenPullRequest).toHaveBeenCalledWith(repository, pullRequest);
  });

  it("打开 Pull Request Review 时立即在目标按钮显示 Spinner", () => {
    renderOverview({
      groups: [{ repository, remoteNames: ["origin"], pullRequests: [pullRequest], error: null }],
      openingReviewKey: "octo/narrant#40",
      reviewNavigationBusy: true,
    });

    const openingButton = screen.getByRole("button", { name: /正在打开 Review #40/ });
    expect(openingButton).toBeDisabled();
    expect(openingButton).toHaveAttribute("aria-busy", "true");
    expect(openingButton).toHaveClass("aria-busy:cursor-default", "aria-busy:pointer-events-auto");
    expect(openingButton.querySelector(".animate-spin")).toBeInTheDocument();
  });

  it("多个 repository 局部失败时保留成功结果并提供一次恢复操作", () => {
    const { onRetryDiscovery } = renderOverview({ groups: [
      { repository, remoteNames: ["origin"], pullRequests: [pullRequest], error: null },
      { repository: { owner: "other", name: "fork" }, remoteNames: ["upstream"], pullRequests: [], error: "request failed" },
    ] });
    expect(screen.getByText("#40 Repository overview")).toBeVisible();
    expect(screen.getByText(/other\/fork/)).toHaveTextContent(/request failed/);
    fireEvent.click(screen.getByRole("button", { name: "重新尝试" }));
    expect(onRetryDiscovery).toHaveBeenCalledOnce();
  });

  it("部分 repository 成功但没有 PR 时仍把失败作为局部提示", () => {
    renderOverview({ groups: [
      { repository, remoteNames: ["origin"], pullRequests: [], error: null },
      { repository: { owner: "other", name: "fork" }, remoteNames: ["upstream"], pullRequests: [], error: "request failed" },
    ] });
    expect(screen.getByText(/other\/fork/)).toHaveTextContent(/request failed/);
    expect(screen.getByText("没有可审核的 Pull Request")).toBeVisible();
    expect(screen.queryByText("无法加载 Pull Requests")).not.toBeInTheDocument();
  });

  it.each([
    [{ connected: false }, "连接 GitHub 以访问授权范围内的 Pull Request"],
    [{ connected: true }, "没有可审核的 Pull Request"],
    [{ connected: true, discoveryError: "request failed" }, "重新尝试"],
  ])("区分 Pull Request 空状态 %#", (options, expected) => {
    renderOverview(options);
    expect(screen.getByText(expected)).toBeInTheDocument();
  });

  it("未连接 GitHub 时把连接作为空状态主操作", () => {
    const { onConnectGitHub } = renderOverview({ connected: false });
    expect(screen.queryByText("连接后可以发现当前账号有权访问的私有仓库。")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "连接 GitHub" }));
    expect(onConnectGitHub).toHaveBeenCalledOnce();
  });

  it("未连接且匿名 discovery 失败时仍优先提供连接 GitHub", () => {
    renderOverview({ connected: false, discoveryError: "private repository" });
    expect(screen.getByRole("button", { name: "连接 GitHub" })).toBeVisible();
    expect(screen.queryByRole("button", { name: "重新尝试" })).not.toBeInTheDocument();
  });

  it("英文 locale 同时翻译标签、状态、恢复动作和 ARIA label", async () => {
    await i18n.changeLanguage("en");
    renderOverview({ discoveryError: "network unavailable", connected: true });

    expect(screen.getByRole("tab", { name: "Local branches" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "Review history" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Try again" })).toBeVisible();
    expect(screen.getByLabelText("Repository tasks")).toBeVisible();
  });
});
