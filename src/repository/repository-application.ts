import type { RecentReviewSummary, RepositoryReviewSummary } from "@/lib/review-types";

export type RepositoryEntry = {
  id: number;
  canonicalPath: string;
  commonDirectory: string;
  createdAt: number;
  lastOpenedAt: number;
  available: boolean;
  unavailableReason: string | null;
};

export type RepositoryEntryOverview = {
  repository: RepositoryEntry;
  reviews: RepositoryReviewSummary[];
};

/** 当前 Repository 页面所需的 Review 摘要投影。 */
export type RepositoryReviewsProjection = Pick<RepositoryEntryOverview, "reviews">;

/** 首页展示的 Repository 列表与全局最近 Review 投影。 */
export type RepositoryCatalogProjection = {
  repositories: RepositoryEntry[];
  recentReviews: RecentReviewSummary[];
};

export type BranchRefs = { refs: string[]; defaultRef: string; currentRef: string };

export type OpenRepositoryResult =
  | { kind: "unavailable"; repository: RepositoryEntry }
  | { kind: "trust-required"; repository: RepositoryEntry }
  | { kind: "ready"; overview: RepositoryEntryOverview };

/** Repository Catalog 与 Workspace Trust 的应用 interface。 */
export interface RepositoryApplicationAdapter {
  loadCatalog(): Promise<RepositoryCatalogProjection>;
  register(path: string): Promise<RepositoryEntry>;
  open(repositoryEntryId: number): Promise<OpenRepositoryResult>;
  grantTrust(repository: RepositoryEntry): Promise<RepositoryEntryOverview>;
  loadReviews(repositoryEntryId: number): Promise<RepositoryReviewsProjection>;
  loadBranches(repositoryEntryId: number, signal: AbortSignal): Promise<BranchRefs>;
}
