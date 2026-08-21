import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  isBabysitError,
  type BabysitApplicationAdapter,
  type BabysitBinding,
  type BabysitError,
  type BabysitFileRequest,
  type BabysitProgress,
  type BabysitState,
} from "@/babysit/types";
import { normalizeLocale } from "@/lib/i18n";
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

/** Review Reader 可直接消费的状态与用户意图 binding。 */
export type ReviewReaderBinding = {
  snapshot: ReviewSnapshot;
  transition:
    | { kind: "stable" }
    | { kind: "switching"; target: ReviewOpenTarget }
    | { kind: "refreshing" }
    | { kind: "closing" };
  fileChange: FileChangeBinding;
  /** 当前 File Change 的最小 Babysit 运行 binding；完整 UI 由 Review Reader 消费。 */
  babysit?: BabysitBinding;
  update: ReviewUpdateBinding;
  selectFileChange(path: string): void;
  retryFileChange(): void;
  refresh(): Promise<ReviewRefreshResult>;
  close(): Promise<ReviewCloseResult>;
};

/** Composition root 与应用级导航 module 消费的 Review lifecycle interface。 */
export type ReviewLifecycleBinding = {
  open(target: ReviewOpenTarget): Promise<ReviewOpenResult>;
  close(): Promise<ReviewCloseResult>;
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

/** 将未知 reject 值收敛为 Babysit contract 的 typed error，兼容未来 provider。 */
function babysitFailure(reason: unknown): BabysitError {
  return isBabysitError(reason)
    ? reason
    : { code: "provider", detail: String(reason) };
}

/** 为一个 composition root 提供唯一当前 Review 的完整生命周期。 */
export function useReviewLifecycle(
  adapter: ReviewApplicationAdapter,
  babysitAdapter?: BabysitApplicationAdapter,
): ReviewLifecycleBinding {
  const { i18n } = useTranslation();
  const locale = normalizeLocale(i18n.resolvedLanguage);
  const [snapshot, setSnapshot] = useState<ReviewSnapshot>();
  const [openingTarget, setOpeningTarget] = useState<ReviewOpenTarget>();
  const [transition, setTransition] = useState<ReviewReaderBinding["transition"]>({ kind: "stable" });
  const [fileChange, setFileChange] = useState<FileChangeBinding>({ kind: "empty" });
  const [update, setUpdate] = useState<ReviewUpdateBinding>({ kind: "current" });
  const snapshotRef = useRef<ReviewSnapshot>();
  const fileRequest = useRef(0);
  const fileAbort = useRef<AbortController>();
  const babysitAbort = useRef<AbortController>();
  const babysitRequest = useRef(0);
  const [babysitState, setBabysitState] = useState<BabysitState>({ kind: "idle" });
  const updateAbort = useRef<AbortController>();
  const lifecycleAbort = useRef<AbortController>();
  const refreshOperation = useRef<{
    abort: AbortController;
    settled: Promise<void>;
    finish: () => void;
  }>();

  /** 取消并使当前 Babysit response 失效；reset=false 供卸载清理使用。 */
  const invalidateBabysit = useCallback((reset = true) => {
    babysitAbort.current?.abort();
    babysitRequest.current += 1;
    if (reset) setBabysitState({ kind: "idle" });
  }, []);

  /** 读取固定 Snapshot 的 File Change，并保证最后一次选择获胜。 */
  const selectFileChange = useCallback((path: string) => {
    const current = snapshotRef.current;
    if (!current) return;
    const request = ++fileRequest.current;
    fileAbort.current?.abort();
    invalidateBabysit();
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
  }, [adapter, invalidateBabysit]);

  /** 对当前 immutable File Change 发起一次完整文件分析，并丢弃 stale response。 */
  const explainBabysit = useCallback(() => {
    const current = snapshotRef.current;
    const selected = fileChange;
    const explain = babysitAdapter?.explainFile;
    if (!current || selected.kind !== "ready" || !explain) return;
    if (selected.content?.contentKind !== "text") return;
    const request: BabysitFileRequest = {
      reviewId: current.reviewId,
      snapshotId: current.snapshotId,
      path: selected.path,
      locale,
    };
    invalidateBabysit(false);
    const abort = new AbortController();
    const requestId = babysitRequest.current;
    babysitAbort.current = abort;
    setBabysitState({ kind: "generating", request });
    void explain(request, (progress: BabysitProgress) => {
      if (abort.signal.aborted || requestId !== babysitRequest.current) return;
      if (progress.phase === "repairing") {
        setBabysitState({ kind: "repairing", request, attempt: progress.attempt });
      } else if (progress.phase === "generating") {
        setBabysitState({ kind: "generating", request });
      }
    }, abort.signal).then((explanations) => {
      if (abort.signal.aborted || requestId !== babysitRequest.current) return;
      setBabysitState({ kind: "completed", request, explanations });
    }).catch((reason) => {
      if (abort.signal.aborted || requestId !== babysitRequest.current) return;
      setBabysitState({ kind: "failed", request, error: babysitFailure(reason), retryable: true });
    });
  }, [babysitAdapter, fileChange, invalidateBabysit, locale]);

  /** 取消当前 Babysit 请求，并清除任何未完成的部分结果。 */
  const cancelBabysit = useCallback(() => {
    invalidateBabysit();
  }, [invalidateBabysit]);

  /** 只允许对最后一次失败的同一 File Change 重新发起分析。 */
  const retryBabysit = useCallback(() => {
    if (babysitState.kind === "failed") explainBabysit();
  }, [babysitState.kind, explainBabysit]);

  const babysit = useMemo<BabysitBinding | undefined>(() => {
    if (!babysitAdapter) return undefined;
    return {
      state: babysitState,
      enabled: Boolean(fileChange.kind === "ready" && fileChange.content?.contentKind === "text"),
      explain: explainBabysit,
      cancel: cancelBabysit,
      retry: retryBabysit,
    };
  }, [babysitAdapter, babysitState, cancelBabysit, explainBabysit, fileChange, retryBabysit]);

  /** UI locale 改变后清掉旧语言结果，下一次点击会使用新的 cache identity。 */
  useEffect(() => {
    if (babysitState.kind !== "idle" && babysitState.request.locale !== locale) {
      invalidateBabysit();
    }
  }, [babysitState, invalidateBabysit, locale]);

  /** 非阻塞检查当前 Review Selection 是否产生 Review Update。 */
  const checkForUpdate = useCallback((current: ReviewSnapshot) => {
    updateAbort.current?.abort();
    const abort = new AbortController();
    updateAbort.current = abort;
    const identity = { reviewId: current.reviewId, snapshotId: current.snapshotId };
    setUpdate({ kind: "checking" });
    void adapter.checkReviewUpdate(current, abort.signal).then((available) => {
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
      return { kind: "opened", reviewId: next.reviewId, snapshotId: next.snapshotId, snapshot: next };
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
    const confirmation = update.kind === "available" ? update.update : undefined;
    if (!current || !confirmation) return { kind: "unchanged" };
    const abort = new AbortController();
    let finishRefresh: () => void = () => {};
    const settled = new Promise<void>((resolve) => { finishRefresh = resolve; });
    refreshOperation.current = { abort, settled, finish: finishRefresh };
    lifecycleAbort.current = abort;
    setTransition({ kind: "refreshing" });
    try {
      const result = await adapter.refreshReview(
        current,
        confirmation,
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
  }, [adapter, presentSnapshot, update]);

  /** 关闭当前 Review；清理失败也不再暴露失效 Reader。 */
  const close = useCallback(async (): Promise<ReviewCloseResult> => {
    const current = snapshotRef.current;
    const refreshing = refreshOperation.current;
    fileAbort.current?.abort();
    invalidateBabysit();
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
  }, [adapter, invalidateBabysit]);

  const retryFileChange = useCallback(() => {
    if (fileChange.kind === "failed") selectFileChange(fileChange.path);
  }, [fileChange, selectFileChange]);

  useEffect(() => () => {
    fileAbort.current?.abort();
    invalidateBabysit(false);
    updateAbort.current?.abort();
    lifecycleAbort.current?.abort();
    const current = snapshotRef.current;
    if (current) void adapter.closeReview(
      { reviewId: current.reviewId, snapshotId: current.snapshotId },
      new AbortController().signal,
    );
  }, [adapter, invalidateBabysit]);

  const reader = useMemo<ReviewReaderBinding | null>(() => snapshot ? {
    snapshot,
    transition,
    fileChange,
    babysit,
    update,
    selectFileChange,
    retryFileChange,
    refresh,
    close,
  } : null, [babysit, close, fileChange, refresh, retryFileChange, selectFileChange, snapshot, transition, update]);

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
