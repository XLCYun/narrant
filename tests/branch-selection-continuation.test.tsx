import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import type { ReviewApplicationAdapter } from "@/review/review-application";
import {
  useBranchSelectionContinuation,
  type BranchSelectionOpenResult,
} from "@/review/use-branch-selection-continuation";

const selection = {
  repositoryEntryId: 41,
  baseRef: "refs/remotes/origin/main",
  headRef: "refs/heads/topic",
};

const existingReview = {
  reviewId: "review-41",
  selection: { kind: "branch" as const, baseRef: selection.baseRef, headRef: selection.headRef },
  currentSnapshot: { baseRevision: "base-1", comparisonBase: "merge-base-1", headRevision: "head-1" },
  lastOpenedAt: 1,
};

const currentUpdate = {
  changed: false,
  currentBaseRevision: "base-1",
  currentHeadRevision: "head-1",
  latestBaseRevision: "base-1",
  latestHeadRevision: "head-1",
};

const availableUpdate = {
  ...currentUpdate,
  changed: true,
  latestBaseRevision: "base-2",
  latestHeadRevision: "head-2",
};

/** 创建只包含 continuation 所需能力的内存 Review operation。 */
function reviewOperations(
  check: ReviewApplicationAdapter["checkBranchSelectionUpdate"],
): Pick<ReviewApplicationAdapter, "checkBranchSelectionUpdate"> {
  return { checkBranchSelectionUpdate: check };
}

