import type { RecentReviewSummary, RepositoryReviewSummary } from "@/lib/review-types";

/** 注册到 Narrant 并拥有 Review 历史的本地 Git 仓库持久化身份。 */
export type Repository = {
  id: number;
  canonicalPath: string;
  gitCommonDirectory: string;
  createdAtMillis: number;
  lastOpenedAtMillis: number;
};

/** Repository 当前是否仍能解析到登记时的 working tree。 */
export type RepositoryAvailability =
  | { kind: "available" }
  | { kind: "unavailable"; reason: string };

/** Repository 持久化身份及 Application 层实时探测的可用性。 */
export type RepositorySummary = Repository & {
  availability: RepositoryAvailability;
};

/** Repository 页面一次呈现所需的仓库状态和 Review 历史。 */
export type RepositoryOverview = {
  repository: RepositorySummary;
  reviews: RepositoryReviewSummary[];
};

/** 当前 Repository 页面所需的 Review 摘要投影。 */
export type RepositoryReviewList = Pick<RepositoryOverview, "reviews">;

/** 首页展示的 Repository 列表与全局最近 Review 投影。 */
export type RepositoryNavigation = {
  repositories: RepositorySummary[];
  recentReviews: RecentReviewSummary[];
};

/** Repository 可选的本地与远端跟踪分支，以及默认、当前分支。 */
export type BranchRefs = { refs: string[]; defaultRef: string; currentRef: string };

/** 尝试打开 Repository 后的可恢复应用结果。 */
export type OpenRepositoryResult =
  | { kind: "unavailable"; repository: RepositorySummary }
  | { kind: "trust-required"; repository: RepositorySummary }
  | { kind: "ready"; overview: RepositoryOverview };

/** Persistence 与 Workspace Trust 的应用 interface。 */
export interface RepositoryApplicationAdapter {
  loadNavigation(): Promise<RepositoryNavigation>;
  register(path: string): Promise<RepositorySummary>;
  open(repositoryId: number): Promise<OpenRepositoryResult>;
  grantTrust(repository: RepositorySummary): Promise<RepositoryOverview>;
  loadReviews(repositoryId: number): Promise<RepositoryReviewList>;
  loadBranches(repositoryId: number, signal: AbortSignal): Promise<BranchRefs>;
}
