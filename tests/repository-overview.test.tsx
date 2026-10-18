import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import type { ComponentProps } from "react";
import { RepositoryOverview, type PullRequestGroup } from "@/components/repository-overview";
import { i18n } from "@/lib/i18n";
import type { RepositoryReviewSummary } from "@/lib/review-types";

const repository = { owner: "octo", name: "narrant" };
const pullRequest = {
  number: 40,
  title: "Repository overview",
  description: "",
  authorUsername: "octocat",
  state: "open",
  isDraft: false,
  webUrl: "https://github.com/octo/narrant/pull/40",
  githubUpdatedAt: "2026-07-31T08:00:00Z",
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
    authorUsername: "octocat",
    state: "open",
    isDraft: false,
    webUrl: pullRequest.webUrl,
    baseRef: "main",
    headRef: "feature",
    synchronizedAtMillis: 1,
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

/** 用稳定的操作回调渲染概览，并允许模拟 discovery 更新而不重新挂载组件。 */
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
  const overview = <RepositoryOverview
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
  />;
  const view = render(overview);
  return {
    onOpenReview, onOpenPullRequest, onRetryDiscovery, onConnectGitHub,
    rerender: (props: Partial<ComponentProps<typeof RepositoryOverview>>) => view.rerender(<RepositoryOverview {...overview.props} {...props} />),
  };
}

/** 通过可见的状态标签切换筛选，覆盖与用户相同的交互入口。 */
async function selectState(name: string, label = "Pull Request 状态") {
  const filters = screen.getByRole("tablist", { name: label });
  fireEvent.click(within(filters).getByRole("tab", { name, exact: true }));
}

/** 为离线筛选构造独立 Review，保留与在线条目相同的生命周期状态。 */
function savedPullRequest(value: typeof pullRequest): RepositoryReviewSummary {
  if (saved.selection.kind !== "pull-request") throw new Error("Expected Pull Request Selection");
  return {
    ...saved,
    reviewId: `review-${value.number}`,
    selection: { ...saved.selection, pullRequest: {
      ...saved.selection.pullRequest,
      ...value,
      state: value.state as "open" | "closed" | "merged",
    } },
  };
}