/** 创建由测试控制完成时机的异步结果。 */
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((complete, fail) => {
    resolve = complete;
    reject = fail;
  });
  return { promise, resolve, reject };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("Branch Selection continuation", () => {
  it("没有已有 Review 时提交新的 Branch Selection", async () => {
    const opened: unknown[] = [];
    const review = reviewOperations(async () => { throw new Error("不应检查更新"); });
    const { result } = renderHook(() => useBranchSelectionContinuation(
      review,
      {
        ...selection,
        reviews: [],
        openReview: async (target) => {
          opened.push(target);
          return { kind: "navigated" } satisfies BranchSelectionOpenResult;
        },
      },
    ));

    await waitFor(() => expect(result.current.state).toEqual({ kind: "ready", action: "start" }));
    await act(async () => {
      await result.current.submit();
    });

    expect(opened).toEqual([{
      kind: "branch-selection",
      ...selection,
      mode: { kind: "start" },
    }]);
  });

  it("不会把相同 Base/Head 的 Pull Request Review 当作 Branch Selection", async () => {
    const opened: unknown[] = [];
    const review = reviewOperations(async () => { throw new Error("不应检查更新"); });
    const pullRequestReview = {
      ...existingReview,
      selection: { kind: "pull-request" as const, baseRef: selection.baseRef, headRef: selection.headRef, pullRequest: {
        owner: "octo-org",
        repository: "octo-repo",
        number: 42,
        title: "Same labels",
        description: "",
        author: "octocat",
        state: "open" as const,
        draft: false,
        url: "https://github.com/octo-org/octo-repo/pull/42",
        baseRef: "main",
        headRef: "topic",
        synchronizedAt: 1,
      } },
    };
    const { result } = renderHook(() => useBranchSelectionContinuation(review, {
      ...selection,
      reviews: [pullRequestReview],
      openReview: async (target) => {
        opened.push(target);
        return { kind: "navigated" };
      },
    }));
    await waitFor(() => expect(result.current.state).toEqual({ kind: "ready", action: "start" }));

    await act(async () => { await result.current.submit(); });
    expect(opened).toEqual([expect.objectContaining({ mode: { kind: "start" } })]);
  });

  it("已有 Review 的 refs 未移动时继续该 Review", async () => {
    const opened: unknown[] = [];
    const review = reviewOperations(async () => currentUpdate);
    const { result } = renderHook(() => useBranchSelectionContinuation(
      review,
      {
        ...selection,
        reviews: [existingReview],
        openReview: async (target) => {
          opened.push(target);
          return { kind: "navigated" };
        },
      },
    ));

    await waitFor(() => expect(result.current.state).toEqual({ kind: "ready", action: "continue" }));
    await act(async () => {
      await result.current.submit();
    });

    expect(opened).toEqual([{
      kind: "branch-selection",
      ...selection,
      mode: { kind: "continue", reviewId: "review-41" },
    }]);
  });

  it("已有 Review 的 refs 移动时使用同一 confirmation 刷新", async () => {
    const opened: unknown[] = [];
    const review = reviewOperations(async () => availableUpdate);
    const { result } = renderHook(() => useBranchSelectionContinuation(review, {
      ...selection,
      reviews: [existingReview],
      openReview: async (target) => {
        opened.push(target);
        return { kind: "navigated" };
      },
    }));
    await waitFor(() => expect(result.current.state).toEqual({ kind: "ready", action: "refresh" }));

    await act(async () => {
      await result.current.submit();
    });

    expect(opened).toEqual([{
      kind: "branch-selection",
      ...selection,
      mode: { kind: "refresh", reviewId: "review-41", confirmation: availableUpdate },
    }]);
  });

  it("更新检查失败时降级为继续已有 Review", async () => {
    const opened: unknown[] = [];
    const review = reviewOperations(async () => { throw new Error("remote unavailable"); });
    const { result } = renderHook(() => useBranchSelectionContinuation(review, {
      ...selection,
      reviews: [existingReview],
      openReview: async (target) => {
        opened.push(target);
        return { kind: "navigated" };
      },
    }));
    await waitFor(() => expect(result.current.state).toEqual({ kind: "ready", action: "fallback" }));

    await act(async () => {
      await result.current.submit();
    });

    expect(opened).toEqual([{
      kind: "branch-selection",
      ...selection,
      mode: { kind: "continue", reviewId: "review-41" },
    }]);
  });

  it("快速切换 Selection 时迟到的旧检查不能覆盖最新决策", async () => {
    const oldCheck = deferred<typeof availableUpdate>();
    const latestCheck = deferred<typeof currentUpdate>();
    const opened: unknown[] = [];
    const review = reviewOperations((reviewId) => (
      reviewId === "review-41" ? oldCheck.promise : latestCheck.promise
    ));
    const latestReview = {
      ...existingReview,
      reviewId: "review-42",
      headRef: "refs/heads/next-topic",
    };
    const { result, rerender } = renderHook(
      ({ headRef, reviews }) => useBranchSelectionContinuation(review, {
        ...selection,
        headRef,
        reviews,
        openReview: async (target) => {
          opened.push(target);
          return { kind: "navigated" };
        },
      }),
      { initialProps: { headRef: selection.headRef, reviews: [existingReview] } },
    );
    await waitFor(() => expect(result.current.state.kind).toBe("checking"));

    rerender({ headRef: latestReview.selection.headRef, reviews: [latestReview] });
    act(() => latestCheck.resolve(currentUpdate));
    await waitFor(() => expect(result.current.state).toEqual({ kind: "ready", action: "continue" }));

    act(() => oldCheck.resolve(availableUpdate));
    await act(async () => { await oldCheck.promise; });
    expect(result.current.state).toEqual({ kind: "ready", action: "continue" });
    await act(async () => {
      await result.current.submit();
    });
    expect(opened).toEqual([{
      kind: "branch-selection",
      repositoryEntryId: 41,
      baseRef: selection.baseRef,
      headRef: latestReview.selection.headRef,
      mode: { kind: "continue", reviewId: "review-42" },
    }]);
  });

  it("检查超过一秒后才显示额外状态文案", () => {
    vi.useFakeTimers();
    const pending = deferred<typeof currentUpdate>();
    const review = reviewOperations(() => pending.promise);
    const { result } = renderHook(() => useBranchSelectionContinuation(review, {
      ...selection,
      reviews: [existingReview],
      openReview: async () => ({ kind: "navigated" }),
    }));

    expect(result.current.state).toEqual({ kind: "checking", showMessage: false });
    act(() => vi.advanceTimersByTime(999));
    expect(result.current.state).toEqual({ kind: "checking", showMessage: false });
    act(() => vi.advanceTimersByTime(1));
    expect(result.current.state).toEqual({ kind: "checking", showMessage: true });
  });

  it("refresh 导航失败后重新检查而不复用旧 confirmation", async () => {
    let checks = 0;
    const review = reviewOperations(async () => checks++ === 0 ? availableUpdate : currentUpdate);
    const { result } = renderHook(() => useBranchSelectionContinuation(review, {
      ...selection,
      reviews: [existingReview],
      openReview: async () => ({
        kind: "failed",
        detail: "current Snapshot 已改变；请重新检查更新。",
        previousDestinationPreserved: true,
      }),
    }));
    await waitFor(() => expect(result.current.state).toEqual({ kind: "ready", action: "refresh" }));

    await act(async () => {
      await result.current.submit();
    });

    await waitFor(() => expect(result.current.state).toEqual({ kind: "ready", action: "continue" }));
    expect(checks).toBe(2);
  });

  it("离开 Repository Entry 时中止检查并清除旧决策", async () => {
    const pending = deferred<typeof availableUpdate>();
    let signal!: AbortSignal;
    const review = reviewOperations((_reviewId, operationSignal) => {
      signal = operationSignal;
      return pending.promise;
    });
    const opened: unknown[] = [];
    const { result, rerender } = renderHook(
      ({ repositoryEntryId }) => useBranchSelectionContinuation(review, {
        ...selection,
        repositoryEntryId,
        reviews: [existingReview],
        openReview: async (target) => {
          opened.push(target);
          return { kind: "navigated" };
        },
      }),
      { initialProps: { repositoryEntryId: 41 as number | undefined } },
    );
    expect(result.current.state.kind).toBe("checking");

    rerender({ repositoryEntryId: undefined });
    expect(result.current.state).toEqual({ kind: "incomplete" });
    expect(signal.aborted).toBe(true);
    act(() => pending.resolve(availableUpdate));
    await act(async () => { await pending.promise; });
    await act(async () => { await result.current.submit(); });

    expect(result.current.state).toEqual({ kind: "incomplete" });
    expect(opened).toEqual([]);
  });
});
