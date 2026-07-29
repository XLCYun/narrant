import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { RepositoryOverview, type PullRequestGroup, type SavedReviewSummary } from "@/components/repository-overview";
import { i18n } from "@/lib/i18n";

const repository = { owner: "octo", name: "narrant" };
const pullRequest = {
  number: 40,
  title: "Repository overview",
  description: "",
  author: "octocat",
  state: "open",
  draft: false,
  url: "https://github.com/octo/narrant/pull/40",
  base: { ref: "main", sha: "base-new" },
  head: { ref: "feature", sha: "head-new" },
};
const saved: SavedReviewSummary = {
  reviewId: "review-40",
  baseRevision: "base-old",
  headRevision: "head-old",
  pullRequest: {
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
  },
};

beforeEach(async () => { await i18n.changeLanguage("zh-CN"); });
afterEach(cleanup);

function renderOverview(options: {
  savedReviews?: SavedReviewSummary[];
  groups?: PullRequestGroup[];
  loading?: boolean;
  discoveryError?: string;
  connected?: boolean;
} = {}) {
  const onOpenSavedReview = vi.fn();
  const onOpenPullRequest = vi.fn();
  render(<RepositoryOverview
    repositoryName="narrant"
    repositoryPath="/work/narrant"
    savedReviews={options.savedReviews ?? []}
    pullRequestGroups={options.groups ?? []}
    repositoryReviews={[]}
    discoveryLoading={options.loading ?? false}
    discoveryError={options.discoveryError}
    githubConnected={options.connected ?? true}
    onRetryDiscovery={vi.fn()}
    onConnectGitHub={vi.fn()}
    onOpenSavedReview={onOpenSavedReview}
    onOpenPullRequest={onOpenPullRequest}
    localBranches={<p>Local branch content</p>}
  />);
  return { onOpenSavedReview, onOpenPullRequest };
}

describe("RepositoryOverview", () => {
  it("默认显示 Pull Requests，并提供三个任务标签", () => {
    renderOverview();
    expect(screen.getByRole("tab", { name: "Pull Requests" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "本地分支" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "审核历史" })).toBeInTheDocument();
  });

  it("后台 discovery 时先保留缓存 Review，失败也能继续并访问本地分支", () => {
    const { onOpenSavedReview } = renderOverview({
      savedReviews: [saved],
      loading: true,
      discoveryError: "network unavailable",
    });
    fireEvent.click(screen.getByRole("button", { name: /继续审核/ }));
    expect(onOpenSavedReview).toHaveBeenCalledWith(saved);
    expect(screen.getByText(/在线元数据可能不是最新/)).toBeInTheDocument();
    expect(screen.getByText(/network unavailable/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("tab", { name: "本地分支" }));
    expect(screen.getByText("Local branch content")).toBeInTheDocument();
  });

  it("合并在线结果并区分开始、继续与有更新", () => {
    const group = { repository, remoteNames: ["origin"], pullRequests: [pullRequest], error: null };
    const { onOpenSavedReview } = renderOverview({
      savedReviews: [saved],
      groups: [group],
    });
    expect(screen.getByText("有更新")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /继续审核/ }));
    expect(onOpenSavedReview).toHaveBeenCalledWith(saved);

    cleanup();
    const fresh = renderOverview({ groups: [group] });
    fireEvent.click(screen.getByRole("button", { name: /开始审核/ }));
    expect(fresh.onOpenPullRequest).toHaveBeenCalledWith(repository, pullRequest);
  });

  it.each([
    [{ connected: false }, "连接 GitHub 以访问授权范围内的 Pull Request"],
    [{ connected: true }, "没有可审核的 Pull Request"],
    [{ connected: true, discoveryError: "request failed" }, "重新尝试"],
  ])("区分 Pull Request 空状态 %#", (options, expected) => {
    renderOverview(options);
    expect(screen.getByText(expected)).toBeInTheDocument();
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
