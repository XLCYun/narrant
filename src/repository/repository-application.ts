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

export type BranchRefs = { refs: string[]; defaultRef: string; currentRef: string };

export type OpenRepositoryResult =
  | { kind: "unavailable"; repository: RepositoryEntry }
  | { kind: "trust-required"; repository: RepositoryEntry }
  | { kind: "ready"; context: RepositoryContext };

/** Repository Catalog 与 Workspace Trust 的应用 interface。 */
export interface RepositoryApplicationAdapter {
  loadCatalog(): Promise<{ repositories: RepositoryEntry[]; recentReviews: ReviewSummary[] }>;
  register(path: string): Promise<RepositoryEntry>;
  open(repositoryEntryId: number): Promise<OpenRepositoryResult>;
  grantTrust(repository: RepositoryEntry): Promise<RepositoryContext>;
  loadReviews(repositoryEntryId: number): Promise<Pick<RepositoryContext, "savedReviews" | "reviews">>;
  loadBranches(repositoryEntryId: number, signal: AbortSignal): Promise<BranchRefs>;
  markOpened(repositoryEntryId: number): Promise<RepositoryEntry>;
}
