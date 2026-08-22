import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { isBabysitError, type BabysitApplicationAdapter, type BabysitBinding, type BabysitError, type BabysitFileExplanations } from "@/babysit/types";
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
    explainFile({ file: request }, onProgress, signal) {
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

  it("regenerates a completed File Change with the regenerate cache policy", async () => {
    const provider = createAdapter();
    const explainFile = vi.spyOn(provider.babysitAdapter, "explainFile");
    const { result } = renderHook(() => useReviewLifecycle(provider.reviewAdapter, provider.babysitAdapter));
    await act(async () => {
      await result.current.open({ kind: "saved-review", reviewId: "review-1" });
    });
    await waitFor(() => expect(result.current.reader?.fileChange.kind).toBe("ready"));
    act(() => result.current.reader?.babysit?.explain());
    await act(async () => {
      provider.resolve({ before: { "1": "旧解释" }, after: { "1": "旧解释" } });
    });
    await waitFor(() => expect(result.current.reader?.babysit?.state.kind).toBe("completed"));

    act(() => (result.current.reader?.babysit as BabysitBinding & { regenerate(): void }).regenerate());

    expect(result.current.reader?.babysit?.state.kind).toBe("generating");
    expect(explainFile).toHaveBeenCalledTimes(2);
    expect(explainFile.mock.calls[1]?.[0]).toEqual(expect.objectContaining({
      generationPolicy: "regenerate",
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

  it("生成中切换文件只取消旧请求，并允许新文件再次分析", async () => {
    const twoFileSnapshot: ReviewSnapshot = {
      ...snapshot,
      fileChanges: [
        { path: "src/example.ts", oldPath: null, status: "modified" },
        { path: "src/next.ts", oldPath: null, status: "modified" },
      ],
    };
    const provider = createAdapter(twoFileSnapshot);
    const { result } = renderHook(() => useReviewLifecycle(provider.reviewAdapter, provider.babysitAdapter));

    await act(async () => {
      await result.current.open({ kind: "saved-review", reviewId: "review-1" });
    });
    await waitFor(() => expect(result.current.reader?.fileChange).toMatchObject({ kind: "ready", path: "src/example.ts" }));
    act(() => result.current.reader?.babysit?.explain());
    const oldSignal = provider.requested()!.signal;

    act(() => result.current.reader?.selectFileChange("src/next.ts"));
    expect(oldSignal.aborted).toBe(true);
    await waitFor(() => expect(result.current.reader?.fileChange).toMatchObject({ kind: "ready", path: "src/next.ts" }));
    expect(provider.requested()?.request.path).toBe("src/example.ts");

    act(() => result.current.reader?.babysit?.explain());
    expect(provider.requested()?.request.path).toBe("src/next.ts");
  });

  it("snapshot refresh 后清除旧缓存 seam，并让新 Snapshot 使用新请求 identity", async () => {
    const refreshedSnapshot = { ...snapshot, snapshotId: "snapshot-2", headRevision: "head-2" };
    const provider = createAdapter(snapshot);
    provider.reviewAdapter.checkReviewUpdate = async () => ({
      currentBaseRevision: "base-1",
      currentHeadRevision: "head-1",
      availableBaseRevision: "base-1",
      availableHeadRevision: "head-2",
    });
    provider.reviewAdapter.refreshReview = async () => ({ kind: "refreshed", snapshot: refreshedSnapshot });
    const clearSnapshot = vi.fn(async () => undefined);
    const babysitAdapter: BabysitApplicationAdapter = {
      ...provider.babysitAdapter,
      clearSnapshot,
    };
    const { result } = renderHook(() => useReviewLifecycle(provider.reviewAdapter, babysitAdapter));

    await act(async () => {
      await result.current.open({ kind: "saved-review", reviewId: "review-1" });
    });
    await waitFor(() => expect(result.current.reader?.fileChange.kind).toBe("ready"));
    await act(async () => {
      await result.current.reader?.refresh();
    });

    expect(clearSnapshot).toHaveBeenCalledWith({ reviewId: "review-1", snapshotId: "snapshot-1" });
    expect(result.current.reader?.snapshot.snapshotId).toBe("snapshot-2");
  });

  it("关闭 Review 后清除 Review 缓存 seam", async () => {
    const provider = createAdapter();
    const clearReview = vi.fn(async () => undefined);
    const babysitAdapter: BabysitApplicationAdapter = { ...provider.babysitAdapter, clearReview };
    const { result } = renderHook(() => useReviewLifecycle(provider.reviewAdapter, babysitAdapter));

    await act(async () => {
      await result.current.open({ kind: "saved-review", reviewId: "review-1" });
    });
    await waitFor(() => expect(result.current.reader?.fileChange.kind).toBe("ready"));
    await act(async () => {
      await result.current.close();
    });

    expect(clearReview).toHaveBeenCalledWith("review-1");
    expect(result.current.reader).toBeNull();
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

  it("eligibility IPC 失败时保留可打开、可重试的失败状态", async () => {
    const provider = createAdapter();
    const babysitAdapter: BabysitApplicationAdapter = {
      async checkEligibility() {
        throw { code: "input_unavailable", detail: "eligibility command failed" } satisfies BabysitError;
      },
      async explainFile() {
        throw new Error("not reached before retry");
      },
    };
    const { result } = renderHook(() => useReviewLifecycle(provider.reviewAdapter, babysitAdapter));

    await act(async () => {
      await result.current.open({ kind: "saved-review", reviewId: "review-1" });
    });
    await waitFor(() => expect(result.current.reader?.babysit).toMatchObject({
      eligibility: { kind: "resolved", result: { eligible: true } },
      state: {
        kind: "failed",
        error: { code: "input_unavailable" },
        retryable: true,
      },
    }));
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
    const error: BabysitError = {
      code: "repair_exhausted",
      detail: "/Users/alice/repo/src/example.ts source=const secret='token'; stderr=raw provider output",
    };
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
