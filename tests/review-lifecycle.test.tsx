import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import {
  useReviewLifecycle,
  type ReviewApplicationAdapter,
  type ReviewOpenTarget,
} from "@/review/use-review-lifecycle";
import type {
  FileChangeContent,
  ReviewSnapshot,
} from "@/review/types";

const firstSnapshot: ReviewSnapshot = {
  reviewId: "review-1",
  snapshotId: "snapshot-1",
  baseRef: "refs/heads/main",
  baseRevision: "base-1",
  comparisonBase: "merge-base-1",
  headRef: "refs/heads/feature",
  headRevision: "head-1",
  fileChanges: [
    { path: "z-last.ts", oldPath: null, status: "modified" },
    { path: "a-first.ts", oldPath: null, status: "added" },
  ],
};

const availableUpdate = {
  currentBaseRevision: "base-1",
  currentHeadRevision: "head-1",
  availableBaseRevision: "base-1",
  availableHeadRevision: "head-2",
};

/** 为 Review lifecycle interface 提供可预测结果的内存 adapter。 */
function createAdapter(): ReviewApplicationAdapter & {
  failTarget?: ReviewOpenTarget;
} {
  const adapter: ReviewApplicationAdapter & { failTarget?: ReviewOpenTarget } = {
    async switchReview(target) {
      if (target === adapter.failTarget) throw new Error("目标 Review 无法 materialize");
      return firstSnapshot;
    },
    async checkReviewUpdate() {
      return null;
    },
    async checkBranchSelectionUpdate() {
      return {
        changed: false,
        currentBaseRevision: "base-1",
        currentHeadRevision: "head-1",
        latestBaseRevision: "base-1",
        latestHeadRevision: "head-1",
      };
    },
    async refreshReview() {
      return { kind: "unchanged" };
    },
    async readFileChange(_identity, path) {
      return {
        path,
        oldPath: null,
        status: path === "a-first.ts" ? "added" : "modified",
        contentKind: "text",
        beforeSize: 0,
        afterSize: 3,
        before: "",
        after: "new",
      } satisfies FileChangeContent;
    },
    async closeReview() {
      return { kind: "closed" };
    },
  };
  return adapter;
}

describe("Review lifecycle module", () => {
  it("打开 Review 后按稳定路径顺序选择第一个 File Change", async () => {
    const adapter = createAdapter();
    const { result } = renderHook(() => useReviewLifecycle(adapter));

    await act(async () => {
      const outcome = await result.current.open({ kind: "saved-review", reviewId: "review-1" });
      expect(outcome.kind).toBe("opened");
    });

    await waitFor(() => expect(result.current.reader?.fileChange).toMatchObject({
      kind: "ready",
      path: "a-first.ts",
    }));
  });

  it("目标 Review 打开失败时保留原 Review Reader", async () => {
    const adapter = createAdapter();
    const failedTarget = { kind: "saved-review", reviewId: "review-2" } as const;
    const { result } = renderHook(() => useReviewLifecycle(adapter));
    await act(async () => {
      await result.current.open({ kind: "saved-review", reviewId: "review-1" });
    });
    adapter.failTarget = failedTarget;

    await act(async () => {
      const outcome = await result.current.open(failedTarget);
      expect(outcome).toMatchObject({ kind: "failed", previousReviewPreserved: true });
    });

    expect(result.current.reader?.snapshot).toBe(firstSnapshot);
  });

  it("切换 Review 时取消刷新并等待刷新稳定终止", async () => {
    const events: string[] = [];
    let finishRefresh: (() => void) | undefined;
    const adapter = createAdapter();
    adapter.checkReviewUpdate = async () => availableUpdate;
    adapter.refreshReview = async (_snapshot, _confirmation, signal) => {
      events.push("refresh-started");
      await new Promise<void>((resolve) => {
        finishRefresh = () => {
          events.push(signal.aborted ? "refresh-cancelled" : "refresh-finished");
          resolve();
        };
      });
      throw new DOMException("cancelled", "AbortError");
    };
    adapter.switchReview = async (target) => {
      events.push(`open:${target.kind === "saved-review" ? target.reviewId : target.kind}`);
      return firstSnapshot;
    };
    const { result } = renderHook(() => useReviewLifecycle(adapter));
    await act(async () => {
      await result.current.open({ kind: "saved-review", reviewId: "review-1" });
    });

    let refresh: Promise<unknown>;
    let switching: Promise<unknown>;
    await act(async () => {
      refresh = result.current.reader!.refresh();
      await Promise.resolve();
      switching = result.current.open({ kind: "saved-review", reviewId: "review-2" });
      await Promise.resolve();
    });

    expect(events).toEqual(["open:review-1", "refresh-started"]);
    await act(async () => {
      finishRefresh?.();
      await Promise.all([refresh!, switching!]);
    });
    expect(events).toEqual([
      "open:review-1",
      "refresh-started",
      "refresh-cancelled",
      "open:review-2",
    ]);
  });

  it("关闭 Review 时取消刷新并等待刷新退出后再清理 workspace", async () => {
    const events: string[] = [];
    let finishRefresh: (() => void) | undefined;
    const adapter = createAdapter();
    adapter.checkReviewUpdate = async () => availableUpdate;
    adapter.refreshReview = async (_snapshot, _confirmation, signal) => {
      events.push("refresh-started");
      await new Promise<void>((resolve) => {
        finishRefresh = () => {
          events.push(signal.aborted ? "refresh-cancelled" : "refresh-finished");
          resolve();
        };
      });
      throw new DOMException("cancelled", "AbortError");
    };
    adapter.closeReview = async () => {
      events.push("workspace-closed");
      return { kind: "closed" };
    };
    const { result } = renderHook(() => useReviewLifecycle(adapter));
    await act(async () => {
      await result.current.open({ kind: "saved-review", reviewId: "review-1" });
    });

    let refresh: Promise<unknown>;
    let closing: Promise<unknown>;
    await act(async () => {
      refresh = result.current.reader!.refresh();
      await Promise.resolve();
      closing = result.current.close();
      await Promise.resolve();
    });

    expect(events).toEqual(["refresh-started"]);
    await act(async () => {
      finishRefresh?.();
      await Promise.all([refresh!, closing!]);
    });
    expect(events).toEqual(["refresh-started", "refresh-cancelled", "workspace-closed"]);
  });
});
