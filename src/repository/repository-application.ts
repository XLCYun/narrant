import type { ReviewSummary, SavedReviewSummary } from "@/lib/review-types";

export type RepositoryEntry = {
  id: number;
  canonicalPath: string;
  commonDirectory: string;
  createdAt: number;
  lastOpenedAt: number;
  available: boolean;
  unavailableReason: string | null;
};

export type RepositoryContext = {
  repository: RepositoryEntry;
  savedReviews: SavedReviewSummary[];
  reviews: ReviewSummary[];
};

/** 当前 Repository 页面所需的 saved Snapshot 与 Review 摘要投影。 */
export type RepositoryReviewsProjection = Pick<RepositoryContext, "savedReviews" | "reviews">;

/** 首页展示的 Repository 列表与全局最近 Review 投影。 */
export type RepositoryCatalogProjection = {
  repositories: RepositoryEntry[];
  recentReviews: ReviewSummary[];
};

export type BranchRefs = { refs: string[]; defaultRef: string; currentRef: string };

export type OpenRepositoryResult =
  | { kind: "unavailable"; repository: RepositoryEntry }
  | { kind: "trust-required"; repository: RepositoryEntry }
  | { kind: "ready"; context: RepositoryContext };

/** Repository Catalog 与 Workspace Trust 的应用 interface。 */
export interface RepositoryApplicationAdapter {
  loadCatalog(): Promise<RepositoryCatalogProjection>;
  register(path: string): Promise<RepositoryEntry>;
  open(repositoryEntryId: number): Promise<OpenRepositoryResult>;
  grantTrust(repository: RepositoryEntry): Promise<RepositoryContext>;
  loadReviews(repositoryEntryId: number): Promise<RepositoryReviewsProjection>;
  loadBranches(repositoryEntryId: number, signal: AbortSignal): Promise<BranchRefs>;
}
