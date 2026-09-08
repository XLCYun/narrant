import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useReviewLifecycle } from "@/review/use-review-lifecycle";
import type { ReviewApplicationAdapter } from "@/review/review-application";
import type { ReviewSnapshot } from "@/review/types";
import type { StructureApplicationAdapter } from "@/review/structure-application";
import type { StructureEvent, StructurePeekContent, StructureResult } from "@/review/structure-types";

const snapshot: ReviewSnapshot = {
  reviewId: "r1", snapshotId: "s1", baseRef: "main", baseRevision: "b1", comparisonBase: "c1", headRef: "feature", headRevision: "h1",
  fileChanges: [{ path: "changed.ts", oldPath: null, status: "modified" }],
};

/** 使用完整公共应用边界提供可观察的 Review，不依赖生命周期内部对象。 */
function reviewAdapter(): ReviewApplicationAdapter {
  return {
    switchReview: async () => snapshot,
    checkReviewUpdate: async () => null,
    checkBranchSelectionUpdate: async () => ({ changed: false, currentBaseRevision: "b1", currentHeadRevision: "h1", latestBaseRevision: "b1", latestHeadRevision: "h1" }),
    refreshReview: async () => ({ kind: "unchanged" }),
    readFileChange: async (_identity, path) => ({ path, oldPath: null, status: "modified", contentKind: "text", beforeSize: 3, afterSize: 3, before: "old", after: "new" }),
    closeReview: async () => ({ kind: "closed" }),
  };
}

/** 已知空仓库结果，便于逐个操控后台事件和完成时机。 */
function emptyResult(reviewId = "r1", snapshotId = "s1"): StructureResult {
  return {
    reviewId, snapshotId, status: "completed", nodes: [], implementationGroups: [], relationships: [], unresolvedRelationships: [], sources: [],
    coverage: { discovery: { before: "complete", after: "complete" }, files: [], diagnostics: [] },
    summary: { changedDeclarations: 0, relationshipChanges: 0, incompleteDeclarations: 0, unresolvedOccurrences: 0, complete: true },
    initialFocus: { nodeIds: [], relationshipIds: [] },
  };
}

/** 可控分析请求允许模拟后端在取消后仍然送达的结果。 */
function structureAdapter() {
  const requests: Array<{ identity: { reviewId: string; snapshotId: string }; event: (event: StructureEvent) => void; signal: AbortSignal; resolve: (result: StructureResult) => void; reject: (reason: unknown) => void }> = [];
  const adapter: StructureApplicationAdapter = {
    analyze: vi.fn<StructureApplicationAdapter["analyze"]>((identity, event, signal) => new Promise((resolve, reject) => requests.push({ identity, event, signal, resolve, reject }))),
    readSource: vi.fn(async () => { throw new Error("no source requested"); }),
  };
  return { adapter, requests };
}

