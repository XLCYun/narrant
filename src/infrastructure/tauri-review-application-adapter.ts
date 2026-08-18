import { tauriTransport, type TypedTauriTransport } from "@/infrastructure/tauri/transport";
import type {
  ReviewApplicationAdapter,
  ReviewOpenTarget,
  ReviewUpdate,
} from "@/review/review-application";
import type { ReviewIdentity, ReviewSnapshot } from "@/review/types";

/** 创建把粗粒度 Review lifecycle 操作映射到 Tauri commands 的生产 adapter。 */
export function createTauriReviewApplicationAdapter(
  transport: TypedTauriTransport = tauriTransport,
): ReviewApplicationAdapter {
  return {
    /** 把 Review 打开意图映射为一个 Tauri lifecycle command。 */
    async switchReview(target: ReviewOpenTarget, _current: ReviewIdentity | null, signal: AbortSignal) {
      let snapshot: ReviewSnapshot;
      if (target.kind === "saved-review") {
        snapshot = await transport.invoke("reopen_review", { reviewId: target.reviewId });
      } else if (target.kind === "branch-selection") {
        if (target.mode.kind === "start") {
          snapshot = await transport.invokeCancellable("compare_local_refs", {
              repositoryId: target.repositoryId,
              baseRef: target.baseRef,
              headRef: target.headRef,
            }, signal);
        } else if (target.mode.kind === "continue") {
          snapshot = await transport.invoke("reopen_review", { reviewId: target.mode.reviewId });
        } else {
          snapshot = await transport.invokeCancellable("reopen_and_refresh_branch_review", {
            reviewId: target.mode.reviewId,
            confirmation: target.mode.confirmation,
          }, signal);
        }
      } else {
        snapshot = await transport.invokeCancellable("open_pull_request_review", {
          repositoryId: target.repositoryId,
          repository: { owner: target.owner, name: target.repository },
          number: target.number,
        }, signal);
      }
      return snapshot;
    },

    /** 检查 Snapshot 所属 Selection 的最新 revisions。 */
    async checkReviewUpdate(current, signal) {
      if (current.pullRequest) {
        const checked = await transport.invokeCancellable("check_pull_request_review_update", {
          reviewId: current.reviewId,
        }, signal);
        if (!checked.updateAvailable || signal.aborted) return null;
        const update = {
          currentBaseRevision: checked.currentBaseRevision,
          currentHeadRevision: checked.currentHeadRevision,
          availableBaseRevision: checked.availableBaseRevision,
          availableHeadRevision: checked.availableHeadRevision,
        } satisfies ReviewUpdate;
        return update;
      }
      const checked = await transport.invokeCancellable("check_branch_review_updates", {
        reviewId: current.reviewId,
      }, signal);
      if (!checked.changed) return null;
      return {
        currentBaseRevision: checked.currentBaseRevision,
        currentHeadRevision: checked.currentHeadRevision,
        availableBaseRevision: checked.latestBaseRevision,
        availableHeadRevision: checked.latestHeadRevision,
      } satisfies ReviewUpdate;
    },

    /** 检查 Branch Selection 并保留 branch-specific transport 投影。 */
    checkBranchSelectionUpdate(reviewId, signal) {
      return transport.invokeCancellable("check_branch_review_updates", { reviewId }, signal);
    },

    /** 使用调用方持有的确认 revisions 显式刷新 Review。 */
    async refreshReview(current, confirmation, signal) {
      const snapshot = current.pullRequest
          ? await transport.invokeCancellable("refresh_pull_request_review", {
            reviewId: current.reviewId,
            confirmation,
          }, signal)
        : await transport.invokeCancellable("refresh_branch_review", {
            reviewId: current.reviewId,
            confirmation: {
              changed: true,
              currentBaseRevision: confirmation.currentBaseRevision,
              currentHeadRevision: confirmation.currentHeadRevision,
              latestBaseRevision: confirmation.availableBaseRevision,
              latestHeadRevision: confirmation.availableHeadRevision,
            },
          }, signal);
      if (snapshot.snapshotId === current.snapshotId
        && snapshot.baseRevision === current.baseRevision
        && snapshot.headRevision === current.headRevision) return { kind: "unchanged" };
      return { kind: "refreshed", snapshot };
    },

    /** 读取与不可变 Snapshot identity 绑定的 File Change。 */
    readFileChange(current, path, signal) {
      return transport.invokeCancellable("read_review_file_change", {
        reviewId: current.reviewId,
        snapshotId: current.snapshotId,
        path,
      }, signal);
    },

    /** 关闭 Review；cleanup failure 仍投影为逻辑关闭。 */
    async closeReview(current) {
      try {
        await transport.invoke("close_review", { reviewId: current.reviewId });
        return { kind: "closed" };
      } catch (reason) {
        return { kind: "closed-with-cleanup-failure", detail: String(reason) };
      }
    },
  };
}
