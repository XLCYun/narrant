/** 已持久化的 Pull Request 协作元数据。 */
export type PullRequestMetadata = {
  owner: string; repository: string; number: number; title: string; description: string;
  author: string; state: "open" | "closed" | "merged"; draft: boolean; url: string;
  baseRef: string; headRef: string; synchronizedAt: number;
};

/** GitHub discovery 返回的一个 Pull Request。 */
export type DiscoveredPullRequest = {
  number: number; title: string; description: string; author: string; state: string;
  draft: boolean; url: string;
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

/** 无需在线 discovery 即可打开的持久化 Review 投影。 */
export type SavedReviewSummary = {
  reviewId: string;
  baseRevision: string;
  headRevision: string;
  pullRequest?: PullRequestMetadata;
};

/** Repository Entry 与最近审核列表共享的 Review 投影。 */
export type ReviewSummary = {
  reviewId: string;
  repositoryEntryId: number;
  repositoryPath: string;
  baseRef: string;
  headRef: string;
  lastOpenedAt: number;
  pullRequest?: PullRequestMetadata;
};
