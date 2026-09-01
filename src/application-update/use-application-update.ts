import { useCallback, useEffect, useRef, useState } from "react";

import type { ApplicationUpdateApplication, ApplicationUpdateSnapshot } from "@/application-update/application-update";

export type ApplicationUpdateCommand = "load" | "manual-check" | "preference" | "retry" | "restart";

/** 单一快照源的 React binding，严格丢弃 stale 与 duplicate sequence。 */
export function useApplicationUpdate(application: ApplicationUpdateApplication, enabled = true) {
  const [snapshot, setSnapshot] = useState<ApplicationUpdateSnapshot>();
  const snapshotRef = useRef<ApplicationUpdateSnapshot>();
  const [commandError, setCommandError] = useState<ApplicationUpdateCommand>();
  const [pending, setPending] = useState<ApplicationUpdateCommand>();
  const pendingRef = useRef<ApplicationUpdateCommand>();
  const [surfaceOpen, setSurfaceOpen] = useState(false);

  /** 只接受 sequence 严格递增的完整快照，命令返回值与事件复用同一入口。 */
  const accept = useCallback((next: ApplicationUpdateSnapshot) => {
    const current = snapshotRef.current;
    if (current && next.sequence <= current.sequence) return false;
    snapshotRef.current = next;
    setSnapshot(next);
    return true;
  }, []);

  useEffect(() => {
    if (!enabled) return;
    let active = true;
    let unsubscribe: (() => void) | undefined;
    // 先建立事件监听再读取初始快照，消除 subscribe 与 snapshot 之间的启动竞态。
    void application.subscribe((next) => { if (active) accept(next); }).then((stop) => {
      if (!active) {
        stop();
        return;
      }
      unsubscribe = stop;
      return application.snapshot().then((next) => { if (active) accept(next); });
    }).catch(() => {
      if (!active) return;
      setCommandError("load");
    });
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [accept, application, enabled]);

  /** 执行一个无信任参数的更新意图，并接受命令返回的权威快照。 */
  const execute = useCallback(async (
    command: Exclude<ApplicationUpdateCommand, "load" | "preference">,
    operation: () => Promise<ApplicationUpdateSnapshot>,
  ) => {
    if (pendingRef.current) return;
    pendingRef.current = command;
    setPending(command);
    setCommandError(undefined);
    try {
      accept(await operation());
    } catch {
      setCommandError(command);
    } finally {
      pendingRef.current = undefined;
      setPending(undefined);
    }
  }, [accept]);

  /** 乐观切换偏好；IPC 失败仅在没有更新快照插队时回滚到基线。 */
  const setAutomaticDownload = useCallback(async (enabledValue: boolean) => {
    if (!snapshot || pendingRef.current || snapshot.automaticDownload === enabledValue) return;
    const baseline = snapshot;
    const optimistic = { ...baseline, automaticDownload: enabledValue };
    snapshotRef.current = optimistic;
    setSnapshot(optimistic);
    pendingRef.current = "preference";
    setPending("preference");
    setCommandError(undefined);
    try {
      const saved = await application.setAutomaticDownload(enabledValue);
      accept(saved);
    } catch {
      if (snapshotRef.current?.sequence === baseline.sequence) {
        snapshotRef.current = baseline;
        setSnapshot(baseline);
      }
      setCommandError("preference");
    } finally {
      pendingRef.current = undefined;
      setPending(undefined);
    }
  }, [accept, application, snapshot]);

  const openSurface = useCallback(() => setSurfaceOpen(true), []);

  return {
    snapshot,
    commandError,
    pending,
    surfaceOpen,
    openSurface,
    setSurfaceOpen,
    manualCheck: () => execute("manual-check", application.manualCheck),
    setAutomaticDownload,
    retry: () => execute("retry", application.retry),
    restart: () => execute("restart", application.restart),
  };
}

export type ApplicationUpdateBinding = ReturnType<typeof useApplicationUpdate>;
