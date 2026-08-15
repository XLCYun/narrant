/** 已持久化的 Pull Request 协作元数据。 */
export type PullRequestMetadata = {
  owner: string; repository: string; number: number; title: string; description: string;
  authorUsername: string; state: "open" | "closed" | "merged"; isDraft: boolean; webUrl: string;
  baseRef: string; headRef: string; githubUpdatedAt?: string; synchronizedAtMillis: number;
};

/** GitHub discovery 返回的一个 Pull Request。 */
export type DiscoveredPullRequest = {
  number: number; title: string; description: string; authorUsername: string; state: string;
  isDraft: boolean; webUrl: string; githubUpdatedAt: string;
  base: { ref: string; sha: string };
  head: { ref: string; sha: string };
};

/** 一个 GitHub repository 的独立 discovery 结果。 */
export type PullRequestGroup = {
  repository: { owner: string; name: string };
  remoteNames: string[];
  pullRequests: DiscoveredPullRequest[];
  error: string | null;
};

/** Repository Overview 中显式呈现的 Review Selection。 */
export type ReviewSelectionSummary =
  | { kind: "branch"; baseRef: string; headRef: string }
  | { kind: "pull-request"; baseRef: string; headRef: string; pullRequest: PullRequestMetadata };

/** Review 当前 Snapshot 的完整 revision identity。 */
export type ReviewSnapshotSummary = {
  baseRevision: string;
  comparisonBase: string;
  headRevision: string;
};

/** Repository 范围内可继续打开的 Review 摘要。 */
export type RepositoryReviewSummary = {
  reviewId: string;
  selection: ReviewSelectionSummary;
  currentSnapshot: ReviewSnapshotSummary;
  lastOpenedAt: number;
};

/** 首页跨 Repository 聚合的最近 Review 摘要。 */
export type RecentReviewSummary = {
  repositoryId: number;
  repositoryPath: string;
  review: RepositoryReviewSummary;
};
