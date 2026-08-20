import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type {
  ReviewApplicationAdapter,
  ReviewOpenTarget,
  ReviewUpdate,
} from "@/review/review-application";
import type { FileChangeContent, ReviewSnapshot } from "@/review/types";

export type { ReviewApplicationAdapter, ReviewOpenTarget, ReviewUpdate } from "@/review/review-application";

/** Review lifecycle seam 返回的稳定失败投影。 */
export type ReviewFailure = {
  kind: "operation-failed" | "workspace-cleanup-failed" | "runtime-lost";
  detail: string;
  retryable: boolean;
};

export type ReviewOpenResult =
  | { kind: "opened"; reviewId: string; snapshotId: string; snapshot: ReviewSnapshot }
  | { kind: "failed"; failure: ReviewFailure; previousReviewPreserved: true }
  | { kind: "fatal"; failure: ReviewFailure };

export type ReviewCloseResult =
  | { kind: "closed" }
  | { kind: "closed-with-cleanup-failure"; failure: ReviewFailure };

export type ReviewRefreshResult =
  | { kind: "refreshed"; reviewId: string; snapshotId: string }
  | { kind: "unchanged" }
  | { kind: "failed"; failure: ReviewFailure }
  | { kind: "cancelled-by-switch" };

export type FileChangeBinding =
  | { kind: "empty" }
  | { kind: "loading"; path: string }
  | { kind: "ready"; path: string; content: FileChangeContent }
  | { kind: "failed"; path: string; failure: ReviewFailure };

export type ReviewUpdateBinding =
  | { kind: "checking" }
  | { kind: "current" }
  | { kind: "available"; update: ReviewUpdate }
  | { kind: "failed"; failure: ReviewFailure };

/** Review Reader 的稳定视图。视图属于 Review，而不是一次组件挂载。 */
export type ReviewView = "overview" | "file-changes";

/** Monaco diff 的用户选择。该选择随 Review 保留，但不属于 Snapshot。 */
export type ReviewDiffMode = "side-by-side" | "inline";

/** 进程内一个 Review 的完整 Reader runtime 状态。 */
type ReviewRuntimeEntry = {
  snapshot: ReviewSnapshot;
  fileChange: FileChangeBinding;
  update: ReviewUpdateBinding;
  view: ReviewView;
  diffMode: ReviewDiffMode;
  treeWidth: number;
  collapsedDirectories: string[];
};

/** Review lifecycle 可配置的初始展示偏好。后续修改按 Review 独立保留。 */
export type ReviewLifecycleOptions = {
  initialDiffMode?: ReviewDiffMode;
  initialTreeWidth?: number;
};

/** Review Reader 可直接消费的状态与用户意图 binding。 */
export type ReviewReaderBinding = {
  snapshot: ReviewSnapshot;
  transition:
    | { kind: "stable" }
    | { kind: "switching"; target: ReviewOpenTarget }
    | { kind: "refreshing" }
    | { kind: "closing" };
  fileChange: FileChangeBinding;
  update: ReviewUpdateBinding;
  /** 当前 Review 的 peer view，以及用于保留它的 setter。 */
  view?: ReviewView;
  setView?(view: ReviewView): void;
  /** 当前 Review 的 diff 布局，以及用于保留它的 setter。 */
  diffMode?: ReviewDiffMode;
  setDiffMode?(mode: ReviewDiffMode): void;
  /** 当前 Review 的文件树宽度，以及用于保留它的 setter。 */
  treeWidth?: number;
  setTreeWidth?(width: number): void;
  /** 已折叠目录路径，作为 Reader 导航位置的一部分保存。 */
  collapsedDirectories?: string[];
  toggleDirectory?(path: string): void;
  selectFileChange(path: string): void;
  retryFileChange(): void;
  refresh(): Promise<ReviewRefreshResult>;
  close(): Promise<ReviewCloseResult>;
};

/** Composition root 与应用级导航 module 消费的 Review lifecycle interface。 */
export type ReviewLifecycleBinding = {
  open(target: ReviewOpenTarget): Promise<ReviewOpenResult>;
  close(): Promise<ReviewCloseResult>;
  /** 离开 Reader 只取消 Presented 状态，不释放 Open Review runtime。 */
  unpresent(): void;
  reader: ReviewReaderBinding | null;
  navigation: {
    activeReviewId?: string;
    openingTarget?: ReviewOpenTarget;
  };
};

