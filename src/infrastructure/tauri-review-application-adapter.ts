import { tauriTransport, type TypedTauriTransport } from "@/infrastructure/tauri/transport";
import { createTauriPullRequestDiscoveryApplicationAdapter } from "@/infrastructure/tauri-pull-request-discovery-application-adapter";
import type { BranchSelectionUpdate } from "@/review/review-application";
import type {
  PullRequestContext,
  PullRequestDiscoveryApplicationAdapter,
} from "@/github/pull-request-discovery-application";
import type {
  ReviewApplicationAdapter,
  ReviewOpenTarget,
  ReviewUpdate,
} from "@/review/review-application";
import type { ReviewIdentity, ReviewSnapshot } from "@/review/types";

type PendingUpdate =
  | { kind: "branch"; confirmation: BranchSelectionUpdate }
  | { kind: "pull-request"; context: PullRequestContext };

/** 创建把粗粒度 Review lifecycle 操作映射到 Tauri commands 的生产 adapter。 */
export function createTauriReviewApplicationAdapter(
  transport: TypedTauriTransport = tauriTransport,
  discovery: PullRequestDiscoveryApplicationAdapter = createTauriPullRequestDiscoveryApplicationAdapter(transport),
): ReviewApplicationAdapter {
  const snapshots = new Map<string, ReviewSnapshot>();
  /** 用户检查是有更新，如果有的话会先存在这里，直到用户点击更新才从这里取出来进行真正的刷新操作 */
  const pendingUpdates = new Map<string, PendingUpdate>();
  const pullRequestContexts = new Map<string, PullRequestContext>();

  /** 按 durable Review identity 找到所属 Repository Entry。 */
  async function repositoryEntryId(reviewId: string) {
    const reviews = await transport.invoke("list_reviews", { repositoryEntryId: null });
    const review = reviews.find((candidate) => candidate.reviewId === reviewId);
    if (!review) throw new Error("Review 所属 Repository Entry 不存在。");
    return review.repositoryEntryId;
  }

  /** 只按 Pull Request Selection identity 解析最新 discovery metadata。 */
  async function discoverPullRequest(
    reviewId: string,
    snapshot: ReviewSnapshot,
  ): Promise<PullRequestContext> {
    const identity = snapshot.pullRequest;
    if (!identity) throw new Error("Review 不是 Pull Request Selection。");
    return discovery.resolve({
      repositoryEntryId: await repositoryEntryId(reviewId),
      owner: identity.owner,
      repository: identity.repository,
      number: identity.number,
    });
  }

  /** 记录 adapter 返回的 current Review Snapshot。 */
  function remember(snapshot: ReviewSnapshot) {
    snapshots.set(snapshot.reviewId, snapshot);
    return snapshot;
  }

  return {
    async switchReview(target: ReviewOpenTarget, _current: ReviewIdentity | null, signal: AbortSignal) {
      let snapshot: ReviewSnapshot;
      if (target.kind === "saved-review") {
        snapshot = await transport.invoke("reopen_review", { reviewId: target.reviewId });
      } else if (target.kind === "branch-selection") {
        snapshot = target.refreshExisting && target.existingReviewId
          ? await transport.invoke("reopen_review", { reviewId: target.existingReviewId })
          : await transport.invokeCancellable("compare_local_refs", {
              repositoryEntryId: target.repositoryEntryId,
              baseRef: target.baseRef,
              headRef: target.headRef,
            }, signal);
        if (target.refreshExisting) {
          const update = await transport.invokeCancellable("check_branch_review_updates", {
            reviewId: snapshot.reviewId,
          }, signal);
          if (update.changed) {
            snapshot = await transport.invokeCancellable("refresh_branch_review", {
              reviewId: snapshot.reviewId,
              confirmation: update,
            }, signal);
          }
        }
      } else {
        const context = await discovery.resolve(target);
        snapshot = await transport.invokeCancellable("open_pull_request_review", {
          repositoryEntryId: target.repositoryEntryId,
          repository: context.repository,
          pullRequest: context.pullRequest,
        }, signal);
        pullRequestContexts.set(snapshot.reviewId, context);
      }
      return remember(snapshot);
    },

    async checkReviewUpdate(current, signal) {
      const snapshot = snapshots.get(current.reviewId);
      if (!snapshot || snapshot.snapshotId !== current.snapshotId) return null;
      if (snapshot.pullRequest) {
        const context = pullRequestContexts.get(current.reviewId)
          ?? await discoverPullRequest(current.reviewId, snapshot);
        const checked = await transport.invokeCancellable("check_pull_request_review_update", {
          reviewId: current.reviewId,
          repository: context.repository,
          pullRequest: context.pullRequest,
        }, signal);
        if (!checked.updateAvailable || signal.aborted) return null;
        pendingUpdates.set(current.snapshotId, { kind: "pull-request", context });
        return checked;
      }
      const checked = await transport.invokeCancellable("check_branch_review_updates", {
        reviewId: current.reviewId,
      }, signal);
      if (!checked.changed) return null;
      pendingUpdates.set(current.snapshotId, { kind: "branch", confirmation: checked });
      return {
        currentBaseRevision: checked.currentBaseRevision,
        currentHeadRevision: checked.currentHeadRevision,
        availableBaseRevision: checked.latestBaseRevision,
        availableHeadRevision: checked.latestHeadRevision,
      } satisfies ReviewUpdate;
    },

    checkBranchSelectionUpdate(reviewId, signal) {
      return transport.invokeCancellable("check_branch_review_updates", { reviewId }, signal);
    },

    async refreshReview(current, signal) {
      const pending = pendingUpdates.get(current.snapshotId);
      if (!pending) return { kind: "unchanged" };
      const snapshot = pending.kind === "branch"
        ? await transport.invokeCancellable("refresh_branch_review", {
            reviewId: current.reviewId,
            confirmation: pending.confirmation,
          }, signal)
        : await transport.invokeCancellable("refresh_pull_request_review", {
            reviewId: current.reviewId,
            repository: pending.context.repository,
            pullRequest: pending.context.pullRequest,
          }, signal);
      pendingUpdates.delete(current.snapshotId);
      const previous = snapshots.get(current.reviewId);
      if (snapshot.snapshotId === current.snapshotId
        && snapshot.baseRevision === previous?.baseRevision
        && snapshot.headRevision === previous?.headRevision) return { kind: "unchanged" };
      return { kind: "refreshed", snapshot: remember(snapshot) };
    },

    readFileChange(current, path, signal) {
      return transport.invokeCancellable("read_review_file_change", {
        reviewId: current.reviewId,
        snapshotId: current.snapshotId,
        path,
      }, signal);
    },

    async closeReview(current) {
      try {
        await transport.invoke("close_review", { reviewId: current.reviewId });
        return { kind: "closed" };
      } catch (reason) {
        return { kind: "closed-with-cleanup-failure", detail: String(reason) };
      } finally {
        snapshots.delete(current.reviewId);
        pullRequestContexts.delete(current.reviewId);
        pendingUpdates.delete(current.snapshotId);
      }
    },
  };
}