describe("RepositoryOverview", () => {
  it.each([
    { groups: [{ repository, remoteNames: ["origin"], pullRequests: [pullRequest], error: null }] },
    {},
    { discoveryError: "network unavailable" },
    { connected: false, discoveryError: "anonymous discovery unavailable" },
  ])("列表、空状态与失败状态都可手动刷新 %#", (options) => {
    const actions = renderOverview(options);
    const refresh = screen.getByRole("button", { name: "刷新列表" });
    expect(refresh).toBeVisible();
    expect(refresh).toBeEnabled();
    fireEvent.click(refresh);
    expect(actions.onRetryDiscovery).toHaveBeenCalledOnce();
    expect(actions.onOpenReview).not.toHaveBeenCalled();
    expect(actions.onOpenPullRequest).not.toHaveBeenCalled();
    expect(actions.onConnectGitHub).not.toHaveBeenCalled();
  });

  it.each([false, true])("加载期间禁用所有 discovery 入口且保留列表：已有内容 %s", (hasRows) => {
    const { onRetryDiscovery } = renderOverview({
      loading: true,
      groups: [
        { repository, remoteNames: ["origin"], pullRequests: hasRows ? [pullRequest] : [], error: null },
        { repository: { owner: "other", name: "fork" }, remoteNames: ["upstream"], pullRequests: [], error: "request failed" },
      ],
    });
    const refresh = screen.getByRole("button", { name: "正在刷新列表" });
    expect(refresh).toBeDisabled();
    expect(refresh).toHaveAttribute("aria-busy", "true");
    expect(refresh.querySelector(".animate-spin")).toHaveAttribute("aria-hidden", "true");
    const retry = screen.getByRole("button", { name: "重新尝试" });
    expect(retry).toBeDisabled();
    fireEvent.click(refresh);
    fireEvent.click(retry);
    expect(onRetryDiscovery).not.toHaveBeenCalled();
    if (hasRows) expect(screen.getByText("#40 Repository overview")).toBeVisible();
  });

  it("阻断错误的重试入口在请求期间禁用", () => {
    const { onRetryDiscovery } = renderOverview({ loading: true, discoveryError: "request failed" });
    const retry = screen.getByRole("button", { name: "重新尝试" });
    expect(retry).toBeDisabled();
    fireEvent.click(retry);
    expect(onRetryDiscovery).not.toHaveBeenCalled();
  });

  it("单仓库刷新失败后保留条目并显示错误与过期提示", () => {
    renderOverview({ groups: [{ repository, remoteNames: ["origin"], pullRequests: [pullRequest], error: "request failed" }] });
    expect(screen.getByText("#40 Repository overview")).toBeVisible();
    expect(screen.getByRole("status")).toHaveTextContent("request failed；在线元数据可能不是最新。");
    expect(screen.getByRole("button", { name: "刷新列表" })).toBeEnabled();
  });

  it("默认显示 Pull Requests，并提供三个任务标签", () => {
    renderOverview();
    expect(screen.getByRole("tab", { name: "Pull Requests" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "本地分支" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "审核历史" })).toBeInTheDocument();
    const filters = screen.getByRole("tablist", { name: "Pull Request 状态" });
    expect(within(filters).getAllByRole("tab").map((tab) => tab.textContent)).toEqual(["已开启", "已关闭", "已合并", "全部"]);
    expect(within(filters).getByRole("tab", { selected: true })).toHaveTextContent("已开启");
  });

  it.each(["online", "saved"])("%s 条目统一按四种状态筛选，Draft 仅在未关闭时归入 Open", async (source) => {
    const values = [
      pullRequest,
      { ...pullRequest, number: 41, title: "Draft change", isDraft: true },
      { ...pullRequest, number: 42, title: "Closed change", state: "closed", isDraft: true },
      { ...pullRequest, number: 43, title: "Merged change", state: "merged" },
    ];
    const reviews = source === "saved" ? values.map(savedPullRequest) : [];
    const groups = source === "online" ? [{ repository, remoteNames: ["origin"], pullRequests: values, error: null }] : [];
    const before = JSON.stringify({ reviews, groups });
    const actions = renderOverview({ reviews, groups, connected: source === "online" });
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByText("#41 Draft change")).toBeVisible();
    expect(screen.queryByText("#42 Closed change")).not.toBeInTheDocument();

    await selectState("已关闭");
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
    expect(screen.getByText("#42 Closed change")).toBeVisible();
    await selectState("已合并");
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
    expect(screen.getByText("#43 Merged change")).toBeVisible();
    await selectState("全部");
    expect(screen.getAllByRole("listitem")).toHaveLength(4);
    await selectState("已开启");
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    expect(JSON.stringify({ reviews, groups })).toBe(before);
    expect(actions.onOpenReview).not.toHaveBeenCalled();
    expect(actions.onOpenPullRequest).not.toHaveBeenCalled();
    expect(actions.onRetryDiscovery).not.toHaveBeenCalled();
    expect(actions.onConnectGitHub).not.toHaveBeenCalled();
  });

  it("在线最新状态覆盖已保存状态，继续审核仍复用原 Review", async () => {
    const { onOpenReview } = renderOverview({
      reviews: [saved],
      groups: [{ repository, remoteNames: ["origin"], pullRequests: [{ ...pullRequest, state: "merged" }], error: null }],
    });
    expect(screen.getByText("没有已开启的 Pull Request")).toBeVisible();
    expect(screen.queryByRole("listitem")).not.toBeInTheDocument();
    await selectState("已关闭");
    expect(screen.getByText("没有已关闭的 Pull Request")).toBeVisible();
    await selectState("已合并");
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
    expect(screen.getByText("#40 Repository overview")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: /继续审核/ }));
    expect(onOpenReview).toHaveBeenCalledWith(saved);
    expect(saved.selection.kind === "pull-request" && saved.selection.pullRequest.state).toBe("open");
  });

  it("筛选空结果保留离线提示和状态入口，切换 All 后可继续已有 Review", async () => {
    const closed = savedPullRequest({ ...pullRequest, state: "closed" });
    renderOverview({ reviews: [closed], connected: false, discoveryError: "offline" });
    expect(screen.getByText("没有已开启的 Pull Request")).toBeVisible();
    expect(screen.getByText("可以切换状态查看其他 Pull Request。")).toBeVisible();
    expect(screen.getByText(/在线元数据可能不是最新/)).toBeVisible();
    expect(screen.queryByRole("button", { name: "连接 GitHub" })).not.toBeInTheDocument();
    await selectState("全部");
    expect(screen.getByRole("button", { name: /继续审核/ })).toBeVisible();
  });

  it("筛选后无结果仍显示局部失败，刷新期间与成功后保留选择", async () => {
    const group = { repository, remoteNames: ["origin"], pullRequests: [pullRequest], error: null };
    const failedGroup = { repository: { owner: "other", name: "fork" }, remoteNames: ["upstream"], pullRequests: [], error: "request failed" };
    const { rerender, onRetryDiscovery } = renderOverview({ reviews: [saved], groups: [group, failedGroup] });
    await selectState("已合并");
    expect(screen.getByText("没有已合并的 Pull Request")).toBeVisible();
    fireEvent.click(screen.getByRole("button", { name: "重新尝试" }));
    expect(onRetryDiscovery).toHaveBeenCalledOnce();
    rerender({ pullRequestGroups: [group, failedGroup], discoveryLoading: true });
    expect(screen.getByRole("tab", { name: "已合并", selected: true })).toBeVisible();
    rerender({ pullRequestGroups: [group], discoveryError: "offline" });
    expect(screen.getByRole("tab", { name: "已合并", selected: true })).toBeVisible();
    rerender({ pullRequestGroups: [{ ...group, pullRequests: [{ ...pullRequest, state: "merged" }] }] });
    expect(screen.getByRole("tab", { name: "已合并", selected: true })).toBeVisible();
    expect(screen.getByRole("button", { name: /继续审核/ })).toBeVisible();
    fireEvent.click(screen.getByRole("tab", { name: "本地分支" }));
    fireEvent.click(screen.getByRole("tab", { name: "Pull Requests" }));
    expect(screen.getByRole("tab", { name: "已合并", selected: true })).toBeVisible();
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
    const older = { ...pullRequest, number: 99, title: "Older", githubUpdatedAt: "2026-07-30T08:00:00Z" };
    const newer = { ...pullRequest, number: 2, title: "Newer", githubUpdatedAt: "2026-08-01T08:00:00Z" };
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
    expect(screen.getByText("private repository")).toBeVisible();
    expect(screen.getByRole("button", { name: "刷新列表" })).toBeEnabled();
    expect(screen.queryByRole("button", { name: "重新尝试" })).not.toBeInTheDocument();
  });

  it("英文 locale 同时翻译标签、状态、恢复动作和 ARIA label", async () => {
    await i18n.changeLanguage("en");
    const { rerender } = renderOverview({ discoveryError: "network unavailable", connected: true });

    expect(screen.getByRole("tab", { name: "Local branches" })).toBeVisible();
    expect(screen.getByRole("tab", { name: "Review history" })).toBeVisible();
    expect(screen.getByRole("button", { name: "Try again" })).toBeVisible();
    expect(screen.getByLabelText("Repository tasks")).toBeVisible();
    expect(screen.getByRole("button", { name: "Refresh list" })).toBeEnabled();
    rerender({ discoveryLoading: true });
    expect(screen.getByRole("button", { name: "Refreshing list" })).toBeDisabled();
  });

  it("英文 locale 翻译筛选入口与空结果说明", async () => {
    await i18n.changeLanguage("en");
    renderOverview({ reviews: [saved] });
    const filters = screen.getByRole("tablist", { name: "Pull Request status" });
    expect(within(filters).getAllByRole("tab").map((tab) => tab.textContent)).toEqual(["Open", "Closed", "Merged", "All"]);
    await selectState("Closed", "Pull Request status");
    expect(screen.getByText("No Closed Pull Requests")).toBeVisible();
    expect(screen.getByText("Choose another status to see other Pull Requests.")).toBeVisible();
  });
});
