import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { useTranslation } from "react-i18next";

import { detectBabysitLanguage } from "@/babysit/detected-language";
import {
  isBabysitError,
  type BabysitApplicationAdapter,
  type BabysitBinding,
  type BabysitError,
  type BabysitEligibilityState,
  type BabysitFileRequest,
  type BabysitGenerationPolicy,
  type BabysitProgress,
  type BabysitState,
} from "@/babysit/types";
import { normalizeLocale } from "@/lib/i18n";
import type { FileChangeContent, ReviewIdentity } from "@/review/types";

/** Babysit 只需知道当前 Snapshot identity 与可选中的 File Change。 */
type BabysitLifecycleInput = {
  adapter: BabysitApplicationAdapter | undefined;
  snapshot: ReviewIdentity | undefined;
  fileChange:
    | { kind: "ready"; path: string; content: FileChangeContent }
    | { kind: "empty" | "loading" | "failed" };
};

/** Review 生命周期用于协调 Babysit 的最小接口。 */
export type BabysitLifecycle = {
  binding: BabysitBinding | undefined;
  invalidateSelection(): void;
  clearSnapshot(identity: ReviewIdentity): Promise<void>;
  clearReview(reviewId: string): Promise<void>;
};

/** 将未知 reject 值收敛为 Babysit contract 的 typed error，兼容未来 provider。 */
function babysitFailure(reason: unknown): BabysitError {
  return isBabysitError(reason)
    ? reason
    : { code: "provider", detail: "Babysit provider failed." };
}

