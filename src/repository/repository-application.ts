import type { RecentReviewSummary, RepositoryReviewSummary } from "@/lib/review-types";

export type Repository = {
  id: number;
  canonicalPath: string;
  gitCommonDirectory: string;
  createdAtMillis: number;
  lastOpenedAtMillis: number;
};

export type RepositoryAvailability =
  | { kind: "available" }
  | { kind: "unavailable"; reason: string };

export type RepositorySummary = Repository & {
  availability: RepositoryAvailability;
};

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

export type BranchRefs = { refs: string[]; defaultRef: string; currentRef: string };

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