/** 把任意 adapter failure 映射为不依赖字符串分支的稳定失败。 */
function operationFailure(reason: unknown): ReviewFailure {
  return { kind: "operation-failed", detail: String(reason), retryable: true };
}

/** 从打开目标中提取已知的 durable Review identity，用于 Presented Review 短路。 */
function targetReviewId(target: ReviewOpenTarget): string | undefined {
  if (target.kind === "saved-review") return target.reviewId;
  if (target.kind === "branch-selection" && target.mode.kind === "continue") return target.mode.reviewId;
  return undefined;
}

/** 按稳定路径选出 Snapshot 的首个 File Change。 */
function firstFileChangePath(snapshot: ReviewSnapshot): string | undefined {
  return [...snapshot.fileChanges]
    .sort((left, right) => left.path < right.path ? -1 : left.path > right.path ? 1 : 0)[0]?.path;
}

/** 为一个 composition root 提供进程级 Open Review Set 与 Presented Review。 */
export function useReviewLifecycle(
  adapter: ReviewApplicationAdapter,
  options: ReviewLifecycleOptions = {},
): ReviewLifecycleBinding {
  const initialDiffMode = options.initialDiffMode ?? "side-by-side";
  const initialTreeWidth = options.initialTreeWidth ?? 24;
  const entries = useRef(new Map<string, ReviewRuntimeEntry>());
  const activeReviewIdRef = useRef<string>();
  const [activeReviewId, setActiveReviewId] = useState<string>();
  const [, setRevision] = useState(0);
  const [openingTarget, setOpeningTarget] = useState<ReviewOpenTarget>();
  const [transition, setTransition] = useState<ReviewReaderBinding["transition"]>({ kind: "stable" });
  const fileRequests = useRef(new Map<string, { request: number; abort: AbortController }>());
  const updateAbort = useRef<AbortController>();
  const lifecycleAbort = useRef<AbortController>();
  const refreshOperation = useRef<{
    reviewId: string;
    abort: AbortController;
    settled: Promise<void>;
    finish: () => void;
  }>();

  /** 修改一个 runtime entry，并立即刷新当前 Presented Review 的投影。 */
  const updateEntry = useCallback((reviewId: string, change: (entry: ReviewRuntimeEntry) => ReviewRuntimeEntry) => {
    const current = entries.current.get(reviewId);
    if (!current) return;
    entries.current.set(reviewId, change(current));
    setRevision((value) => value + 1);
  }, []);

  /** 将指定 Review 设为 Presented，并同步 snapshot ref 语义到 React state。 */
  const presentEntry = useCallback((reviewId: string) => {
    if (!entries.current.has(reviewId)) return;
    activeReviewIdRef.current = reviewId;
    setActiveReviewId(reviewId);
    setTransition({ kind: "stable" });
  }, []);

  /** 读取固定 Snapshot 的 File Change，并把结果写回对应 Review 的 runtime entry。 */
  const selectFileChange = useCallback((path: string) => {
    const reviewId = activeReviewIdRef.current;
    const entry = reviewId ? entries.current.get(reviewId) : undefined;
    if (!reviewId || !entry) return;
    const request = (fileRequests.current.get(reviewId)?.request ?? 0) + 1;
    fileRequests.current.get(reviewId)?.abort.abort();
    const abort = new AbortController();
    fileRequests.current.set(reviewId, { request, abort });
    const identity = { reviewId, snapshotId: entry.snapshot.snapshotId };
    updateEntry(reviewId, (current) => ({ ...current, fileChange: { kind: "loading", path } }));
    void adapter.readFileChange(identity, path, abort.signal).then((content) => {
      const latest = entries.current.get(reviewId);
      const latestRequest = fileRequests.current.get(reviewId);
      if (abort.signal.aborted || latestRequest?.request !== request
        || latest?.snapshot.reviewId !== identity.reviewId || latest.snapshot.snapshotId !== identity.snapshotId) return;
      updateEntry(reviewId, (current) => ({ ...current, fileChange: { kind: "ready", path, content } }));
    }).catch((reason) => {
      const latestRequest = fileRequests.current.get(reviewId);
      if (abort.signal.aborted || latestRequest?.request !== request) return;
      updateEntry(reviewId, (current) => ({ ...current, fileChange: { kind: "failed", path, failure: operationFailure(reason) } }));
    });
  }, [adapter, updateEntry]);

  /** 为一个 Presented Review 检查 Selection 更新；离开 Reader 后立即取消该检查。 */
  const checkForUpdate = useCallback((current: ReviewSnapshot) => {
    updateAbort.current?.abort();
    const abort = new AbortController();
    updateAbort.current = abort;
    const reviewId = current.reviewId;
    const identity = { reviewId, snapshotId: current.snapshotId };
    updateEntry(reviewId, (entry) => ({ ...entry, update: { kind: "checking" } }));
    void adapter.checkReviewUpdate(current, abort.signal).then((available) => {
      const latest = entries.current.get(reviewId);
      if (abort.signal.aborted || activeReviewIdRef.current !== reviewId
        || latest?.snapshot.reviewId !== identity.reviewId || latest.snapshot.snapshotId !== identity.snapshotId) return;
      updateEntry(reviewId, (entry) => ({
        ...entry,
        update: available ? { kind: "available", update: available } : { kind: "current" },
      }));
    }).catch((reason) => {
      if (abort.signal.aborted || activeReviewIdRef.current !== reviewId) return;
      updateEntry(reviewId, (entry) => ({ ...entry, update: { kind: "failed", failure: operationFailure(reason) } }));
    });
  }, [adapter, updateEntry]);

  /** 安装一个新 Snapshot；只重置该 Review 的 Snapshot-scoped Reader state。 */
  const installSnapshot = useCallback((next: ReviewSnapshot, existing?: ReviewRuntimeEntry) => {
    const previous = existing && existing.snapshot.snapshotId === next.snapshotId ? existing : undefined;
    const entry: ReviewRuntimeEntry = previous ?? {
      snapshot: next,
      fileChange: { kind: "empty" },
      update: { kind: "current" },
      view: existing?.view ?? "overview",
      diffMode: existing?.diffMode ?? initialDiffMode,
      treeWidth: existing?.treeWidth ?? initialTreeWidth,
      collapsedDirectories: existing?.collapsedDirectories ?? [],
    };
    if (previous) {
      entries.current.set(next.reviewId, { ...previous, snapshot: next });
      return false;
    }
    entries.current.set(next.reviewId, entry);
    return true;
  }, [initialDiffMode, initialTreeWidth]);

  /** 新 Snapshot 首次呈现时清空旧内容，并按稳定路径读取首个 File Change。 */
  const resetSnapshotReader = useCallback((snapshot: ReviewSnapshot) => {
    updateEntry(snapshot.reviewId, (entry) => ({
      ...entry,
      snapshot,
      fileChange: { kind: "empty" },
      update: { kind: "current" },
    }));
    const firstPath = firstFileChangePath(snapshot);
    if (firstPath) selectFileChange(firstPath);
  }, [selectFileChange, updateEntry]);

  /** 关闭一个 Review 的 update/file 操作并移除其 Open Review runtime entry。 */
  const releaseEntry = useCallback((reviewId: string) => {
    fileRequests.current.get(reviewId)?.abort.abort();
    fileRequests.current.delete(reviewId);
    if (activeReviewIdRef.current === reviewId) updateAbort.current?.abort();
    entries.current.delete(reviewId);
    if (activeReviewIdRef.current === reviewId) {
      activeReviewIdRef.current = undefined;
      setActiveReviewId(undefined);
      setTransition({ kind: "stable" });
    }
    setRevision((value) => value + 1);
  }, []);

  /** 打开或切换 Review；失败时保留当前 Presented Review 与全部已有 runtime。 */
  const open = useCallback(async (target: ReviewOpenTarget): Promise<ReviewOpenResult> => {
    const requestedReviewId = targetReviewId(target);
    if (requestedReviewId && requestedReviewId === activeReviewIdRef.current) {
      const current = entries.current.get(requestedReviewId);
      if (current) return {
        kind: "opened",
        reviewId: current.snapshot.reviewId,
        snapshotId: current.snapshot.snapshotId,
        snapshot: current.snapshot,
      };
    }

    const refreshing = refreshOperation.current;
    if (refreshing) {
      refreshing.abort.abort();
      await refreshing.settled;
    }
    const abort = new AbortController();
    lifecycleAbort.current = abort;
    updateAbort.current?.abort();
    const previousReviewId = activeReviewIdRef.current;
    setOpeningTarget(target);
    if (previousReviewId) setTransition({ kind: "switching", target });
    try {
      const previousEntry = previousReviewId ? entries.current.get(previousReviewId) : undefined;
      const next = await adapter.switchReview(
        target,
        previousEntry ? { reviewId: previousEntry.snapshot.reviewId, snapshotId: previousEntry.snapshot.snapshotId } : null,
        abort.signal,
      );
      const existing = entries.current.get(next.reviewId);
      const reusedSnapshot = Boolean(existing && existing.snapshot.snapshotId === next.snapshotId);
      installSnapshot(next, existing);
      presentEntry(next.reviewId);
      if (!reusedSnapshot) {
        resetSnapshotReader(next);
        checkForUpdate(next);
      }
      return { kind: "opened", reviewId: next.reviewId, snapshotId: next.snapshotId, snapshot: entries.current.get(next.reviewId)!.snapshot };
    } catch (reason) {
      if (previousReviewId && activeReviewIdRef.current === previousReviewId) setTransition({ kind: "stable" });
      return { kind: "failed", failure: operationFailure(reason), previousReviewPreserved: true };
    } finally {
      if (lifecycleAbort.current === abort) lifecycleAbort.current = undefined;
      setOpeningTarget(undefined);
    }
  }, [adapter, checkForUpdate, installSnapshot, presentEntry, resetSnapshotReader]);

  /** 显式刷新 Presented Review；只替换该 Review 的 Snapshot-scoped state。 */
  const refresh = useCallback(async (): Promise<ReviewRefreshResult> => {
    const reviewId = activeReviewIdRef.current;
    const currentEntry = reviewId ? entries.current.get(reviewId) : undefined;
    const confirmation = currentEntry?.update.kind === "available" ? currentEntry.update.update : undefined;
    if (!reviewId || !currentEntry || !confirmation) return { kind: "unchanged" };
    const abort = new AbortController();
    let finishRefresh: () => void = () => {};
    const settled = new Promise<void>((resolve) => { finishRefresh = resolve; });
    refreshOperation.current = { reviewId, abort, settled, finish: finishRefresh };
    lifecycleAbort.current = abort;
    setTransition({ kind: "refreshing" });
    try {
      const result = await adapter.refreshReview(currentEntry.snapshot, confirmation, abort.signal);
      if (result.kind === "unchanged") {
        if (activeReviewIdRef.current === reviewId) setTransition({ kind: "stable" });
        return result;
      }
      installSnapshot(result.snapshot, currentEntry);
      if (activeReviewIdRef.current === reviewId) {
        resetSnapshotReader(result.snapshot);
        checkForUpdate(result.snapshot);
        setTransition({ kind: "stable" });
      }
      return { kind: "refreshed", reviewId: result.snapshot.reviewId, snapshotId: result.snapshot.snapshotId };
    } catch (reason) {
      if (activeReviewIdRef.current === reviewId) setTransition({ kind: "stable" });
      if (abort.signal.aborted) return { kind: "cancelled-by-switch" };
      return { kind: "failed", failure: operationFailure(reason) };
    } finally {
      refreshOperation.current?.finish();
      if (refreshOperation.current?.abort === abort) refreshOperation.current = undefined;
      if (lifecycleAbort.current === abort) lifecycleAbort.current = undefined;
    }
  }, [adapter, checkForUpdate, installSnapshot, resetSnapshotReader]);

  /** 关闭当前 Presented Review；该内部 primitive 不由普通导航调用。 */
  const close = useCallback(async (): Promise<ReviewCloseResult> => {
    const reviewId = activeReviewIdRef.current;
    const current = reviewId ? entries.current.get(reviewId) : undefined;
    const refreshing = refreshOperation.current;
    if (!reviewId || !current) return { kind: "closed" };
    updateAbort.current?.abort();
    lifecycleAbort.current?.abort();
    if (refreshing?.reviewId === reviewId) {
      refreshing.abort.abort();
      await refreshing.settled;
    }
    releaseEntry(reviewId);
    setTransition({ kind: "closing" });
    const result = await adapter.closeReview(
      { reviewId: current.snapshot.reviewId, snapshotId: current.snapshot.snapshotId },
      new AbortController().signal,
    );
    setTransition({ kind: "stable" });
    return result.kind === "closed"
      ? result
      : {
          kind: "closed-with-cleanup-failure",
          failure: { kind: "workspace-cleanup-failed", detail: result.detail, retryable: true },
        };
  }, [adapter, releaseEntry]);

  /** 离开 Review Reader，只撤销 Presented Review 和 update 检查，保留全部 runtime。 */
  const unpresent = useCallback(() => {
    updateAbort.current?.abort();
    lifecycleAbort.current?.abort();
    refreshOperation.current?.abort.abort();
    activeReviewIdRef.current = undefined;
    setActiveReviewId(undefined);
    setTransition({ kind: "stable" });
  }, []);

  /** 更新当前 Review 的 peer view，而不触碰其 Snapshot 或 reader content。 */
  const setView = useCallback((view: ReviewView) => {
    const reviewId = activeReviewIdRef.current;
    if (reviewId) updateEntry(reviewId, (entry) => ({ ...entry, view }));
  }, [updateEntry]);

  /** 更新当前 Review 的 diff layout，而不影响其他 Open Review。 */
  const setDiffMode = useCallback((diffMode: ReviewDiffMode) => {
    const reviewId = activeReviewIdRef.current;
    if (reviewId) updateEntry(reviewId, (entry) => ({ ...entry, diffMode }));
  }, [updateEntry]);

  /** 更新当前 Review 的文件树宽度，并让它随 Review 返回。 */
  const setTreeWidth = useCallback((treeWidth: number) => {
    const reviewId = activeReviewIdRef.current;
    if (reviewId) updateEntry(reviewId, (entry) => ({ ...entry, treeWidth }));
  }, [updateEntry]);

  /** 切换当前 Review 的目录折叠状态，作为 Reader 导航位置保存。 */
  const toggleDirectory = useCallback((path: string) => {
    const reviewId = activeReviewIdRef.current;
    if (!reviewId) return;
    updateEntry(reviewId, (entry) => ({
      ...entry,
      collapsedDirectories: entry.collapsedDirectories.includes(path)
        ? entry.collapsedDirectories.filter((candidate) => candidate !== path)
        : [...entry.collapsedDirectories, path],
    }));
  }, [updateEntry]);

  /** 重新读取当前 Review 上一次失败的 File Change。 */
  const retryFileChange = useCallback(() => {
    const reviewId = activeReviewIdRef.current;
    const current = reviewId ? entries.current.get(reviewId) : undefined;
    if (current?.fileChange.kind === "failed") selectFileChange(current.fileChange.path);
  }, [selectFileChange]);

  /** 应用卸载等正常退出路径释放整个 Open Review Set；历史记录仍由后端保留。 */
  useEffect(() => () => {
    updateAbort.current?.abort();
    lifecycleAbort.current?.abort();
    for (const [reviewId, entry] of entries.current) {
      fileRequests.current.get(reviewId)?.abort.abort();
      void adapter.closeReview(
        { reviewId, snapshotId: entry.snapshot.snapshotId },
        new AbortController().signal,
      );
    }
    entries.current.clear();
  }, [adapter]);

  const activeEntry = activeReviewId ? entries.current.get(activeReviewId) : undefined;
  const reader = useMemo<ReviewReaderBinding | null>(() => activeEntry ? {
    snapshot: activeEntry.snapshot,
    transition,
    fileChange: activeEntry.fileChange,
    update: activeEntry.update,
    view: activeEntry.view,
    setView,
    diffMode: activeEntry.diffMode,
    setDiffMode,
    treeWidth: activeEntry.treeWidth,
    setTreeWidth,
    collapsedDirectories: activeEntry.collapsedDirectories,
    toggleDirectory,
    selectFileChange,
    retryFileChange,
    refresh,
    close,
  } : null, [activeEntry, close, refresh, retryFileChange, selectFileChange, setDiffMode, setTreeWidth, setView, toggleDirectory, transition]);

  return useMemo(() => ({
    open,
    close,
    unpresent,
    reader,
    navigation: {
      activeReviewId,
      openingTarget,
    },
  }), [activeReviewId, close, open, openingTarget, reader, unpresent]);
}
