import { useCallback, useEffect, useMemo, useRef, useState } from "react";

import type { FileChangeContent, ReviewIdentity, ReviewSnapshot } from "@/review/types";

/** Workspace Trust 已由 Repository Entry context 确认的 Review 打开目标。 */
export type ReviewOpenTarget =
  | { kind: "saved-review"; reviewId: string }
  | {
      kind: "pull-request-selection";
      repositoryEntryId: number;
      owner: string;
      repository: string;
      number: number;
    }
  | {
      kind: "branch-selection";
      repositoryEntryId: number;
      baseRef: string;
      headRef: string;
      refreshExisting?: boolean;
      existingReviewId?: string;
    };

/** Review lifecycle seam 返回的稳定失败投影。 */
export type ReviewFailure = {
  kind: "operation-failed" | "workspace-cleanup-failed" | "runtime-lost";
  detail: string;
  retryable: boolean;
};

/** Review Update 检查得到的 moving revision 对照。 */
export type ReviewUpdate = {
  currentBaseRevision: string;
  currentHeadRevision: string;
  availableBaseRevision: string;
  availableHeadRevision: string;
};

/** Review lifecycle implementation 所依赖的应用 adapter。 */
export interface ReviewApplicationAdapter {
  switchReview(
    target: ReviewOpenTarget,
    current: ReviewIdentity | null,
    signal: AbortSignal,
  ): Promise<ReviewSnapshot>;
  checkReviewUpdate(current: ReviewIdentity, signal: AbortSignal): Promise<ReviewUpdate | null>;
  refreshReview(
    current: ReviewIdentity,
    signal: AbortSignal,
  ): Promise<{ kind: "unchanged" } | { kind: "refreshed"; snapshot: ReviewSnapshot }>;
  readFileChange(
    current: ReviewIdentity,
    path: string,
    signal: AbortSignal,
  ): Promise<FileChangeContent>;
  closeReview(
    current: ReviewIdentity,
    signal: AbortSignal,
  ): Promise<{ kind: "closed" } | { kind: "closed-with-cleanup-failure"; detail: string }>;
}

export type ReviewOpenResult =
  | { kind: "opened"; reviewId: string; snapshotId: string }
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
  selectFileChange(path: string): void;
  retryFileChange(): void;
  refresh(): Promise<ReviewRefreshResult>;
  close(): Promise<ReviewCloseResult>;
};

/** 把任意 adapter failure 映射为不依赖字符串分支的稳定失败。 */
function operationFailure(reason: unknown): ReviewFailure {
  return { kind: "operation-failed", detail: String(reason), retryable: true };
}

