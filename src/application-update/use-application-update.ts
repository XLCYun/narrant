import { useCallback, useEffect, useRef, useState } from "react";

import type { ApplicationUpdateApplication, ApplicationUpdateSnapshot } from "@/application-update/application-update";

/** 前端更新控制器可观察的命令阶段与稳定错误类别。 */
export type ApplicationUpdateCommand = "load" | "manual-check" | "preference" | "retry" | "restart";

/** 以 Rust 权威快照为单一状态源，并在其上提供可回滚的偏好乐观显示。 */
export function useApplicationUpdate(application: ApplicationUpdateApplication, enabled = true) {
  const [snapshot, setSnapshot] = useState<ApplicationUpdateSnapshot>();
  const authoritativeSnapshotRef = useRef<ApplicationUpdateSnapshot>();
  const displayedSnapshotRef = useRef<ApplicationUpdateSnapshot>();
  const [commandError, setCommandError] = useState<ApplicationUpdateCommand>();
  const [pending, setPending] = useState<ApplicationUpdateCommand>();
  const pendingRef = useRef<ApplicationUpdateCommand>();
  const preferenceRequestRef = useRef(0);
  const preferenceIntentRef = useRef<{ request: number; enabled: boolean }>();
  const [loadAttempt, setLoadAttempt] = useState(0);
  const [surfaceOpen, setSurfaceOpen] = useState(false);

  /** 以最新权威快照为底，只在当前偏好请求未完成时覆盖开关的乐观显示值。 */
  const publishDisplay = useCallback((authoritative: ApplicationUpdateSnapshot) => {
    const intent = preferenceIntentRef.current;
    const displayed = intent
      ? { ...authoritative, automaticDownload: intent.enabled }
      : authoritative;
    displayedSnapshotRef.current = displayed;
    setSnapshot(displayed);
  }, []);

  /** 只接受 sequence 严格递增的完整快照，命令返回值与事件复用同一入口。 */
  const accept = useCallback((next: ApplicationUpdateSnapshot) => {
    const current = authoritativeSnapshotRef.current;
    if (current && next.sequence <= current.sequence) return false;
    authoritativeSnapshotRef.current = next;
    publishDisplay(next);
    // 新快照是跨事件与命令的唯一权威来源，也会使旧命令错误失效。
    setCommandError(undefined);
    return true;
  }, [publishDisplay]);

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
      return application.snapshot().then((next) => {
        if (!active) return;
        accept(next);
        setCommandError((current) => current === "load" ? undefined : current);
      });
    }).catch(() => {
      if (!active) return;
      setCommandError("load");
    });
    return () => {
      active = false;
      unsubscribe?.();
    };
  }, [accept, application, enabled, loadAttempt]);

  /** 重新建立单一事件监听并读取权威快照，用于从订阅或初始读取失败中恢复。 */
  const reload = useCallback(() => {
    setCommandError((current) => current === "load" ? undefined : current);
    setLoadAttempt((current) => current + 1);
  }, []);

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

  /** 乐观切换偏好；权威快照与显示意图分离，允许多个请求安全乱序完成。 */
  const setAutomaticDownload = useCallback(async (enabledValue: boolean) => {
    const authoritative = authoritativeSnapshotRef.current;
    const displayed = displayedSnapshotRef.current;
    if (!authoritative || !displayed || (pendingRef.current && pendingRef.current !== "preference") || displayed.automaticDownload === enabledValue) return;
    const request = ++preferenceRequestRef.current;
    preferenceIntentRef.current = { request, enabled: enabledValue };
    publishDisplay(authoritative);
    pendingRef.current = "preference";
    setPending("preference");
    setCommandError(undefined);
    try {
      const saved = await application.setAutomaticDownload(enabledValue);
      // 每个命令结果都是 Rust 权威候选，request token 不能阻断更高 sequence。
      accept(saved);
    } catch {
      if (request !== preferenceRequestRef.current) return;
      const current = authoritativeSnapshotRef.current;
      preferenceIntentRef.current = undefined;
      if (current) publishDisplay(current);
      // 没有权威结果插队时，本次失败才显示“未保存并已恢复”。
      if (current?.sequence === authoritative.sequence) {
        setCommandError("preference");
      } else {
        setCommandError((error) => error === "preference" ? undefined : error);
      }
    } finally {
      if (request === preferenceRequestRef.current) {
        preferenceIntentRef.current = undefined;
        const current = authoritativeSnapshotRef.current;
        if (current) publishDisplay(current);
        pendingRef.current = undefined;
        setPending(undefined);
      }
    }
  }, [accept, application, publishDisplay]);

  const openSurface = useCallback(() => setSurfaceOpen(true), []);

  return {
    snapshot,
    commandError,
    pending,
    surfaceOpen,
    reload,
    openSurface,
    setSurfaceOpen,
    manualCheck: () => execute("manual-check", application.manualCheck),
    setAutomaticDownload,
    retry: () => execute("retry", application.retry),
    restart: () => execute("restart", application.restart),
  };
}

/** 更新组件共享的 controller 返回契约。 */
export type ApplicationUpdateBinding = ReturnType<typeof useApplicationUpdate>;
