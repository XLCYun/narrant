import { useState } from "react";
import { RepositoryOverview } from "@/components/repository-overview";
import type { DiscoveredPullRequest, PullRequestMetadata, RepositoryReviewSummary } from "@/lib/review-types";

/** 用代表性生命周期与标题长度覆盖筛选、离线和重试场景。 */
type PreviewMode = "mixed" | "closed-only" | "offline" | "retry" | "loading";

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

/** 通过真实概览组件验收筛选，重试会模拟在线状态从 Open 更新为 Merged。 */
export function PullRequestsPreview({ mode = "mixed" }: { mode?: PreviewMode }) {
  const [refreshed, setRefreshed] = useState(false);
  const [opened, setOpened] = useState<string>();
  const pullRequests = mode === "closed-only" ? [closedPullRequest] : [
    { ...openPullRequest, state: refreshed ? "merged" : "open" }, draftPullRequest, closedPullRequest, mergedPullRequest,
  ];
  const reviews = mode === "offline" ? savedReviews : mode === "closed-only" ? [savedReviews[2]] : [savedReviews[0]];
  const groups = mode === "offline" ? [] : [{
    repository: { owner: "example", name: "narrant" }, remoteNames: ["origin"], pullRequests, error: null,
  }];
  return <>
    <RepositoryOverview
      repositoryName="narrant"
      repositoryPath="/work/narrant"
      reviews={reviews}
      pullRequestGroups={mode === "retry" && !refreshed ? [...groups, {
        repository: { owner: "example", name: "upstream" }, remoteNames: ["upstream"], pullRequests: [], error: "GitHub 暂时无法连接",
      }] : groups}
      discoveryLoading={mode === "loading"}
      discoveryError={mode === "offline" ? "当前离线" : undefined}
      githubConnected={mode !== "offline"}
      localBranches={<p>本地分支预览</p>}
      onRetryDiscovery={() => setRefreshed(true)}
      onConnectGitHub={() => setOpened("连接 GitHub")}
      onOpenReview={(review) => setOpened(`继续审核 ${review.reviewId}`)}
      onOpenPullRequest={(_, pullRequest) => setOpened(`开始审核 #${pullRequest.number}`)}
    />
    {opened && <p className="mt-4 text-sm text-muted-foreground" role="status">预览动作：{opened}</p>}
  </>;
}