describe("Review 的代码结构后台生命周期", () => {
  it("仅首次进入第三视图开始分析，期间文件阅读正常，取消保留检查点并拒绝迟到进度和完成", async () => {
    const structure = structureAdapter();
    const adapter = reviewAdapter();
    const { result } = renderHook(() => useReviewLifecycle(adapter, {}, undefined, structure.adapter));
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "r1" }); });
    expect(structure.requests).toHaveLength(0);
    act(() => result.current.reader!.setView("code-structure"));
    expect(structure.requests).toHaveLength(1);
    expect(result.current.reader!.structure!.analysis.status).toBe("running");
    await waitFor(() => expect(result.current.reader!.fileChange.kind).toBe("ready"));
    const checkpoint = { ...emptyResult(), status: "running" as const };
    act(() => structure.requests[0].event({ kind: "checkpoint", result: checkpoint }));
    act(() => result.current.reader!.structure!.cancel());
    expect(structure.requests[0].signal.aborted).toBe(true);
    act(() => {
      structure.requests[0].event({ kind: "progress", progress: { phase: "relationships", side: null, processedFiles: 10, totalFiles: 10 } });
      structure.requests[0].resolve(emptyResult());
    });
    await act(async () => {});
    expect(result.current.reader!.structure!.analysis).toMatchObject({ status: "cancelled", result: checkpoint });
    expect(result.current.reader!.structure!.analysis.progress).toBeNull();
    act(() => result.current.reader!.setView("file-changes"));
    act(() => result.current.reader!.setView("code-structure"));
    expect(structure.requests).toHaveLength(1);
  });
  it("切换开放 Review 保留各自相机和分析，重试后的旧失败不能覆盖新请求", async () => {
    const structure = structureAdapter();
    const adapter = reviewAdapter();
    adapter.switchReview = async (target) => ({ ...snapshot, reviewId: target.kind === "saved-review" ? target.reviewId : "r1" });
    const { result } = renderHook(() => useReviewLifecycle(adapter, {}, undefined, structure.adapter));
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "r1" }); });
    act(() => result.current.reader!.setView("code-structure"));
    act(() => result.current.reader!.structure!.setPresentation({ ...result.current.reader!.structure!.presentation, viewport: { x: 44, y: 19, zoom: 1.4 } }));
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "r2" }); });
    expect(structure.requests[0].signal.aborted).toBe(false);
    act(() => result.current.reader!.setView("code-structure"));
    act(() => structure.requests[0].resolve(emptyResult()));
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "r1" }); });
    expect(result.current.reader!.structure!.analysis.status).toBe("completed");
    expect(result.current.reader!.structure!.presentation.viewport).toEqual({ x: 44, y: 19, zoom: 1.4 });
    act(() => result.current.reader!.structure!.retry());
    act(() => result.current.reader!.structure!.retry());
    expect(structure.requests[2].signal.aborted).toBe(true);
    act(() => structure.requests[2].reject(new Error("late failure")));
    await act(async () => {});
    expect(result.current.reader!.structure!.analysis.status).toBe("running");
    act(() => structure.requests[3].resolve(emptyResult()));
    await waitFor(() => expect(result.current.reader!.structure!.analysis.status).toBe("completed"));
    act(() => structure.requests[3].event({ kind: "progress", progress: { phase: "discovery", side: "before", processedFiles: 1, totalFiles: null } }));
    expect(result.current.reader!.structure!.analysis.progress).toBeNull();
  });

  it("刷新新 Snapshot 撤销旧操作与身份，保留展示偏好，分析失败不会回滚刷新", async () => {
    const structure = structureAdapter();
    const adapter = reviewAdapter();
    adapter.checkReviewUpdate = async () => ({ currentBaseRevision: "b1", currentHeadRevision: "h1", availableBaseRevision: "b1", availableHeadRevision: "h2" });
    adapter.refreshReview = async () => ({ kind: "refreshed", snapshot: { ...snapshot, snapshotId: "s2", headRevision: "h2" } });
    const { result } = renderHook(() => useReviewLifecycle(adapter, {}, undefined, structure.adapter));
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "r1" }); });
    act(() => result.current.reader!.setView("code-structure"));
    act(() => result.current.reader!.structure!.setPresentation({ ...result.current.reader!.structure!.presentation, selectedNodeId: "old-node", viewport: { x: 1, y: 2, zoom: 2 }, filters: { ...result.current.reader!.structure!.presentation.filters, imports: true } }));
    await act(async () => { expect(await result.current.reader!.refresh()).toEqual({ kind: "refreshed", reviewId: "r1", snapshotId: "s2" }); });
    expect(structure.requests[0].signal.aborted).toBe(true);
    expect(structure.requests[1].identity.snapshotId).toBe("s2");
    act(() => {
      structure.requests[0].event({ kind: "checkpoint", result: emptyResult() });
      structure.requests[0].reject(new Error("old failure"));
      structure.requests[1].event({ kind: "checkpoint", result: emptyResult() });
    });
    await act(async () => {});
    expect(result.current.reader!.structure!.analysis).toMatchObject({ status: "running", result: null });
    expect(result.current.reader!.structure!.presentation).toMatchObject({ selectedNodeId: null, viewport: null, filters: { imports: true } });
    act(() => structure.requests[1].reject(new Error("analysis failed")));
    await waitFor(() => expect(result.current.reader!.structure!.analysis.status).toBe("failed"));
    expect(result.current.reader!.snapshot.snapshotId).toBe("s2");
    expect(result.current.reader!.transition.kind).toBe("stable");
    expect(result.current.reader!.fileChange.kind).toBe("ready");
  });

  it("关闭后重新打开同一身份也不接受旧结果，卸载取消所有未呈现分析", async () => {
    const structure = structureAdapter();
    const adapter = reviewAdapter();
    const { result, unmount } = renderHook(() => useReviewLifecycle(adapter, {}, undefined, structure.adapter));
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "r1" }); });
    act(() => result.current.reader!.setView("code-structure"));
    const oldBinding = result.current.reader!.structure!;
    await act(async () => { await result.current.close(); });
    expect(structure.requests[0].signal.aborted).toBe(true);
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "r1" }); });
    act(() => { structure.requests[0].resolve(emptyResult()); oldBinding.retry(); });
    await act(async () => {});
    expect(result.current.reader!.structure!.analysis.status).toBe("idle");
    expect(structure.requests).toHaveLength(1);
    act(() => result.current.reader!.setView("code-structure"));
    act(() => result.current.unpresent());
    expect(structure.requests[1].signal.aborted).toBe(false);
    unmount();
    expect(structure.requests[1].signal.aborted).toBe(true);
  });

  it("Peek 读取真实未改上下文且独立于文件选择，忽略旧源码读取并保留各来源 editor state", async () => {
    const structure = structureAdapter();
    const reads: Array<{ path: string; signal: AbortSignal; resolve: (value: StructurePeekContent) => void }> = [];
    structure.adapter.readSource = vi.fn<StructureApplicationAdapter["readSource"]>((_identity, request, signal) => new Promise((resolve) => reads.push({ path: request.path, signal, resolve })));
    const analysis = emptyResult();
    analysis.sources = ["context.ts", "other.ts"].map((path) => ({ id: path, before: { state: "present", path, revision: "c1" }, after: { state: "present", path, revision: "h1" }, classification: { kind: "unchanged", path } }));
    analysis.nodes = analysis.sources.map((source) => ({ kind: "file", id: source.id, sourceId: source.id, containedChanges: { before: 0, after: 0, union: 0 } }));
    const adapter = reviewAdapter();
    const { result } = renderHook(() => useReviewLifecycle(adapter, {}, undefined, structure.adapter));
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "r1" }); });
    act(() => result.current.reader!.setView("code-structure"));
    act(() => structure.requests[0].resolve(analysis));
    await waitFor(() => expect(result.current.reader!.structure!.analysis.status).toBe("completed"));
    act(() => result.current.reader!.structure!.navigate({ kind: "node", nodeId: "context.ts" }));
    expect(reads[0].path).toBe("context.ts");
    expect(result.current.reader!.fileChange).toMatchObject({ kind: "ready", path: "changed.ts" });
    act(() => result.current.reader!.structure!.navigate({ kind: "node", nodeId: "other.ts" }));
    expect(reads[0].signal.aborted).toBe(true);
    /** 完整源码内容保持真实固定文件对，绝不借用 File Change 身份。 */
    const content = (index: number): StructurePeekContent => ({ reviewId: "r1", snapshotId: "s1", sourceId: analysis.sources[index].id, source: analysis.sources[index], before: { state: "available", path: analysis.sources[index].id, revision: "c1", objectId: "blob", text: "full old file", size: 13 }, after: { state: "available", path: analysis.sources[index].id, revision: "h1", objectId: "blob2", text: "full new file", size: 13 } });
    act(() => { reads[1].resolve(content(1)); reads[0].resolve(content(0)); });
    await waitFor(() => expect(result.current.reader!.structure!.peek?.load.status).toBe("ready"));
    expect(result.current.reader!.structure!.peek?.selection.title).toBe("other.ts");
    const viewState = { scroll: 75 };
    act(() => result.current.reader!.structure!.savePeekViewState({ reviewId: "r1", snapshotId: "s1", sourceId: "other.ts" }, viewState));
    act(() => result.current.reader!.structure!.closePeek());
    act(() => result.current.reader!.structure!.navigate({ kind: "node", nodeId: "other.ts" }));
    expect(result.current.reader!.structure!.peekViewState).toEqual(viewState);
    expect(result.current.reader!.fileChange).toMatchObject({ kind: "ready", path: "changed.ts" });
    expect(result.current.reader!.view).toBe("code-structure");
  });

  it("显式取消仅接受同一请求的 cancelled 部分终态，重复终态与重试后的旧取消结果不能覆盖", async () => {
    const structure = structureAdapter();
    const adapter = reviewAdapter();
    const { result } = renderHook(() => useReviewLifecycle(adapter, {}, undefined, structure.adapter));
    await act(async () => { await result.current.open({ kind: "saved-review", reviewId: "r1" }); });
    act(() => result.current.reader!.setView("code-structure"));
    act(() => result.current.reader!.structure!.cancel());
    const partial = { ...emptyResult(), status: "cancelled" as const, summary: { ...emptyResult().summary, complete: false, incompleteDeclarations: 2 } };
    act(() => structure.requests[0].event({ kind: "checkpoint", result: partial }));
    expect(result.current.reader!.structure!.analysis).toMatchObject({ status: "cancelled", result: partial });
    act(() => {
      structure.requests[0].event({ kind: "checkpoint", result: { ...partial, summary: { ...partial.summary, incompleteDeclarations: 99 } } });
      structure.requests[0].resolve(emptyResult());
    });
    await act(async () => {});
    expect(result.current.reader!.structure!.analysis.result?.summary.incompleteDeclarations).toBe(2);
    act(() => result.current.reader!.structure!.retry());
    act(() => result.current.reader!.structure!.cancel());
    act(() => result.current.reader!.structure!.retry());
    act(() => structure.requests[1].resolve({ ...partial, summary: { ...partial.summary, incompleteDeclarations: 88 } }));
    await act(async () => {});
    expect(result.current.reader!.structure!.analysis).toMatchObject({ status: "running", result: partial });
    act(() => result.current.reader!.structure!.cancel());
    act(() => structure.requests[2].resolve({ ...partial, summary: { ...partial.summary, incompleteDeclarations: 3 } }));
    await waitFor(() => expect(result.current.reader!.structure!.analysis.result?.summary.incompleteDeclarations).toBe(3));
    expect(result.current.reader!.structure!.analysis.status).toBe("cancelled");
  });

});
