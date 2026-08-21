import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";

import { isBabysitError, type BabysitApplicationAdapter, type BabysitError, type BabysitFileExplanations } from "@/babysit/types";
import { i18n } from "@/lib/i18n";
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
function createAdapter(reviewSnapshot = snapshot, fileContent = content) {
  let resolveExplanation!: (value: BabysitFileExplanations) => void;
  let requested: { request: Parameters<BabysitApplicationAdapter["explainFile"]>[0]; signal: AbortSignal } | undefined;
  let eligibilityLanguage: string | undefined;
  const reviewAdapter: ReviewApplicationAdapter = {
    async switchReview() { return reviewSnapshot; },
    async checkReviewUpdate() { return null; },
    async checkBranchSelectionUpdate() {
      return { changed: false, currentBaseRevision: "base-1", currentHeadRevision: "head-1", latestBaseRevision: "base-1", latestHeadRevision: "head-1" };
    },
    async refreshReview() { return { kind: "unchanged" }; },
    async readFileChange() { return fileContent; },
    async closeReview() { return { kind: "closed" }; },
  };
  const babysitAdapter: BabysitApplicationAdapter = {
    async checkEligibility(_request, detectedLanguage) {
      eligibilityLanguage = detectedLanguage;
      return detectedLanguage
        ? { eligible: true, reason: null }
        : { eligible: false, reason: "unsupported_language" };
    },
    explainFile(request, onProgress, signal) {
      requested = { request, signal };
      onProgress({ phase: "generating" });
      return new Promise((resolve) => { resolveExplanation = resolve; });
    },
  };
  return {
    reviewAdapter,
    babysitAdapter,
    resolve(value: BabysitFileExplanations) { resolveExplanation(value); },
    requested: () => requested,
    eligibilityLanguage: () => eligibilityLanguage,
  };
}

afterEach(async () => {
  cleanup();
  await i18n.changeLanguage("zh-CN");
});

describe("Babysit contract spine", () => {
  it("rejects unknown provider-specific error codes at the runtime boundary", () => {
    expect(isBabysitError({ code: "provider_timeout", detail: "timeout" })).toBe(false);
    expect(isBabysitError({ code: "provider", detail: "provider failed" })).toBe(true);
    expect(isBabysitError({ code: "codex_not_found", detail: "missing", recovery: "open_shell" })).toBe(false);
  });

  it("selects a File Change, emits generating, and renders a complete validated result binding", async () => {
    const provider = createAdapter();
    const { result } = renderHook(() => useReviewLifecycle(provider.reviewAdapter, provider.babysitAdapter));

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
    const { result } = renderHook(() => useReviewLifecycle(provider.reviewAdapter, provider.babysitAdapter));
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

  it("uses the current UI locale in the typed analysis request", async () => {
    await i18n.changeLanguage("en");
    const provider = createAdapter();
    const { result } = renderHook(() => useReviewLifecycle(provider.reviewAdapter, provider.babysitAdapter));

    await act(async () => {
      await result.current.open({ kind: "saved-review", reviewId: "review-1" });
    });
    await waitFor(() => expect(result.current.reader?.fileChange.kind).toBe("ready"));
    act(() => result.current.reader?.babysit?.explain());

    expect(provider.requested()?.request.locale).toBe("en");
  });

  it("extensionless Python shebang 对 eligibility 与 explain 使用同一 detectedLanguage", async () => {
    const scriptSnapshot = { ...snapshot, fileChanges: [{ path: "bin/tool", oldPath: null, status: "added" as const }] };
    const scriptContent: FileChangeContent = {
      path: "bin/tool", oldPath: null, status: "added", contentKind: "text",
      beforeSize: null, afterSize: 38, before: null, after: "#!/usr/bin/env python3\nprint('hello')\n",
    };
    const provider = createAdapter(scriptSnapshot, scriptContent);
    const { result } = renderHook(() => useReviewLifecycle(provider.reviewAdapter, provider.babysitAdapter));
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-1" }); });
    await waitFor(() => expect(result.current.reader?.babysit?.eligibility).toMatchObject({ kind: "resolved", result: { eligible: true } }));
    expect(provider.eligibilityLanguage()).toBe("python");
    act(() => result.current.reader?.babysit?.explain());
    expect(provider.requested()?.request.detectedLanguage).toBe("python");
  });

  it("未知 extensionless 内容保持 disabled 且不发起 explain", async () => {
    const unknownSnapshot = { ...snapshot, fileChanges: [{ path: "bin/tool", oldPath: null, status: "added" as const }] };
    const unknownContent: FileChangeContent = {
      path: "bin/tool", oldPath: null, status: "added", contentKind: "text",
      beforeSize: null, afterSize: 10, before: null, after: "echo maybe\n",
    };
    const provider = createAdapter(unknownSnapshot, unknownContent);
    const { result } = renderHook(() => useReviewLifecycle(provider.reviewAdapter, provider.babysitAdapter));
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "review-1" }); });
    await waitFor(() => expect(result.current.reader?.babysit?.eligibility).toMatchObject({ kind: "resolved", result: { eligible: false } }));
    act(() => result.current.reader?.babysit?.explain());
    expect(provider.requested()).toBeUndefined();
  });

  it("keeps structured provider errors in the lifecycle failure state", async () => {
    const error: BabysitError = { code: "repair_exhausted", detail: "three repairs" };
    const provider = createAdapter();
    const babysitAdapter: BabysitApplicationAdapter = {
      async checkEligibility() { return { eligible: true, reason: null }; },
      async explainFile() {
        throw error;
      },
    };
    const { result } = renderHook(() => useReviewLifecycle(provider.reviewAdapter, babysitAdapter));

    await act(async () => {
      await result.current.open({ kind: "saved-review", reviewId: "review-1" });
    });
    await waitFor(() => expect(result.current.reader?.fileChange.kind).toBe("ready"));
    act(() => result.current.reader?.babysit?.explain());

    await waitFor(() => expect(result.current.reader?.babysit?.state).toMatchObject({
      kind: "failed",
      error,
    }));
  });
});