/** 管理当前 File Change 的 Babysit eligibility、provider 请求与恢复动作。 */
export function useBabysitLifecycle(input: BabysitLifecycleInput): BabysitLifecycle {
  const { adapter, snapshot, fileChange } = input;
  const { i18n } = useTranslation();
  const locale = normalizeLocale(i18n.resolvedLanguage);
  const explanationAbort = useRef<AbortController>();
  const explanationRequest = useRef(0);
  const eligibilityAbort = useRef<AbortController>();
  const eligibilityRequest = useRef(0);
  const [eligibility, setEligibility] = useState<BabysitEligibilityState>({ kind: "unavailable" });
  const [state, setState] = useState<BabysitState>({ kind: "idle" });
  const lastGenerationPolicy = useRef<BabysitGenerationPolicy>("reuse_cached");

  /** 取消并使当前 provider response 失效；reset=false 仅用于卸载清理。 */
  const invalidateExplanation = useCallback((reset = true) => {
    explanationAbort.current?.abort();
    explanationRequest.current += 1;
    if (reset) setState({ kind: "idle" });
  }, []);

  /** 文件或 Snapshot 边界变化时同时终止 eligibility 与解释请求。 */
  const invalidateSelection = useCallback(() => {
    invalidateExplanation();
    eligibilityAbort.current?.abort();
    eligibilityRequest.current += 1;
    setEligibility({ kind: "unavailable" });
  }, [invalidateExplanation]);

  /** File Change 就绪后查询后端唯一 eligibility，并拒绝过期返回。 */
  useEffect(() => {
    if (!snapshot || fileChange.kind !== "ready" || !adapter) {
      setEligibility({ kind: "unavailable" });
      return;
    }
    const requestId = ++eligibilityRequest.current;
    eligibilityAbort.current?.abort();
    const abort = new AbortController();
    eligibilityAbort.current = abort;
    setEligibility({ kind: "checking" });
    const detectedLanguage = detectBabysitLanguage(fileChange.path, fileChange.content);
    const identity = { ...snapshot, path: fileChange.path };
    void adapter.checkEligibility(identity, detectedLanguage, abort.signal).then((result) => {
      if (!abort.signal.aborted && requestId === eligibilityRequest.current) {
        setEligibility({ kind: "resolved", result });
      }
    }).catch((reason) => {
      if (abort.signal.aborted || requestId !== eligibilityRequest.current) return;
      // 已识别为代码文件时，IPC 失败应呈现可重试错误；真正 explain 仍经过后端 gate。
      if (detectedLanguage) {
        setEligibility({ kind: "resolved", result: { eligible: true, reason: null } });
        setState({
          kind: "failed",
          request: { ...identity, locale, detectedLanguage },
          error: babysitFailure(reason),
          retryable: true,
        });
      } else {
        setEligibility({ kind: "unavailable" });
      }
    });
    return () => abort.abort();
  }, [adapter, fileChange, locale, snapshot]);

  /** 对当前 immutable File Change 发起完整文件分析，并丢弃 stale response。 */
  const generate = useCallback((generationPolicy: BabysitGenerationPolicy) => {
    if (!snapshot || fileChange.kind !== "ready" || !adapter) return;
    if (eligibility.kind !== "resolved" || !eligibility.result.eligible) return;
    const request: BabysitFileRequest = {
      ...snapshot,
      path: fileChange.path,
      locale,
      detectedLanguage: detectBabysitLanguage(fileChange.path, fileChange.content),
    };
    lastGenerationPolicy.current = generationPolicy;
    invalidateExplanation(false);
    const abort = new AbortController();
    const requestId = explanationRequest.current;
    explanationAbort.current = abort;
    setState({ kind: "generating", request });
    void adapter.explainFile({ file: request, generationPolicy }, (progress: BabysitProgress) => {
      if (abort.signal.aborted || requestId !== explanationRequest.current) return;
      setState(progress.phase === "repairing"
        ? { kind: "repairing", request, attempt: progress.attempt }
        : { kind: "generating", request });
    }, abort.signal).then((explanations) => {
      if (!abort.signal.aborted && requestId === explanationRequest.current) {
        setState({ kind: "completed", request, explanations });
      }
    }).catch((reason) => {
      if (!abort.signal.aborted && requestId === explanationRequest.current) {
        setState({ kind: "failed", request, error: babysitFailure(reason), retryable: true });
      }
    });
  }, [adapter, eligibility, fileChange, invalidateExplanation, locale, snapshot]);

  /** 普通开启允许复用精确 identity 的已完成结果。 */
  const explain = useCallback(() => generate("reuse_cached"), [generate]);

  /** 主动重新生成只对已完成结果开放，并立即替换展示状态。 */
  const regenerate = useCallback(() => {
    if (state.kind === "completed") generate("regenerate");
  }, [generate, state.kind]);

  /** 仅在 typed recovery action 要求时打开 native picker；取消不改变失败状态。 */
  const selectExecutable = useCallback(async () => {
    const select = adapter?.selectCodexExecutable;
    const failed = state.kind === "failed" ? state : undefined;
    if (!select || !failed) return;
    const requestId = explanationRequest.current;
    let selected: string | string[] | null;
    try {
      selected = await openDialog({ multiple: false, directory: false });
    } catch {
      setState({
        kind: "failed",
        request: failed.request,
        error: { code: "provider", detail: "Codex executable picker unavailable." },
        retryable: true,
      });
      return;
    }
    if (typeof selected !== "string" || selected.length === 0 || requestId !== explanationRequest.current) return;
    try {
      await select(selected);
      if (requestId !== explanationRequest.current) return;
      invalidateExplanation();
      generate(lastGenerationPolicy.current);
    } catch (reason) {
      if (requestId === explanationRequest.current) {
        setState({ kind: "failed", request: failed.request, error: babysitFailure(reason), retryable: true });
      }
    }
  }, [adapter, generate, invalidateExplanation, state]);

  /** UI locale 改变后清掉旧语言结果，下一次点击使用新的 cache identity。 */
  useEffect(() => {
    if (state.kind !== "idle" && state.request.locale !== locale) invalidateExplanation();
  }, [invalidateExplanation, locale, state]);

  /** Module 卸载时只终止在途请求；Review cache 由宿主的 close/switch 协调。 */
  useEffect(() => () => {
    eligibilityAbort.current?.abort();
    invalidateExplanation(false);
  }, [invalidateExplanation]);

  const binding = useMemo<BabysitBinding | undefined>(() => adapter ? {
    state,
    eligibility,
    explain,
    regenerate,
    cancel: invalidateExplanation,
    retry: () => { if (state.kind === "failed") generate(lastGenerationPolicy.current); },
    selectExecutable,
  } : undefined, [adapter, eligibility, explain, generate, invalidateExplanation, regenerate, selectExecutable, state]);

  /** 清理动作不阻塞 Review 主流程，provider cleanup 失败由 adapter 自行记录。 */
  const clearSnapshot = useCallback(async (identity: ReviewIdentity) => {
    const clear = adapter?.clearSnapshot;
    if (!clear) return;
    try {
      await clear(identity);
    } catch {
      // Cache cleanup 不改变 Review 主流程结果。
    }
  }, [adapter]);

  /** Review 离开 Reader 后删除其进程内解释。 */
  const clearReview = useCallback(async (reviewId: string) => {
    const clear = adapter?.clearReview;
    if (!clear) return;
    try {
      await clear(reviewId);
    } catch {
      // Cache cleanup 不改变 Review 主流程结果。
    }
  }, [adapter]);

  return useMemo(() => ({
    binding,
    invalidateSelection,
    clearSnapshot,
    clearReview,
  }), [binding, clearReview, clearSnapshot, invalidateSelection]);
}
