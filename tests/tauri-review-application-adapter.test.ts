import { describe, expect, it, vi } from "vitest";

import { createTauriBabysitApplicationAdapter } from "@/infrastructure/tauri-babysit-application-adapter";
import { createTauriReviewApplicationAdapter } from "@/infrastructure/tauri-review-application-adapter";
import { createTauriChannel, type TypedTauriTransport } from "@/infrastructure/tauri/transport";
import type { BabysitError } from "@/babysit/types";
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
      repositoryId: 17,
      baseRef: "refs/heads/main",
      headRef: "refs/heads/feature",
      mode: { kind: "start" },
    }, null, signal);

    expect(invokeCancellable).toHaveBeenCalledWith("compare_local_refs", {
      repositoryId: 17,
      baseRef: "refs/heads/main",
      headRef: "refs/heads/feature",
    }, signal);
  });

  it("Branch Selection continuation 通过单个 lifecycle command 重开并刷新", async () => {
    const confirmation = {
      changed: true,
      currentBaseRevision: "base-1",
      currentHeadRevision: "head-1",
      latestBaseRevision: "base-2",
      latestHeadRevision: "head-2",
    };
    const invoke = vi.fn().mockResolvedValue(snapshot);
    const invokeCancellable = vi.fn().mockResolvedValue({
      ...snapshot,
      snapshotId: "snapshot-2",
      baseRevision: "base-2",
      headRevision: "head-2",
    });
    const adapter = createTauriReviewApplicationAdapter({
      invoke,
      invokeCancellable,
    } as unknown as TypedTauriTransport);
    const signal = new AbortController().signal;
    const target = {
      kind: "branch-selection" as const,
      repositoryId: 17,
      baseRef: "refs/heads/main",
      headRef: "refs/heads/feature",
    };

    await adapter.switchReview({
      ...target,
      mode: { kind: "continue", reviewId: "review-1" },
    }, null, signal);
    await adapter.switchReview({
      ...target,
      mode: { kind: "refresh", reviewId: "review-1", confirmation },
    }, null, signal);

    expect(invoke).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenNthCalledWith(1, "reopen_review", { reviewId: "review-1" });
    expect(invokeCancellable).toHaveBeenCalledOnce();
    expect(invokeCancellable).toHaveBeenCalledWith("reopen_and_refresh_branch_review", {
      reviewId: "review-1",
      confirmation,
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
        authorUsername: "octocat",
        state: "open",
        isDraft: false,
        webUrl: "https://github.com/octo-org/octo-repo/pull/42",
        baseRef: "main",
        headRef: "feature",
        synchronizedAtMillis: 1,
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
      repositoryId: 17,
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
      repositoryId: 17,
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

describe("Tauri Babysit application adapter", () => {
  it("通过 typed command 读取后端 eligibility 结论", async () => {
    const invokeCancellable = vi.fn().mockResolvedValue({ eligible: false, reason: "unsupported_language" });
    const adapter = createTauriBabysitApplicationAdapter({ invoke: vi.fn(), invokeCancellable } as unknown as TypedTauriTransport);
    await expect(adapter.checkEligibility(
      { reviewId: "review-1", snapshotId: "snapshot-1", path: "README" },
      "markdown",
      new AbortController().signal,
    )).resolves.toEqual({ eligible: false, reason: "unsupported_language" });
    expect(invokeCancellable).toHaveBeenCalledWith("check_babysit_file_eligibility", {
      reviewId: "review-1", snapshotId: "snapshot-1", path: "README", detectedLanguage: "markdown",
    }, expect.any(AbortSignal));
  });

  it("保留后端结构化 Babysit error code/detail，不压扁为字符串", async () => {
    const error: BabysitError = {
      code: "unsupported_file",
      detail: "lock file",
    };
    const invokeCancellable = vi.fn().mockRejectedValue(error);
    const createChannel = vi.fn(() => ({})) as unknown as typeof createTauriChannel;
    const adapter = createTauriBabysitApplicationAdapter({
      invoke: vi.fn(),
      invokeCancellable,
    } as unknown as TypedTauriTransport, createChannel);

    await expect(adapter.explainFile(
      {
        file: {
          reviewId: "review-1",
          snapshotId: "snapshot-1",
          path: "Cargo.lock",
          locale: "zh-CN",
        },
        generationPolicy: "reuse_cached",
      },
      vi.fn(),
      new AbortController().signal,
    )).rejects.toEqual(error);
    expect(invokeCancellable).toHaveBeenCalledWith(
      "explain_review_file_change",
      expect.objectContaining({
        reviewId: "review-1",
        snapshotId: "snapshot-1",
        path: "Cargo.lock",
        locale: "zh-CN",
        generationPolicy: "reuse_cached",
      }),
      expect.any(AbortSignal),
    );
  });

  it("把 provider identity 与 native picker executable 一起交给通用 setup command", async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);
    const adapter = createTauriBabysitApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await adapter.selectProviderExecutable?.("claude-code", "C:\\Tools\\claude.exe");

    expect(invoke).toHaveBeenCalledWith("select_babysit_provider_executable", {
      providerId: "claude-code",
      executable: "C:\\Tools\\claude.exe",
    });
  });
  it("把 Snapshot/Review 生命周期清理映射到 typed cache commands", async () => {
    const invoke = vi.fn().mockResolvedValue(undefined);
    const adapter = createTauriBabysitApplicationAdapter({
      invoke,
      invokeCancellable: vi.fn(),
    } as unknown as TypedTauriTransport);

    await adapter.clearSnapshot?.({ reviewId: "review-1", snapshotId: "snapshot-1" });
    await adapter.clearReview?.("review-1");

    expect(invoke).toHaveBeenNthCalledWith(1, "clear_babysit_snapshot_cache", {
      reviewId: "review-1",
      snapshotId: "snapshot-1",
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "clear_babysit_review_cache", {
      reviewId: "review-1",
    });
  });
});