/** 为一个 composition root 提供唯一当前 Review 的完整生命周期。 */
export function useReviewLifecycle(adapter: ReviewApplicationAdapter) {
  const [snapshot, setSnapshot] = useState<ReviewSnapshot>();
  const [openingTarget, setOpeningTarget] = useState<ReviewOpenTarget>();
  const [transition, setTransition] = useState<ReviewReaderBinding["transition"]>({ kind: "stable" });
  const [fileChange, setFileChange] = useState<FileChangeBinding>({ kind: "empty" });
  const [update, setUpdate] = useState<ReviewUpdateBinding>({ kind: "current" });
  const snapshotRef = useRef<ReviewSnapshot>();
  const fileRequest = useRef(0);
  const fileAbort = useRef<AbortController>();
  const updateAbort = useRef<AbortController>();
  const lifecycleAbort = useRef<AbortController>();
  const refreshOperation = useRef<{
    abort: AbortController;
    settled: Promise<void>;
    finish: () => void;
  }>();

  /** 读取固定 Snapshot 的 File Change，并保证最后一次选择获胜。 */
  const selectFileChange = useCallback((path: string) => {
    const current = snapshotRef.current;
    if (!current) return;
    const request = ++fileRequest.current;
    fileAbort.current?.abort();
    const abort = new AbortController();
    fileAbort.current = abort;
    const identity = { reviewId: current.reviewId, snapshotId: current.snapshotId };
    setFileChange({ kind: "loading", path });
    void adapter.readFileChange(identity, path, abort.signal).then((content) => {
      const latest = snapshotRef.current;
      if (abort.signal.aborted || request !== fileRequest.current
        || latest?.reviewId !== identity.reviewId || latest.snapshotId !== identity.snapshotId) return;
      setFileChange({ kind: "ready", path, content });
    }).catch((reason) => {
      if (abort.signal.aborted || request !== fileRequest.current) return;
      setFileChange({ kind: "failed", path, failure: operationFailure(reason) });
    });
  }, [adapter]);

  /** 非阻塞检查当前 Review Selection 是否产生 Review Update。 */
  const checkForUpdate = useCallback((current: ReviewSnapshot) => {
    updateAbort.current?.abort();
    const abort = new AbortController();
    updateAbort.current = abort;
    const identity = { reviewId: current.reviewId, snapshotId: current.snapshotId };
    setUpdate({ kind: "checking" });
    void adapter.checkReviewUpdate(identity, abort.signal).then((available) => {
      const latest = snapshotRef.current;
      if (abort.signal.aborted || latest?.reviewId !== identity.reviewId
        || latest.snapshotId !== identity.snapshotId) return;
      setUpdate(available ? { kind: "available", update: available } : { kind: "current" });
    }).catch((reason) => {
      if (!abort.signal.aborted) setUpdate({ kind: "failed", failure: operationFailure(reason) });
    });
  }, [adapter]);

  /** 提交一个可读 Snapshot，并确定性读取其首个 File Change。 */
  const presentSnapshot = useCallback((next: ReviewSnapshot) => {
    snapshotRef.current = next;
    setSnapshot(next);
    setTransition({ kind: "stable" });
    const firstPath = [...next.fileChanges]
      .sort((left, right) => left.path < right.path ? -1 : left.path > right.path ? 1 : 0)[0]?.path;
    if (firstPath) selectFileChange(firstPath); else setFileChange({ kind: "empty" });
    checkForUpdate(next);
  }, [checkForUpdate, selectFileChange]);

  /** 打开或切换 Review；失败时保留当前可读 Review。 */
  const open = useCallback(async (target: ReviewOpenTarget): Promise<ReviewOpenResult> => {
    const refreshing = refreshOperation.current;
    if (refreshing) {
      refreshing.abort.abort();
      await refreshing.settled;
    }
    const abort = new AbortController();
    lifecycleAbort.current = abort;
    const current = snapshotRef.current;
    setOpeningTarget(target);
    if (current) setTransition({ kind: "switching", target });
    try {
      const next = await adapter.switchReview(
        target,
        current ? { reviewId: current.reviewId, snapshotId: current.snapshotId } : null,
        abort.signal,
      );
      presentSnapshot(next);
      return { kind: "opened", reviewId: next.reviewId, snapshotId: next.snapshotId };
    } catch (reason) {
      if (current) setTransition({ kind: "stable" });
      return { kind: "failed", failure: operationFailure(reason), previousReviewPreserved: true };
    } finally {
      if (lifecycleAbort.current === abort) lifecycleAbort.current = undefined;
      setOpeningTarget(undefined);
    }
  }, [adapter, presentSnapshot]);

  /** 显式刷新当前 Review，失败时保留旧 Snapshot 与 File Change。 */
  const refresh = useCallback(async (): Promise<ReviewRefreshResult> => {
    const current = snapshotRef.current;
    if (!current) return { kind: "unchanged" };
    const abort = new AbortController();
    let finishRefresh: () => void = () => {};
    const settled = new Promise<void>((resolve) => { finishRefresh = resolve; });
    refreshOperation.current = { abort, settled, finish: finishRefresh };
    lifecycleAbort.current = abort;
    setTransition({ kind: "refreshing" });
    try {
      const result = await adapter.refreshReview(
        { reviewId: current.reviewId, snapshotId: current.snapshotId },
        abort.signal,
      );
      if (result.kind === "unchanged") {
        setTransition({ kind: "stable" });
        return result;
      }
      presentSnapshot(result.snapshot);
      return { kind: "refreshed", reviewId: result.snapshot.reviewId, snapshotId: result.snapshot.snapshotId };
    } catch (reason) {
      setTransition({ kind: "stable" });
      if (abort.signal.aborted) return { kind: "cancelled-by-switch" };
      return { kind: "failed", failure: operationFailure(reason) };
    } finally {
      refreshOperation.current?.finish();
      if (refreshOperation.current?.abort === abort) refreshOperation.current = undefined;
      if (lifecycleAbort.current === abort) lifecycleAbort.current = undefined;
    }
  }, [adapter, presentSnapshot]);

  /** 关闭当前 Review；清理失败也不再暴露失效 Reader。 */
  const close = useCallback(async (): Promise<ReviewCloseResult> => {
    const current = snapshotRef.current;
    const refreshing = refreshOperation.current;
    fileAbort.current?.abort();
    updateAbort.current?.abort();
    lifecycleAbort.current?.abort();
    fileRequest.current += 1;
    snapshotRef.current = undefined;
    setSnapshot(undefined);
    setFileChange({ kind: "empty" });
    setUpdate({ kind: "current" });
    if (!current) return { kind: "closed" };
    setTransition({ kind: "closing" });
    if (refreshing) await refreshing.settled;
    const result = await adapter.closeReview(
      { reviewId: current.reviewId, snapshotId: current.snapshotId },
      new AbortController().signal,
    );
    setTransition({ kind: "stable" });
    return result.kind === "closed"
      ? result
      : {
          kind: "closed-with-cleanup-failure",
          failure: { kind: "workspace-cleanup-failed", detail: result.detail, retryable: true },
        };
  }, [adapter]);

  const retryFileChange = useCallback(() => {
    if (fileChange.kind === "failed") selectFileChange(fileChange.path);
  }, [fileChange, selectFileChange]);

  useEffect(() => () => {
    fileAbort.current?.abort();
    updateAbort.current?.abort();
    lifecycleAbort.current?.abort();
    const current = snapshotRef.current;
    if (current) void adapter.closeReview(
      { reviewId: current.reviewId, snapshotId: current.snapshotId },
      new AbortController().signal,
    );
  }, [adapter]);

  const reader = useMemo<ReviewReaderBinding | null>(() => snapshot ? {
    snapshot,
    transition,
    fileChange,
    update,
    selectFileChange,
    retryFileChange,
    refresh,
    close,
  } : null, [close, fileChange, refresh, retryFileChange, selectFileChange, snapshot, transition, update]);

  return useMemo(() => ({
    open,
    close,
    reader,
    navigation: {
      activeReviewId: snapshot?.reviewId,
      openingTarget,
    },
  }), [close, open, openingTarget, reader, snapshot?.reviewId]);
}
