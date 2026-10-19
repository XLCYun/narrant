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

/** 当前概览独立保存的生命周期查询，Closed 不包含已合并条目。 */
export type PullRequestStateFilter = "open" | "closed" | "merged" | "all";

/** 单个来源下一次需要读取或重试的页码。 */
export type PullRequestPageRequest = {
  repository: { owner: string; name: string };
  page: number;
};

/** 省略 pages 时读取所有来源首页，传入时只读取指定来源各一页。 */
export type PullRequestDiscoveryQuery = {
  state: PullRequestStateFilter;
  pages?: PullRequestPageRequest[];
};

/** 一个 GitHub repository 的独立 discovery 页；条目保留扫描到的原始生命周期。 */
export type PullRequestGroup = {
  repository: { owner: string; name: string };
  remoteNames: string[];
  pullRequests: DiscoveredPullRequest[];
  error: string | null;
  page: number;
  /** 成功时为下一页，失败时为重试页；null 只代表成功到达末尾。 */
  nextPage: number | null;
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
