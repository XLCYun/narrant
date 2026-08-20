import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import {
  useReviewLifecycle,
  type ReviewApplicationAdapter,
  type ReviewOpenTarget,
} from "@/review/use-review-lifecycle";
import type {
  FileChangeContent,
  ReviewFileViewState,
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

/** 构造无需依赖 Monaco DOM 的可观察 Diff view state。 */
function diffViewState(marker: string): ReviewFileViewState {
  return { original: null, modified: null, modelState: { marker } };
}

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

  it("按 reviewId 保留完整 Reader 状态，并在 A → B → A 时复用缓存", async () => {
    const snapshots: Record<string, ReviewSnapshot> = {
      "review-a": { ...firstSnapshot, reviewId: "review-a", snapshotId: "snapshot-a", fileChanges: [
        { path: "a.ts", oldPath: null, status: "modified" },
        { path: "nested/b.ts", oldPath: null, status: "added" },
      ] },
      "review-b": { ...firstSnapshot, reviewId: "review-b", snapshotId: "snapshot-b", fileChanges: [
        { path: "b.ts", oldPath: null, status: "modified" },
      ] },
    };
    const switchReview = vi.fn(async (target: ReviewOpenTarget) => {
      const reviewId = target.kind === "saved-review" ? target.reviewId : "review-a";
      return snapshots[reviewId];
    });
    const checkReviewUpdate = vi.fn(async () => null);
    const readFileChange = vi.fn(async (_identity, path) => ({
      path,
      oldPath: null,
      status: "modified" as const,
      contentKind: "text" as const,
      beforeSize: 1,
      afterSize: 2,
      before: "old",
      after: "new",
    }));
    const adapter: ReviewApplicationAdapter = {
      switchReview,
      checkReviewUpdate,
      checkBranchSelectionUpdate: async () => ({
        changed: false,
        currentBaseRevision: "base",
        currentHeadRevision: "head",
        latestBaseRevision: "base",
        latestHeadRevision: "head",
      }),
      refreshReview: async () => ({ kind: "unchanged" }),
      readFileChange,
      closeReview: async () => ({ kind: "closed" }),
    };
    const { result } = renderHook(() => useReviewLifecycle(adapter));

    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-a" }); });
    await waitFor(() => expect(result.current.reader?.fileChange).toMatchObject({ kind: "ready", path: "a.ts" }));
    act(() => {
      result.current.reader?.setView("file-changes");
      result.current.reader?.setDiffMode("inline");
      result.current.reader?.setTreeWidth(37);
      result.current.reader?.toggleDirectory("nested");
      result.current.reader?.setScrollPosition("overview", 41);
      result.current.reader?.setScrollPosition("fileTree", 63);
      result.current.reader?.setScrollPosition("content", 87);
      result.current.reader?.selectFileChange("nested/b.ts");
    });
    await waitFor(() => expect(result.current.reader?.fileChange).toMatchObject({ kind: "ready", path: "nested/b.ts" }));

    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-b" }); });
    await waitFor(() => expect(result.current.reader?.snapshot.reviewId).toBe("review-b"));
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-a" }); });

    expect(switchReview).toHaveBeenCalledTimes(3);
    expect(checkReviewUpdate).toHaveBeenCalledTimes(2);
    expect(result.current.reader?.snapshot.reviewId).toBe("review-a");
    expect(result.current.reader?.fileChange).toMatchObject({ kind: "ready", path: "nested/b.ts" });
    expect(result.current.reader?.view).toBe("file-changes");
    expect(result.current.reader?.diffMode).toBe("inline");
    expect(result.current.reader?.treeWidth).toBe(37);
    expect(result.current.reader?.collapsedDirectories).toEqual(["nested"]);
    expect(result.current.reader?.scrollPositions).toEqual({ overview: 41, fileTree: 63, content: 87 });
  });

  it("切换会结束 inactive checking，warm present 复用缓存且不重复检查", async () => {
    const snapshots: Record<string, ReviewSnapshot> = {
      "review-a": { ...firstSnapshot, reviewId: "review-a", snapshotId: "snapshot-a" },
      "review-b": { ...firstSnapshot, reviewId: "review-b", snapshotId: "snapshot-b" },
    };
    const checks: Array<{
      reviewId: string;
      signal: AbortSignal;
      resolve: (value: null) => void;
    }> = [];
    const adapter = createAdapter();
    adapter.switchReview = async (target) => snapshots[target.kind === "saved-review" ? target.reviewId : "review-a"];
    adapter.checkReviewUpdate = (current, signal) => new Promise((resolve) => {
      checks.push({ reviewId: current.reviewId, signal, resolve });
    });
    const { result } = renderHook(() => useReviewLifecycle(adapter));

    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-a" }); });
    await waitFor(() => expect(checks).toHaveLength(1));
    expect(result.current.reader?.update.kind).toBe("checking");

    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-b" }); });
    await waitFor(() => expect(checks).toHaveLength(2));
    expect(checks[0].signal.aborted).toBe(true);

    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-a" }); });
    expect(checks).toHaveLength(2);
    expect(checks[1].signal.aborted).toBe(true);
    expect(result.current.reader?.update.kind).toBe("current");
  });

  it("打开 C 失败后为仍 Presented 的 B 重启 update check，并隔离旧请求", async () => {
    const adapter = createAdapter();
    const snapshotB = { ...firstSnapshot, reviewId: "review-b", snapshotId: "snapshot-b" };
    const checks: Array<{ signal: AbortSignal; resolve: (value: null) => void }> = [];
    adapter.switchReview = async (target) => {
      if (target.kind === "saved-review" && target.reviewId === "review-c") throw new Error("C materialize failed");
      return snapshotB;
    };
    adapter.checkReviewUpdate = (_current, signal) => new Promise((resolve) => { checks.push({ signal, resolve }); });
    const { result } = renderHook(() => useReviewLifecycle(adapter));

    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-b" }); });
    await waitFor(() => expect(checks).toHaveLength(1));
    expect(result.current.reader?.update.kind).toBe("checking");

    await act(async () => {
      expect(await result.current.open({ kind: "saved-review", reviewId: "review-c" })).toMatchObject({ kind: "failed" });
    });
    await waitFor(() => expect(checks).toHaveLength(2));
    expect(checks[0].signal.aborted).toBe(true);
    expect(result.current.reader?.snapshot.reviewId).toBe("review-b");
    expect(result.current.reader?.update.kind).toBe("checking");

    act(() => checks[0].resolve(null));
    expect(result.current.reader?.update.kind).toBe("checking");
    act(() => checks[1].resolve(null));
    await waitFor(() => expect(result.current.reader?.update.kind).toBe("current"));
  });

  it.each(["current", "available", "failed"] as const)(
    "打开 C 失败时保留 B 的稳定 %s update 状态且不发起额外检查",
    async (stableKind) => {
      const adapter = createAdapter();
      const snapshotB = { ...firstSnapshot, reviewId: "review-b", snapshotId: "snapshot-b" };
      adapter.switchReview = async (target) => {
        if (target.kind === "saved-review" && target.reviewId === "review-c") throw new Error("C materialize failed");
        return snapshotB;
      };
      const checkReviewUpdate = vi.fn(async () => {
        if (stableKind === "failed") throw new Error("update check failed");
        return stableKind === "available" ? availableUpdate : null;
      });
      adapter.checkReviewUpdate = checkReviewUpdate;
      const { result } = renderHook(() => useReviewLifecycle(adapter));

      await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-b" }); });
      await waitFor(() => expect(result.current.reader?.update.kind).toBe(stableKind));
      checkReviewUpdate.mockClear();

      await act(async () => {
        expect(await result.current.open({ kind: "saved-review", reviewId: "review-c" })).toMatchObject({ kind: "failed" });
      });
      expect(result.current.reader?.snapshot.reviewId).toBe("review-b");
      expect(result.current.reader?.update.kind).toBe(stableKind);
      expect(checkReviewUpdate).not.toHaveBeenCalled();
    },
  );

  it("重复选择 Presented Review 在 adapter 前短路", async () => {
    const adapter = createAdapter();
    const switchReview = vi.spyOn(adapter, "switchReview");
    const { result } = renderHook(() => useReviewLifecycle(adapter));
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-1" }); });
    switchReview.mockClear();

    await act(async () => {
      const outcome = await result.current.open({ kind: "saved-review", reviewId: "review-1" });
      expect(outcome.kind).toBe("opened");
    });

    expect(switchReview).not.toHaveBeenCalled();
  });

  it("A → B 的真实切换调用 adapter，只有重复选择当前 B 才短路", async () => {
    const adapter = createAdapter();
    const snapshots: Record<string, ReviewSnapshot> = {
      "review-a": { ...firstSnapshot, reviewId: "review-a", snapshotId: "snapshot-a" },
      "review-b": { ...firstSnapshot, reviewId: "review-b", snapshotId: "snapshot-b" },
    };
    const switchReview = vi.spyOn(adapter, "switchReview").mockImplementation(async (target, current) => {
      const reviewId = target.kind === "saved-review" ? target.reviewId : "review-a";
      if (reviewId === "review-b") expect(current).toEqual({ reviewId: "review-a", snapshotId: "snapshot-a" });
      return snapshots[reviewId];
    });
    const { result } = renderHook(() => useReviewLifecycle(adapter));

    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-a" }); });
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-b" }); });
    expect(switchReview).toHaveBeenCalledTimes(2);
    expect(result.current.reader?.snapshot.reviewId).toBe("review-b");

    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-b" }); });
    expect(switchReview).toHaveBeenCalledTimes(2);
  });

  it("打开 C 失败后仍保留 A/B runtime，并可分别恢复", async () => {
    const adapter = createAdapter();
    const failedTarget = { kind: "saved-review", reviewId: "review-c" } as const;
    const snapshots: Record<string, ReviewSnapshot> = {
      "review-a": { ...firstSnapshot, reviewId: "review-a", snapshotId: "snapshot-a" },
      "review-b": { ...firstSnapshot, reviewId: "review-b", snapshotId: "snapshot-b" },
    };
    adapter.switchReview = async (target) => {
      if (target === failedTarget) throw new Error("C materialize failed");
      return snapshots[target.kind === "saved-review" ? target.reviewId : "review-a"];
    };
    const { result } = renderHook(() => useReviewLifecycle(adapter));
    const stateA = diffViewState("a");
    const stateB = diffViewState("b");

    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-a" }); });
    act(() => result.current.reader?.setFileViewState({ reviewId: "review-a", snapshotId: "snapshot-a", path: "a-first.ts" }, stateA));
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-b" }); });
    act(() => result.current.reader?.setFileViewState({ reviewId: "review-b", snapshotId: "snapshot-b", path: "a-first.ts" }, stateB));

    await act(async () => {
      expect(await result.current.open(failedTarget)).toMatchObject({ kind: "failed", previousReviewPreserved: true });
    });
    expect(result.current.reader?.snapshot.reviewId).toBe("review-b");
    expect(result.current.reader?.fileViewState).toBe(stateB);

    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-a" }); });
    expect(result.current.reader?.fileViewState).toBe(stateA);
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-b" }); });
    expect(result.current.reader?.fileViewState).toBe(stateB);
  });

  it("refresh 只清目标 Review 的 Snapshot state，不影响其他 Open Review", async () => {
    const adapter = createAdapter();
    const snapshots: Record<string, ReviewSnapshot> = {
      "review-a": { ...firstSnapshot, reviewId: "review-a", snapshotId: "snapshot-a" },
      "review-b": { ...firstSnapshot, reviewId: "review-b", snapshotId: "snapshot-b" },
    };
    adapter.switchReview = async (target) => snapshots[target.kind === "saved-review" ? target.reviewId : "review-a"];
    adapter.checkReviewUpdate = async () => availableUpdate;
    adapter.refreshReview = async () => ({
      kind: "refreshed",
      snapshot: { ...snapshots["review-a"], snapshotId: "snapshot-a-2", headRevision: "head-2" },
    });
    const { result } = renderHook(() => useReviewLifecycle(adapter));
    const stateA = diffViewState("a");
    const stateB = diffViewState("b");

    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-a" }); });
    act(() => result.current.reader?.setFileViewState({ reviewId: "review-a", snapshotId: "snapshot-a", path: "a-first.ts" }, stateA));
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-b" }); });
    act(() => {
      result.current.reader?.setView("file-changes");
      result.current.reader?.setScrollPosition("fileTree", 72);
      result.current.reader?.setFileViewState({ reviewId: "review-b", snapshotId: "snapshot-b", path: "a-first.ts" }, stateB);
    });
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-a" }); });
    await waitFor(() => expect(result.current.reader?.update.kind).toBe("available"));

    await act(async () => { expect((await result.current.reader!.refresh()).kind).toBe("refreshed"); });
    expect(result.current.reader?.snapshot.snapshotId).toBe("snapshot-a-2");
    expect(result.current.reader?.fileViewState).toBeNull();

    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-b" }); });
    expect(result.current.reader?.fileViewState).toBe(stateB);
    expect(result.current.reader?.view).toBe("file-changes");
    expect(result.current.reader?.scrollPositions.fileTree).toBe(72);
  });
});
