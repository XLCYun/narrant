import { useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import { RepositoryOverview } from "@/components/repository-overview";
import { Button } from "@/components/ui/button";
import { usePullRequestDiscoveryLifecycle } from "@/github/use-pull-request-discovery-lifecycle";
import type { PullRequestDiscoveryApplicationAdapter } from "@/github/pull-request-discovery-application";
import type { DiscoveredPullRequest, PullRequestDiscoveryQuery, PullRequestGroup, PullRequestMetadata, RepositoryReviewSummary } from "@/lib/review-types";

/** 用代表性生命周期与标题长度覆盖筛选、离线和重试场景。 */
type PreviewMode = "mixed" | "closed-only" | "offline" | "retry" | "loading" | "empty" | "failed" | "first-load";

/** 构造与生产 discovery 相同形状的预览条目。 */
function previewPullRequest(number: number, title: string, state: PullRequestMetadata["state"], isDraft = false): DiscoveredPullRequest {
  return {
    number, title, state, isDraft, description: "", authorUsername: "octocat",
    webUrl: `https://github.com/example/narrant/pull/${number}`,
    githubUpdatedAt: "2026-10-17T08:00:00Z",
    base: { ref: "main", sha: "base" },
    head: { ref: `feature/pull-request-${number}`, sha: "head" },
  };
}

/** 从预览条目生成已保存 Review，保持 Review identity 与快照不变。 */
function previewReview(pullRequest: DiscoveredPullRequest, state: PullRequestMetadata["state"]): RepositoryReviewSummary {
  return {
    reviewId: `review-${pullRequest.number}`,
    selection: { kind: "pull-request", baseRef: pullRequest.base.ref, headRef: pullRequest.head.ref, pullRequest: {
      owner: "example", repository: "narrant", number: pullRequest.number, title: pullRequest.title,
      description: "", authorUsername: pullRequest.authorUsername, state, isDraft: pullRequest.isDraft,
      webUrl: pullRequest.webUrl, baseRef: pullRequest.base.ref, headRef: pullRequest.head.ref,
      githubUpdatedAt: pullRequest.githubUpdatedAt, synchronizedAtMillis: 1,
    } },
    currentSnapshot: { baseRevision: "base", comparisonBase: "comparison-base", headRevision: "head" },
    lastOpenedAt: 1,
  };
}

const openPullRequest = previewPullRequest(201, "Fix navigation", "open");
const draftPullRequest = previewPullRequest(202, "让 Repository Overview 在长标题与窄窗口下仍能清楚呈现 Pull Request 状态、草稿标记和审核操作，避免历史条目干扰当前任务", "open", true);
const closedPullRequest = previewPullRequest(203, "Closed proposal", "closed");
const mergedPullRequest = previewPullRequest(204, "Merged improvement", "merged");
const savedReviews = [
  previewReview(openPullRequest, "open"), previewReview(draftPullRequest, "open"),
  previewReview(closedPullRequest, "closed"), previewReview(mergedPullRequest, "merged"),
];

/** 保存可手动完成的请求，让浏览器直接验收生产 lifecycle 的竞态与重试。 */
type PendingRequest = {
  id: number;
  query: PullRequestDiscoveryQuery;
  resolve: (groups: PullRequestGroup[]) => void;
  reject: (reason: Error) => void;
};

/** 根据 REST 页语义生成有限的预览数据，Closed 首页面含 Merged 但没有 Closed。 */
function previewPage(query: PullRequestDiscoveryQuery, mode: PreviewMode, initialFailure = false): PullRequestGroup[] {
  const repository = { owner: "example", name: "narrant" };
  const sources = query.pages ?? [
    { repository, page: 1 },
    ...(mode === "retry" ? [{ repository: { owner: "example", name: "upstream" }, page: 1 }] : []),
  ];
  return sources.map(({ repository: source, page }) => {
    const failed = initialFailure && source.name === "upstream";
    const first = query.state === "open" ? [openPullRequest, draftPullRequest]
      : query.state === "closed" ? [mergedPullRequest]
      : query.state === "merged" ? [mergedPullRequest, closedPullRequest]
      : [openPullRequest, draftPullRequest, mergedPullRequest];
    const empty = mode === "empty" || (mode === "closed-only" && query.state === "open");
    return {
      repository: source, remoteNames: [source.name === "upstream" ? "upstream" : "origin"], page,
      nextPage: failed ? page : empty || page >= 2 ? null : 2,
      error: failed ? "GitHub 暂时无法连接" : null,
      pullRequests: failed || empty ? [] : page === 1 ? first : [query.state === "closed" || query.state === "all"
        ? closedPullRequest : previewPullRequest(205, "Older loaded result", query.state === "merged" ? "merged" : "open")],
    };
  });
}

/** 使用真实分页 lifecycle 与可控 adapter，不访问 Tauri、GitHub 或个人业务数据。 */
export function PullRequestsPreview({ mode = "mixed" }: { mode?: PreviewMode }) {
  const { i18n } = useTranslation();
  const [opened, setOpened] = useState<string>();
  const [pending, setPending] = useState<PendingRequest[]>([]);
  const [requestCount, setRequestCount] = useState(0);
  const readyRef = useRef(false);
  const idRef = useRef(0);
  const adapter = useMemo<PullRequestDiscoveryApplicationAdapter>(() => ({
    /** 首次可立即显示代表性结果；后续由预览控制区逐个完成，支持故意乱序。 */
    discover: (_repositoryId, query) => {
      setRequestCount((count) => count + 1);
      if (!readyRef.current && !["loading", "first-load"].includes(mode)) {
        return Promise.resolve().then(() => {
          readyRef.current = true;
          if (mode === "offline" || mode === "failed") throw new Error("GitHub 暂时无法连接");
          return previewPage(query, mode, mode === "retry");
        });
      }
      return new Promise((resolve, reject) => {
        const id = ++idRef.current;
        setPending((requests) => [...requests, { id, query, resolve, reject }]);
      });
    },
  }), [mode]);
  const discovery = usePullRequestDiscoveryLifecycle(adapter, { enabled: true, repositoryId: 1, signedIn: mode !== "offline" });
  const loading = ["loading", "refreshing", "loading-more"].includes(discovery.transition.kind);
  const reviews = ["empty", "failed", "first-load"].includes(mode) ? [] : mode === "offline" ? savedReviews : mode === "closed-only" ? [savedReviews[2]] : [savedReviews[0]];
  return <>
    <RepositoryOverview
      repositoryName="narrant"
      repositoryPath="/work/narrant"
      reviews={reviews}
      pullRequestGroups={discovery.groups}
      discoveryLoading={loading}
      discoveryLoadingMore={discovery.transition.kind === "loading-more"}
      discoveryComplete={discovery.complete}
      discoveryError={discovery.error?.detail}
      pullRequestFilter={discovery.filter}
      onPullRequestFilterChange={discovery.selectFilter}
      githubConnected={mode !== "offline"}
      localBranches={<p>本地分支预览</p>}
      onRefreshDiscovery={() => void discovery.refresh()}
      onLoadMoreDiscovery={() => void discovery.loadMore()}
      onRetryDiscovery={() => void discovery.retry()}
      onConnectGitHub={() => setOpened("连接 GitHub")}
      onOpenReview={(review) => setOpened(`继续审核 ${review.reviewId}`)}
      onOpenPullRequest={(_, pullRequest) => setOpened(`开始审核 #${pullRequest.number}`)}
    />
    <div className="mt-6 flex flex-wrap items-center gap-3 border-t pt-4" aria-label="预览请求控制">
      <span className="text-sm text-muted-foreground">请求次数：{requestCount}</span>
      <Button variant="outline" size="sm" onClick={() => void i18n.changeLanguage(i18n.resolvedLanguage === "en" ? "zh-CN" : "en")}>中文 / English</Button>
      {pending.map((request) => <div key={request.id} className="flex flex-wrap items-center gap-2 text-sm">
        <span>请求 {request.id}：{request.query.state} / {request.query.pages?.map((page) => `${page.repository.name}:${page.page}`).join(", ") ?? "首页"}</span>
        <Button variant="outline" size="sm" onClick={() => {
          request.resolve(previewPage(request.query, mode));
          setPending((requests) => requests.filter((entry) => entry.id !== request.id));
        }}>完成请求 {request.id}</Button>
        <Button variant="outline" size="sm" onClick={() => {
          request.reject(new Error("GitHub 暂时无法连接"));
          setPending((requests) => requests.filter((entry) => entry.id !== request.id));
        }}>拒绝请求 {request.id}</Button>
      </div>)}
    </div>
    {opened && <p className="mt-4 text-sm text-muted-foreground" role="status">预览动作：{opened}</p>}
  </>;
}
