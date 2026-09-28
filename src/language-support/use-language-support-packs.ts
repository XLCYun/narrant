import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type { StructureReviewAdapter } from "@/structure-review/types";
import { isPackActive, type LanguageSupportApplication, type PackCollection, type PackId } from "./types";

/** 一个应用级轮询器服务所有页面，后台下载和分析请求共同决定查询寿命。 */
export function useLanguageSupportPacks(application: LanguageSupportApplication, analysis: StructureReviewAdapter, enabled = true) {
  const [snapshot, setSnapshot] = useState<PackCollection>();
  const [queryError, setQueryError] = useState(false);
  const [actionError, setActionError] = useState<string>();
  const [pending, setPending] = useState<string>();
  const pendingAction = useRef(false);
  const current = useRef<PackCollection>();
  const requests = useRef(0);
  const wake = useRef<() => void>(() => {});

  /** 查询和管理操作共用递增快照入口，较旧结果不能覆盖新阶段。 */
  const accept = useCallback((next: PackCollection) => {
    if (current.current && next.sequence <= current.current.sequence) return;
    current.current = next;
    setSnapshot(next);
  }, []);

  useEffect(() => {
    if (!enabled) return;
    let active = true, querying = false, again = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    /** 串行读取本地状态；仍在启动的分析即使尚未登记下载也必须继续观察。 */
    async function poll() {
      if (!active) return;
      if (querying) { again = true; return; }
      clearTimeout(timer);
      querying = true;
      let failed = false;
      try {
        const next = await application.snapshot();
        if (active) { accept(next); setQueryError(false); }
      } catch {
        failed = true;
        if (active) setQueryError(true);
      } finally {
        querying = false;
        if (active) {
          if (again) { again = false; void poll(); }
          else if (failed || requests.current > 0 || current.current?.packs.some(isPackActive)) timer = setTimeout(() => { void poll(); }, 1_000);
        }
      }
    }
    const refresh = () => { void poll(); };
    const visible = () => { if (document.visibilityState === "visible") refresh(); };
    wake.current = refresh;
    document.addEventListener("visibilitychange", visible);
    refresh();
    return () => { active = false; clearTimeout(timer); wake.current = () => {}; document.removeEventListener("visibilitychange", visible); };
  }, [accept, application, enabled]);

  const structureAdapter = useMemo<StructureReviewAdapter>(() => ({
    /** 跟踪实际 Promise，用户取消界面状态不能让共享下载失去观察者。 */
    async analyze(input, signal) {
      if (!enabled) return analysis.analyze(input, signal);
      requests.current += 1;
      wake.current();
      try { return await analysis.analyze(input, signal); }
      finally { requests.current -= 1; wake.current(); }
    },
  }), [analysis, enabled]);

  /** 管理操作完成后立即重新查询，重试与空闲中的新下载也能启动轮询。 */
  const execute = useCallback(async (key: string, action: () => Promise<PackCollection>) => {
    if (pendingAction.current) return;
    pendingAction.current = true;
    setPending(key);
    setActionError(undefined);
    try { accept(await action()); }
    catch (error) { setActionError(String(error)); }
    finally { pendingAction.current = false; setPending(undefined); wake.current(); }
  }, [accept]);

  return {
    snapshot, queryError, actionError, pending, structureAdapter,
    reload: useCallback(() => wake.current(), []),
    retry: useCallback((id: PackId) => execute(id, () => application.retry(id)), [application, execute]),
    remove: useCallback((id: PackId, version: string) => execute(`${id}:${version}`, () => application.remove(id, version)), [application, execute]),
  };
}

/** 三个界面使用同一控制器，不各自创建轮询。 */
export type LanguageSupportBinding = ReturnType<typeof useLanguageSupportPacks>;
