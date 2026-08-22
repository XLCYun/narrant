import type { GitHubIdentity } from "@/components/github-identity-entry";
import type { DeviceAuthorization, GitHubAuthStatus } from "@/github/github-oauth-application";
import type { PullRequestGroup } from "@/lib/review-types";
import type { FileChangeContent, ReviewSnapshot } from "@/review/types";
import type { BranchSelectionUpdate } from "@/review/review-application";
import type { BabysitEligibility, BabysitError, BabysitFileExplanations, BabysitGenerationPolicy, BabysitProgress } from "@/babysit/types";
import type { Channel } from "@tauri-apps/api/core";
import type {
  BranchRefs,
  OpenRepositoryResult,
  RepositorySummary,
} from "@/repository/repository-application";
import type {
  RepositoryNavigation,
  RepositoryOverview,
  RepositoryReviewList,
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
  set_e2e_repository_directory: { args: { path: string }; result: void };
  choose_e2e_repository_directory: { args: EmptyArgs; result: string };
  cancel_operation: { args: { operationId: string }; result: void };
  compare_local_refs: { args: { operationId: string; repositoryId: number; baseRef: string; headRef: string }; result: ReviewSnapshot };
  check_branch_review_updates: { args: { operationId: string; reviewId: string }; result: BranchSelectionUpdate };
  reopen_and_refresh_branch_review: { args: { operationId: string; reviewId: string; confirmation: BranchSelectionUpdate }; result: ReviewSnapshot };
  refresh_branch_review: { args: { operationId: string; reviewId: string; confirmation: BranchSelectionUpdate }; result: ReviewSnapshot };
  open_pull_request_review: { args: { operationId: string; repositoryId: number; repository: GitHubRepository; number: number }; result: ReviewSnapshot };
  check_pull_request_review_update: { args: { operationId: string; reviewId: string }; result: PullRequestReviewUpdate };
  refresh_pull_request_review: { args: { operationId: string; reviewId: string; confirmation: ConfirmedPullRequestUpdate }; result: ReviewSnapshot };
  list_local_refs: { args: { operationId: string; repositoryId: number }; result: BranchRefs };
  read_review_file_change: { args: { operationId: string; reviewId: string; snapshotId: string; path: string }; result: FileChangeContent };
  explain_review_file_change: { args: { operationId: string; reviewId: string; snapshotId: string; path: string; locale: string; detectedLanguage?: string; generationPolicy: BabysitGenerationPolicy; progress: Channel<BabysitProgress> }; result: BabysitFileExplanations; error: BabysitError };
  check_babysit_file_eligibility: { args: { operationId: string; reviewId: string; snapshotId: string; path: string; detectedLanguage?: string }; result: BabysitEligibility; error: BabysitError };
  select_babysit_codex_executable: { args: { executable: string }; result: void; error: BabysitError };
  clear_babysit_snapshot_cache: { args: { reviewId: string; snapshotId: string }; result: void; error: BabysitError };
  clear_babysit_review_cache: { args: { reviewId: string }; result: void; error: BabysitError };
  close_review: { args: { reviewId: string }; result: void };
  discover_github_pull_requests: { args: { repositoryId: number }; result: PullRequestGroup[] };
  github_auth_status: { args: EmptyArgs; result: GitHubAuthStatus };
  refresh_github_identity: { args: EmptyArgs; result: GitHubIdentity };
  start_github_authorization: { args: EmptyArgs; result: DeviceAuthorization };
  complete_github_authorization: { args: { authorizationId: string }; result: void };
  sign_out_github: { args: EmptyArgs; result: void };
  load_repository_navigation: { args: EmptyArgs; result: RepositoryNavigation };
  open_repository_overview: { args: { repositoryId: number }; result: OpenRepositoryResult };
  grant_repository_workspace_trust_and_open: { args: { repositoryId: number }; result: RepositoryOverview };
  load_repository_reviews: { args: { repositoryId: number }; result: RepositoryReviewList };
  register_repository: { args: { path: string }; result: RepositorySummary };
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
  | "read_review_file_change"
  | "explain_review_file_change"
  | "check_babysit_file_eligibility";
  // Setup recovery is initiated by the native picker and does not hold an analysis operation.
export type PlainTauriCommandName = Exclude<TauriCommandName, CancellableTauriCommandName>;
export type CommandArgs<K extends TauriCommandName> = TauriCommandContract[K]["args"];
export type CommandResult<K extends TauriCommandName> = TauriCommandContract[K]["result"];
/** 读取 command contract 中声明的结构化失败 payload。 */
export type CommandError<K extends TauriCommandName> = TauriCommandContract[K] extends { error: infer Error } ? Error : unknown;
export type CancellableCommandArgs<K extends CancellableTauriCommandName> = Omit<CommandArgs<K>, "operationId">;

/** 与 Rust command handler 对齐的完整 command inventory。 */
export const TAURI_COMMAND_NAMES = [
  "set_e2e_repository_directory",
  "choose_e2e_repository_directory",
  "register_repository",
  "load_repository_navigation",
  "open_repository_overview",
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
  "explain_review_file_change",
  "check_babysit_file_eligibility",
  "select_babysit_codex_executable",
  "clear_babysit_snapshot_cache",
  "clear_babysit_review_cache",
  "close_review",
] as const satisfies readonly TauriCommandName[];
