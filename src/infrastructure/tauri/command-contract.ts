import type { GitHubIdentity } from "@/components/github-identity-entry";
import type { DeviceAuthorization, GitHubAuthStatus } from "@/github/github-oauth-application";
import type { PullRequestGroup } from "@/lib/review-types";
import type { FileChangeContent, ReviewSnapshot } from "@/review/types";
import type { BranchSelectionUpdate } from "@/review/review-application";
import type {
  BranchRefs,
  OpenRepositoryResult,
  RepositoryEntry,
} from "@/repository/repository-application";
import type {
  RepositoryCatalogProjection,
  RepositoryContext,
  RepositoryReviewsProjection,
} from "@/repository/repository-application";

export type PullRequestReviewUpdate = {
  updateAvailable: boolean;
  currentBaseRevision: string;
  currentHeadRevision: string;
  availableBaseRevision: string;
  availableHeadRevision: string;
};

export type ConfirmedPullRequestUpdate = Omit<PullRequestReviewUpdate, "updateAvailable">;

type GitHubRepository = PullRequestGroup["repository"];
type EmptyArgs = Record<string, never>;

export type TauriCommandContract = {
  cancel_operation: { args: { operationId: string }; result: void };
  compare_local_refs: { args: { operationId: string; repositoryEntryId: number; baseRef: string; headRef: string }; result: ReviewSnapshot };
  check_branch_review_updates: { args: { operationId: string; reviewId: string }; result: BranchSelectionUpdate };
  reopen_and_refresh_branch_review: { args: { operationId: string; reviewId: string; confirmation: BranchSelectionUpdate }; result: ReviewSnapshot };
  refresh_branch_review: { args: { operationId: string; reviewId: string; confirmation: BranchSelectionUpdate }; result: ReviewSnapshot };
  open_pull_request_review: { args: { operationId: string; repositoryEntryId: number; repository: GitHubRepository; number: number }; result: ReviewSnapshot };
  check_pull_request_review_update: { args: { operationId: string; reviewId: string }; result: PullRequestReviewUpdate };
  refresh_pull_request_review: { args: { operationId: string; reviewId: string; confirmation: ConfirmedPullRequestUpdate }; result: ReviewSnapshot };
  list_local_refs: { args: { operationId: string; repositoryEntryId: number }; result: BranchRefs };
  read_review_file_change: { args: { operationId: string; reviewId: string; snapshotId: string; path: string }; result: FileChangeContent };
  close_review: { args: { reviewId: string }; result: void };
  discover_github_pull_requests: { args: { repositoryEntryId: number }; result: PullRequestGroup[] };
  github_auth_status: { args: EmptyArgs; result: GitHubAuthStatus };
  refresh_github_identity: { args: EmptyArgs; result: GitHubIdentity };
  start_github_authorization: { args: EmptyArgs; result: DeviceAuthorization };
  complete_github_authorization: { args: { authorizationId: string }; result: void };
  sign_out_github: { args: EmptyArgs; result: void };
  load_repository_catalog: { args: EmptyArgs; result: RepositoryCatalogProjection };
  open_repository_context: { args: { repositoryEntryId: number }; result: OpenRepositoryResult };
  grant_repository_workspace_trust_and_open: { args: { repositoryEntryId: number }; result: RepositoryContext };
  load_repository_reviews: { args: { repositoryEntryId: number }; result: RepositoryReviewsProjection };
  register_repository_entry: { args: { path: string }; result: RepositoryEntry };
  reopen_review: { args: { reviewId: string }; result: ReviewSnapshot };
};

export type TauriCommandName = keyof TauriCommandContract;
export type CancellableTauriCommandName =
  | "compare_local_refs"
  | "check_branch_review_updates"
  | "reopen_and_refresh_branch_review"
  | "refresh_branch_review"
  | "open_pull_request_review"
  | "check_pull_request_review_update"
  | "refresh_pull_request_review"
  | "list_local_refs"
  | "read_review_file_change";
export type PlainTauriCommandName = Exclude<TauriCommandName, CancellableTauriCommandName>;
export type CommandArgs<K extends TauriCommandName> = TauriCommandContract[K]["args"];
export type CommandResult<K extends TauriCommandName> = TauriCommandContract[K]["result"];
export type CancellableCommandArgs<K extends CancellableTauriCommandName> = Omit<CommandArgs<K>, "operationId">;

/** 与 Rust invoke handler 对齐的完整 command inventory。 */
export const TAURI_COMMAND_NAMES = [
  "register_repository_entry",
  "load_repository_catalog",
  "open_repository_context",
  "grant_repository_workspace_trust_and_open",
  "load_repository_reviews",
  "compare_local_refs",
  "check_branch_review_updates",
  "reopen_and_refresh_branch_review",
  "refresh_branch_review",
  "reopen_review",
  "list_local_refs",
  "cancel_operation",
  "discover_github_pull_requests",
  "open_pull_request_review",
  "check_pull_request_review_update",
  "refresh_pull_request_review",
  "github_auth_status",
  "refresh_github_identity",
  "start_github_authorization",
  "complete_github_authorization",
  "sign_out_github",
  "read_review_file_change",
  "close_review",
] as const satisfies readonly TauriCommandName[];
