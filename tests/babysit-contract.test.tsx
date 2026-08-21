import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import type { BabysitFileExplanations } from "@/babysit/types";
import { useReviewLifecycle, type ReviewApplicationAdapter } from "@/review/use-review-lifecycle";
import type { FileChangeContent, ReviewSnapshot } from "@/review/types";

const snapshot: ReviewSnapshot = {
  reviewId: "review-1",
  snapshotId: "snapshot-1",
  baseRef: "refs/heads/main",
  baseRevision: "base-1",
  comparisonBase: "merge-base-1",
  headRef: "refs/heads/feature",
  headRevision: "head-1",
  fileChanges: [{ path: "src/example.ts", oldPath: null, status: "modified" }],
};

const content: FileChangeContent = {
  path: "src/example.ts",
  oldPath: null,
  status: "modified",
  contentKind: "text",
  beforeSize: 16,
  afterSize: 16,
  before: "const value = 1;",
  after: "const value = 2;",
};

/** 构造最小 Review lifecycle adapter，并保留一次 Babysit 请求的控制权。 */
function createAdapter() {
  let resolveExplanation!: (value: BabysitFileExplanations) => void;
  let requested: { request: Parameters<NonNullable<ReviewApplicationAdapter["explainFileChange"]>>[0]; signal: AbortSignal } | undefined;
  const adapter: ReviewApplicationAdapter = {
    async switchReview() { return snapshot; },
    async checkReviewUpdate() { return null; },
    async checkBranchSelectionUpdate() {
      return { changed: false, currentBaseRevision: "base-1", currentHeadRevision: "head-1", latestBaseRevision: "base-1", latestHeadRevision: "head-1" };
    },
    async refreshReview() { return { kind: "unchanged" }; },
    async readFileChange() { return content; },
    async closeReview() { return { kind: "closed" }; },
    explainFileChange(request, onProgress, signal) {
      requested = { request, signal };
      onProgress({ phase: "generating" });
      return new Promise((resolve) => { resolveExplanation = resolve; });
    },
  };
  return {
    adapter,
    resolve(value: BabysitFileExplanations) { resolveExplanation(value); },
    requested: () => requested,
  };
}

afterEach(() => cleanup());

describe("Babysit contract spine", () => {
  it("selects a File Change, emits generating, and renders a complete validated result binding", async () => {
    const provider = createAdapter();
    const { result } = renderHook(() => useReviewLifecycle(provider.adapter));

    await act(async () => {
      await result.current.open({ kind: "saved-review", reviewId: "review-1" });
    });
    await waitFor(() => expect(result.current.reader?.fileChange.kind).toBe("ready"));

    act(() => result.current.reader?.babysit?.explain());
    expect(result.current.reader?.babysit?.state.kind).toBe("generating");
    expect(provider.requested()?.request).toMatchObject({ reviewId: "review-1", snapshotId: "snapshot-1", path: "src/example.ts" });

    await act(async () => {
      provider.resolve({ before: { "1": "定义旧值。" }, after: { "1": "定义新值。" } });
    });
    await waitFor(() => expect(result.current.reader?.babysit?.state).toMatchObject({
      kind: "completed",
      explanations: { before: { "1": "定义旧值。" }, after: { "1": "定义新值。" } },
    }));
  });

  it("取消或切换 File Change 后丢弃迟到的 provider result", async () => {
    const provider = createAdapter();
    const { result } = renderHook(() => useReviewLifecycle(provider.adapter));
    await act(async () => {
      await result.current.open({ kind: "saved-review", reviewId: "review-1" });
    });
    await waitFor(() => expect(result.current.reader?.fileChange.kind).toBe("ready"));

    act(() => result.current.reader?.babysit?.explain());
    const oldSignal = provider.requested()!.signal;
    act(() => result.current.reader?.babysit?.cancel());
    expect(oldSignal.aborted).toBe(true);
    expect(result.current.reader?.babysit?.state).toEqual({ kind: "idle" });

    await act(async () => {
      provider.resolve({ before: { "1": "迟到" }, after: { "1": "迟到" } });
    });
    expect(result.current.reader?.babysit?.state).toEqual({ kind: "idle" });
  });
});
