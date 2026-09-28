import { act, cleanup, renderHook } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { useLanguageSupportPacks } from "@/language-support/use-language-support-packs";
import type { LanguageSupportApplication, PackCollection } from "@/language-support/types";
import { structureReviewFixture } from "@/dev/structure-review-fixture";

afterEach(() => { cleanup(); vi.useRealTimers(); });

it("keeps polling before a pending analysis publishes its first download and stops after its final snapshot", async () => {
  vi.useFakeTimers();
  let sequence = 0;
  const application: LanguageSupportApplication = { snapshot: vi.fn(async (): Promise<PackCollection> => ({ sequence: ++sequence, packs: [] })), retry: vi.fn(), remove: vi.fn() };
  let complete!: () => void;
  const analysis = { analyze: vi.fn(() => new Promise<ReturnType<typeof structureReviewFixture>>(resolve => { complete = () => resolve(structureReviewFixture()); })) };
  const { result } = renderHook(() => useLanguageSupportPacks(application, analysis));
  await act(async () => {});
  const initial = vi.mocked(application.snapshot).mock.calls.length;
  await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
  expect(application.snapshot).toHaveBeenCalledTimes(initial);
  let pending!: Promise<unknown>;
  await act(async () => { pending = result.current.structureAdapter.analyze({ reviewId: "r", snapshotId: "s", includeTests: false }, new AbortController().signal); });
  await act(async () => { await vi.advanceTimersByTimeAsync(2_000); });
  expect(vi.mocked(application.snapshot).mock.calls.length).toBeGreaterThan(initial + 1);
  await act(async () => { complete(); await pending; });
  const terminal = vi.mocked(application.snapshot).mock.calls.length;
  await act(async () => { await vi.advanceTimersByTimeAsync(3_000); });
  expect(application.snapshot).toHaveBeenCalledTimes(terminal);
});

it("does not overlap snapshot requests and recovers from query errors", async () => {
  vi.useFakeTimers();
  let finish!: (value: PackCollection) => void;
  const application: LanguageSupportApplication = { snapshot: vi.fn().mockRejectedValueOnce(new Error("IPC unavailable")).mockImplementationOnce(() => new Promise<PackCollection>(resolve => { finish = resolve; })).mockResolvedValue({ sequence: 2, packs: [] }), retry: vi.fn(), remove: vi.fn() };
  const analysis = { analyze: vi.fn() };
  const { result } = renderHook(() => useLanguageSupportPacks(application, analysis));
  await act(async () => {});
  expect(result.current.queryError).toBe(true);
  await act(async () => { await vi.advanceTimersByTimeAsync(1_000); });
  await act(async () => { result.current.reload(); await vi.advanceTimersByTimeAsync(5_000); });
  expect(application.snapshot).toHaveBeenCalledTimes(2);
  await act(async () => { finish({ sequence: 1, packs: [] }); });
  expect(result.current.queryError).toBe(false);
  expect(application.snapshot).toHaveBeenCalledTimes(3);
});

it("keeps newer management results when an older query finishes late", async () => {
  let finish!: (value: PackCollection) => void;
  const application: LanguageSupportApplication = { snapshot: vi.fn().mockImplementationOnce(() => new Promise<PackCollection>(resolve => { finish = resolve; })).mockResolvedValue({ sequence: 1, packs: [] }), retry: vi.fn().mockResolvedValue({ sequence: 3, packs: [] }), remove: vi.fn() };
  const { result } = renderHook(() => useLanguageSupportPacks(application, { analyze: vi.fn() }));
  await act(async () => { await result.current.retry("rust"); });
  await act(async () => { finish({ sequence: 2, packs: [] }); });
  expect(result.current.snapshot?.sequence).toBe(3);
});
