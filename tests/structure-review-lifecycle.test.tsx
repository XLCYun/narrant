import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { structureReviewFixture } from "@/dev/structure-review-fixture";
import { useStructureReview } from "@/structure-review/use-structure-review";
import type { StructureComparison, StructureReviewAdapter } from "@/structure-review/types";
import type { ReviewSnapshot } from "@/review/types";

afterEach(cleanup);
const comparison = structureReviewFixture();
const snapshot: ReviewSnapshot = { ...comparison, baseRef: "main", baseRevision: "base", headRef: "topic", fileChanges: [] };

/** 暴露可控完成点，验证取消之后仍然成功返回的 Adapter。 */
function deferred() {
  let resolve!: (value: StructureComparison) => void;
  const promise = new Promise<StructureComparison>((done) => { resolve = done; });
  return { promise, resolve };
}

it("analyzes lazily and reuses the same Snapshot across tabs", async () => {
  const adapter: StructureReviewAdapter = { analyze: vi.fn().mockResolvedValue(comparison) };
  const { result, rerender } = renderHook(({ enabled }) => useStructureReview(adapter, snapshot, enabled), { initialProps: { enabled: false } });
  expect(adapter.analyze).not.toHaveBeenCalled();
  rerender({ enabled: true });
  await waitFor(() => expect(result.current.binding?.state.kind).toBe("ready"));
  rerender({ enabled: false });
  rerender({ enabled: true });
  expect(adapter.analyze).toHaveBeenCalledTimes(1);
});

it("ignores an old completion after Snapshot replacement", async () => {
  const first = deferred(), second = deferred();
  const adapter: StructureReviewAdapter = { analyze: vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise) };
  const { result, rerender } = renderHook(({ current }) => useStructureReview(adapter, current, true), { initialProps: { current: snapshot } });
  const next = { ...snapshot, snapshotId: "next", headRevision: "next-head" };
  rerender({ current: next });
  await act(async () => { second.resolve({ ...comparison, snapshotId: next.snapshotId, headRevision: next.headRevision }); });
  await act(async () => { first.resolve(comparison); });
  expect(result.current.binding?.state).toMatchObject({ kind: "ready", comparison: { snapshotId: "next" } });
  expect(vi.mocked(adapter.analyze).mock.calls[0][1].aborted).toBe(true);
});

it("keeps cancellation stable until explicit retry and releases caches on close", async () => {
  const first = deferred();
  const adapter: StructureReviewAdapter = { analyze: vi.fn().mockReturnValueOnce(first.promise).mockResolvedValue(comparison) };
  const { result } = renderHook(() => useStructureReview(adapter, snapshot, true));
  act(() => result.current.binding?.cancel());
  await act(async () => first.resolve(comparison));
  expect(result.current.binding?.state.kind).toBe("cancelled");
  act(() => result.current.binding?.retry());
  await waitFor(() => expect(result.current.binding?.state.kind).toBe("ready"));
  act(() => result.current.clearReview(snapshot.reviewId));
  expect(result.current.binding?.state.kind).toBe("idle");
  expect(adapter.analyze).toHaveBeenCalledTimes(2);
});

it("shows backend failures and rejects mismatched comparison identities", async () => {
  const adapter: StructureReviewAdapter = { analyze: vi.fn().mockRejectedValueOnce({ code: "analysis-failed", detail: "server unavailable" }).mockResolvedValue({ ...comparison, headRevision: "wrong-head" }) };
  const { result } = renderHook(() => useStructureReview(adapter, snapshot, true));
  await waitFor(() => expect(result.current.binding?.state).toEqual({ kind: "failed", detail: "server unavailable" }));
  act(() => result.current.binding?.retry());
  await waitFor(() => expect(result.current.binding?.state.kind).toBe("failed"));
});
