import { describe, expect, it, vi } from "vitest";

import { createTauriReviewApplicationAdapter } from "@/infrastructure/tauri-review-application-adapter";
import type { TypedTauriTransport } from "@/infrastructure/tauri/transport";
import type { ReviewSnapshot } from "@/review/types";

const snapshot: ReviewSnapshot = {
  reviewId: "review-1",
  snapshotId: "snapshot-1",
  baseRef: "refs/heads/main",
  baseRevision: "base-1",
  comparisonBase: "base-1",
  headRef: "refs/heads/feature",
  headRevision: "head-1",
  fileChanges: [],
};

describe("Tauri Review application adapter", () => {
  it("通过 typed transport 重开 saved Review", async () => {
    const invoke = vi.fn().mockResolvedValue(snapshot);
    const transport = {
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport;
    const adapter = createTauriReviewApplicationAdapter(transport);

    await expect(adapter.switchReview(
      { kind: "saved-review", reviewId: "review-1" },
      null,
      new AbortController().signal,
    )).resolves.toBe(snapshot);

    expect(invoke).toHaveBeenCalledWith("reopen_review", { reviewId: "review-1" });
  });

  it("通过 transport 的 cancellation interface 打开 Branch Selection", async () => {
    const invokeCancellable = vi.fn().mockResolvedValue(snapshot);
    const transport = {
      invoke: vi.fn(),
      invokeCancellable,
    } as unknown as TypedTauriTransport;
    const adapter = createTauriReviewApplicationAdapter(transport);
    const signal = new AbortController().signal;

    await adapter.switchReview({
      kind: "branch-selection",
      repositoryEntryId: 17,
      baseRef: "refs/heads/main",
      headRef: "refs/heads/feature",
    }, null, signal);

    expect(invokeCancellable).toHaveBeenCalledWith("compare_local_refs", {
      repositoryEntryId: 17,
      baseRef: "refs/heads/main",
      headRef: "refs/heads/feature",
    }, signal);
  });

  it("把 workspace 清理失败投影为 close result", async () => {
    const transport = {
      invoke: vi.fn()
        .mockResolvedValueOnce(snapshot)
        .mockRejectedValueOnce(new Error("worktree cleanup failed")),
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport;
    const adapter = createTauriReviewApplicationAdapter(transport);
    await adapter.switchReview(
      { kind: "saved-review", reviewId: "review-1" },
      null,
      new AbortController().signal,
    );

    await expect(adapter.closeReview(
      { reviewId: "review-1", snapshotId: "snapshot-1" },
      new AbortController().signal,
    )).resolves.toEqual({
      kind: "closed-with-cleanup-failure",
      detail: "Error: worktree cleanup failed",
    });
  });
});
