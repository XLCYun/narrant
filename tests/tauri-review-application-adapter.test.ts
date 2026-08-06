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

  it("Pull Request lifecycle 只传 Selection identity 与确认 revisions", async () => {
    const pullRequestSnapshot: ReviewSnapshot = {
      ...snapshot,
      pullRequest: {
        owner: "octo-org",
        repository: "octo-repo",
        number: 42,
        title: "Resolver seam",
        description: "",
        author: "octocat",
        state: "open",
        draft: false,
        url: "https://github.com/octo-org/octo-repo/pull/42",
        baseRef: "main",
        headRef: "feature",
        synchronizedAt: 1,
      },
    };
    const update = {
      updateAvailable: true,
      currentBaseRevision: "base-1",
      currentHeadRevision: "head-1",
      availableBaseRevision: "base-2",
      availableHeadRevision: "head-2",
    };
    const refreshed = { ...pullRequestSnapshot, snapshotId: "snapshot-2", headRevision: "head-2" };
    const invokeCancellable = vi.fn()
      .mockResolvedValueOnce(pullRequestSnapshot)
      .mockResolvedValueOnce(update)
      .mockResolvedValueOnce(refreshed);
    const adapter = createTauriReviewApplicationAdapter({
      invoke: vi.fn(),
      invokeCancellable,
    } as unknown as TypedTauriTransport);
    const signal = new AbortController().signal;

    await adapter.switchReview({
      kind: "pull-request-selection",
      repositoryEntryId: 17,
      owner: "octo-org",
      repository: "octo-repo",
      number: 42,
    }, null, signal);
    await adapter.checkReviewUpdate(pullRequestSnapshot, signal);
    await adapter.refreshReview(pullRequestSnapshot, {
      currentBaseRevision: "base-1",
      currentHeadRevision: "head-1",
      availableBaseRevision: "base-2",
      availableHeadRevision: "head-2",
    }, signal);

    expect(invokeCancellable).toHaveBeenNthCalledWith(1, "open_pull_request_review", {
      repositoryEntryId: 17,
      repository: { owner: "octo-org", name: "octo-repo" },
      number: 42,
    }, signal);
    expect(invokeCancellable).toHaveBeenNthCalledWith(2, "check_pull_request_review_update", {
      reviewId: "review-1",
    }, signal);
    expect(invokeCancellable).toHaveBeenNthCalledWith(3, "refresh_pull_request_review", {
      reviewId: "review-1",
      confirmation: {
        currentBaseRevision: "base-1",
        currentHeadRevision: "head-1",
        availableBaseRevision: "base-2",
        availableHeadRevision: "head-2",
      },
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
