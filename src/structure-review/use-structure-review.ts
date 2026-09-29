import type { PackId } from "@/language-support/types";
import { useCallback, useEffect, useRef, useState } from "react";
import type { ReviewSnapshot } from "@/review/types";
import type { AnalysisOptions, StructureBinding, StructureReviewAdapter, StructureState } from "./types";

/** 每个打开的 Review 仅保留当前 Snapshot 的结果与在途请求。 */
type Entry = { snapshotId: string; options: AnalysisOptions; state: StructureState; abort?: AbortController };
const IDLE: StructureState = { kind: "idle" };
const DEFAULT_OPTIONS: AnalysisOptions = { includeTests: false };

/** 懒分析、Snapshot 缓存和迟到结果隔离集中在一个应用 Module 中。 */
export function useStructureReview(adapter: StructureReviewAdapter | undefined, snapshot: ReviewSnapshot | undefined, enabled: boolean) {
  const cache = useRef(new Map<string, Entry>());
  const [, render] = useState(0);
  const publish = useCallback(() => render((revision) => revision + 1), []);

  /** 取消只影响指定 Review，晚到的成功结果也不能覆盖取消状态。 */
  const cancelReview = useCallback((reviewId: string) => {
    const entry = cache.current.get(reviewId);
    if (!entry?.abort) return;
    entry.abort.abort();
    entry.abort = undefined;
    entry.state = { kind: "cancelled" };
    publish();
  }, [publish]);

  /** 关闭或替换 Snapshot 时释放图缓存，并使旧请求失去写回资格。 */
  const clearReview = useCallback((reviewId: string) => {
    cache.current.get(reviewId)?.abort?.abort();
    cache.current.delete(reviewId);
    publish();
  }, [publish]);

  /** 显式重试和首次进入使用同一入口，返回值必须匹配请求的固定提交。 */
  const analyze = useCallback((current: ReviewSnapshot, options: AnalysisOptions = DEFAULT_OPTIONS) => {
    if (!adapter) return;
    cache.current.get(current.reviewId)?.abort?.abort();
    const abort = new AbortController();
    const entry: Entry = { snapshotId: current.snapshotId, options: { ...options }, state: { kind: "loading" }, abort };
    cache.current.set(current.reviewId, entry);
    publish();
    const valid = () => !abort.signal.aborted && cache.current.get(current.reviewId) === entry;
    void adapter.analyze({ reviewId: current.reviewId, snapshotId: current.snapshotId, ...entry.options }, abort.signal).then((comparison) => {
      if (!valid()) return;
      if (comparison.reviewId !== current.reviewId || comparison.snapshotId !== current.snapshotId
        || comparison.comparisonBase !== current.comparisonBase || comparison.headRevision !== current.headRevision) {
        throw new Error("Structure comparison does not match the current Snapshot.");
      }
      entry.abort = undefined;
      entry.state = { kind: "ready", comparison };
      publish();
    }).catch((reason: unknown) => {
      if (!valid()) return;
      entry.abort = undefined;
      entry.state = typeof reason === "object" && reason !== null && "code" in reason && reason.code === "cancelled"
        ? { kind: "cancelled" }
        : { kind: "failed", supportPacks: typeof reason === "object" && reason !== null && "supportPacks" in reason && Array.isArray(reason.supportPacks)
          ? reason.supportPacks.filter((id): id is PackId => id === "rust" || id === "typescript-javascript" || id === "python") : undefined, detail: typeof reason === "object" && reason !== null && "detail" in reason
          ? String(reason.detail) : String(reason) };
      publish();
    });
  }, [adapter, publish]);

  useEffect(() => {
    if (!snapshot) return;
    const entry = cache.current.get(snapshot.reviewId);
    if (entry && entry.snapshotId !== snapshot.snapshotId) clearReview(snapshot.reviewId);
    const current = cache.current.get(snapshot.reviewId);
    if (enabled && (!current || current.state.kind === "idle")) analyze(snapshot, current?.options);
  }, [analyze, clearReview, enabled, snapshot]);

  useEffect(() => {
    const entries = cache.current;
    return () => {
      for (const entry of entries.values()) entry.abort?.abort();
      entries.clear();
    };
  }, [adapter]);

  const entry = snapshot ? cache.current.get(snapshot.reviewId) : undefined;
  const options = entry && entry.snapshotId === snapshot?.snapshotId ? entry.options : DEFAULT_OPTIONS;
  const binding: StructureBinding | undefined = adapter && snapshot ? {
    state: entry && entry.snapshotId === snapshot.snapshotId ? entry.state : IDLE,
    supportRequest: JSON.stringify([snapshot.reviewId, snapshot.snapshotId, options.includeTests]),
    options,
    setOptions: (next) => {
      if (next.includeTests === options.includeTests) return;
      if (enabled) analyze(snapshot, next);
      else {
        entry?.abort?.abort();
        cache.current.set(snapshot.reviewId, { snapshotId: snapshot.snapshotId, options: { ...next }, state: IDLE });
        publish();
      }
    },
    retry: () => { if (enabled) analyze(snapshot, options); },
    cancel: () => cancelReview(snapshot.reviewId),
  } : undefined;
  return { binding, cancelReview, clearReview };
}
