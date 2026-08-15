import type { FileChangeContent, ReviewIdentity, ReviewSnapshot } from "@/review/types";

/** Workspace Trust 已由 Repository context 确认的 Review 打开目标。 */
export type ReviewOpenTarget =
  | { kind: "saved-review"; reviewId: string }
  | {
      kind: "pull-request-selection";
      repositoryId: number;
      owner: string;
      repository: string;
      number: number;
    }
  | {
      kind: "branch-selection";
      repositoryId: number;
      baseRef: string;
      headRef: string;
      mode:
        | { kind: "start" }
        | { kind: "continue"; reviewId: string }
        | { kind: "refresh"; reviewId: string; confirmation: BranchSelectionUpdate };
    };

/** Review Update 检查得到的 moving revision 对照。 */
export type ReviewUpdate = {
  currentBaseRevision: string;
  currentHeadRevision: string;
  availableBaseRevision: string;
  availableHeadRevision: string;
};

export type BranchSelectionUpdate = {
  changed: boolean;
  currentBaseRevision: string;
  currentHeadRevision: string;
  latestBaseRevision: string;
  latestHeadRevision: string;
};

/** Review lifecycle module 所依赖的应用 interface。 */
export interface ReviewApplicationAdapter {
  switchReview(
    target: ReviewOpenTarget,
    current: ReviewIdentity | null,
    signal: AbortSignal,
  ): Promise<ReviewSnapshot>;
  checkReviewUpdate(current: ReviewSnapshot, signal: AbortSignal): Promise<ReviewUpdate | null>;
  checkBranchSelectionUpdate(reviewId: string, signal: AbortSignal): Promise<BranchSelectionUpdate>;
  refreshReview(
    current: ReviewSnapshot,
    confirmation: ReviewUpdate,
    signal: AbortSignal,
  ): Promise<{ kind: "unchanged" } | { kind: "refreshed"; snapshot: ReviewSnapshot }>;
  readFileChange(
    current: ReviewIdentity,
    path: string,
    signal: AbortSignal,
  ): Promise<FileChangeContent>;
  closeReview(
    current: ReviewIdentity,
    signal: AbortSignal,
  ): Promise<{ kind: "closed" } | { kind: "closed-with-cleanup-failure"; detail: string }>;
}
