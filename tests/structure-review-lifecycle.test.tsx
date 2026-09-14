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
  expect(adapter.analyze).toHaveBeenCalledWith({ reviewId: snapshot.reviewId, snapshotId: snapshot.snapshotId, includeTests: false }, expect.any(AbortSignal));
  rerender({ enabled: false });
  rerender({ enabled: true });
  expect(adapter.analyze).toHaveBeenCalledTimes(1);
});

it("restarts analysis for option changes and isolates late results from the previous scope", async () => {
  const first = deferred(), second = deferred(), third = deferred();
  const adapter: StructureReviewAdapter = { analyze: vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise).mockReturnValueOnce(third.promise) };
  const { result } = renderHook(() => useStructureReview(adapter, snapshot, true));
  expect(result.current.binding?.options).toEqual({ includeTests: false });
  act(() => result.current.binding?.setOptions({ includeTests: true }));
  expect(result.current.binding?.state.kind).toBe("loading");
  expect(vi.mocked(adapter.analyze).mock.calls[0][1].aborted).toBe(true);
  expect(vi.mocked(adapter.analyze).mock.calls[1][0].includeTests).toBe(true);
  await act(async () => { first.resolve(comparison); });
  expect(result.current.binding?.state.kind).toBe("loading");
  await act(async () => { second.resolve(comparison); });
  expect(result.current.binding?.state.kind).toBe("ready");
  act(() => result.current.binding?.setOptions({ includeTests: true }));
  expect(adapter.analyze).toHaveBeenCalledTimes(2);
  act(() => result.current.binding?.setOptions({ includeTests: false }));
  expect(result.current.binding?.state.kind).toBe("loading");
  expect(vi.mocked(adapter.analyze).mock.calls[2][0].includeTests).toBe(false);
  await act(async () => { third.resolve(comparison); });
  expect(result.current.binding?.state.kind).toBe("ready");
});

it("keeps the requested scope on retry and does not analyze a hidden view on option change", async () => {
  const pending = deferred();
  const adapter: StructureReviewAdapter = { analyze: vi.fn().mockReturnValueOnce(pending.promise).mockResolvedValue(comparison) };
  const { result, rerender } = renderHook(({ enabled }) => useStructureReview(adapter, snapshot, enabled), { initialProps: { enabled: false } });
  act(() => result.current.binding?.setOptions({ includeTests: true }));
  expect(adapter.analyze).not.toHaveBeenCalled();
  rerender({ enabled: true });
  expect(vi.mocked(adapter.analyze).mock.calls[0][0].includeTests).toBe(true);
  act(() => result.current.binding?.cancel());
  act(() => result.current.binding?.retry());
  await waitFor(() => expect(result.current.binding?.state.kind).toBe("ready"));
  expect(vi.mocked(adapter.analyze).mock.calls[1][0].includeTests).toBe(true);
});

it("ignores an old completion after Snapshot replacement", async () => {
  const first = deferred(), included = deferred(), second = deferred();
  const adapter: StructureReviewAdapter = { analyze: vi.fn().mockReturnValueOnce(first.promise).mockReturnValueOnce(included.promise).mockReturnValueOnce(second.promise) };
  const { result, rerender } = renderHook(({ current }) => useStructureReview(adapter, current, true), { initialProps: { current: snapshot } });
  act(() => result.current.binding?.setOptions({ includeTests: true }));
  const next = { ...snapshot, snapshotId: "next", headRevision: "next-head" };
  rerender({ current: next });
  expect(result.current.binding?.options.includeTests).toBe(false);
  expect(vi.mocked(adapter.analyze).mock.calls[2][0].includeTests).toBe(false);
  await act(async () => { second.resolve({ ...comparison, snapshotId: next.snapshotId, headRevision: next.headRevision }); });
  await act(async () => { first.resolve(comparison); included.resolve(comparison); });
  expect(result.current.binding?.state).toMatchObject({ kind: "ready", comparison: { snapshotId: "next" } });
  expect(vi.mocked(adapter.analyze).mock.calls[0][1].aborted).toBe(true);
  expect(vi.mocked(adapter.analyze).mock.calls[1][1].aborted).toBe(true);
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
