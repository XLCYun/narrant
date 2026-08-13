import { useCallback, useEffect, useRef, useState } from "react";

import type { RepositoryReviewSummary } from "@/lib/review-types";
import type { ReviewApplicationAdapter, ReviewOpenTarget } from "@/review/review-application";

/** Branch Selection continuation 可呈现的稳定状态。 */
export type BranchSelectionContinuationState =
  | { kind: "incomplete" }
  | { kind: "invalid" }
  | { kind: "checking"; showMessage: boolean }
  | { kind: "ready"; action: "start" | "continue" | "refresh" | "fallback" };

/** 注入的 App Shell Navigation 返回给 continuation 的最小结果。 */
export type BranchSelectionOpenResult =
  | { kind: "navigated" | "trust-required" | "ignored" }
  | { kind: "failed"; detail: string; previousDestinationPreserved: true };

/** Branch Selection continuation 的当前 Repository Entry 输入。 */
export type BranchSelectionContinuationOptions = {
  repositoryEntryId?: number;
  baseRef: string;
  headRef: string;
  reviews: RepositoryReviewSummary[];
  openReview(target: Extract<ReviewOpenTarget, { kind: "branch-selection" }>): Promise<BranchSelectionOpenResult>;
};

/** Branch Selection 选择完成后暴露给展示层的 lifecycle interface。 */
export type BranchSelectionContinuationBinding = {
  state: BranchSelectionContinuationState;
  submit(): Promise<BranchSelectionOpenResult>;
};

/** 集中匹配已有 Review、检查更新并生成唯一的 continuation 决策。 */
export function useBranchSelectionContinuation(
  review: Pick<ReviewApplicationAdapter, "checkBranchSelectionUpdate">,
  options: BranchSelectionContinuationOptions,
): BranchSelectionContinuationBinding {
  const { repositoryEntryId, baseRef, headRef, openReview } = options;
  const [state, setState] = useState<BranchSelectionContinuationState>({ kind: "incomplete" });
  const [checkAttempt, setCheckAttempt] = useState(0);
  const decisionRef = useRef<Extract<ReviewOpenTarget, { kind: "branch-selection" }>>();
  const requestRef = useRef(0);
  const abortRef = useRef<AbortController>();
  const matchingReviewId = options.reviews.find((candidate) => (
    candidate.selection.kind === "branch"
    && candidate.selection.baseRef === options.baseRef
    && candidate.selection.headRef === options.headRef
  ))?.reviewId;

  useEffect(() => {
    const request = ++requestRef.current;
    abortRef.current?.abort();
    decisionRef.current = undefined;
    if (options.repositoryEntryId === undefined || !options.baseRef || !options.headRef) {
      setState({ kind: "incomplete" });
      return;
    }
    if (options.baseRef === options.headRef) {
      setState({ kind: "invalid" });
      return;
    }
    if (matchingReviewId) {
      setState({ kind: "checking", showMessage: false });
      const abort = new AbortController();
      abortRef.current = abort;
      const messageTimer = window.setTimeout(() => {
        if (request !== requestRef.current) return;
        setState((current) => current.kind === "checking"
          ? { kind: "checking", showMessage: true }
          : current);
      }, 1000);
      void review.checkBranchSelectionUpdate(matchingReviewId, abort.signal).then((update) => {
        if (abort.signal.aborted || request !== requestRef.current) return;
        decisionRef.current = {
          kind: "branch-selection",
          repositoryEntryId: options.repositoryEntryId!,
          baseRef: options.baseRef,
          headRef: options.headRef,
          mode: update.changed
            ? { kind: "refresh", reviewId: matchingReviewId, confirmation: update }
            : { kind: "continue", reviewId: matchingReviewId },
        };
        setState({ kind: "ready", action: update.changed ? "refresh" : "continue" });
      }).catch(() => {
        if (abort.signal.aborted || request !== requestRef.current) return;
        decisionRef.current = {
          kind: "branch-selection",
          repositoryEntryId: options.repositoryEntryId!,
          baseRef: options.baseRef,
          headRef: options.headRef,
          mode: { kind: "continue", reviewId: matchingReviewId },
        };
        setState({ kind: "ready", action: "fallback" });
      }).finally(() => window.clearTimeout(messageTimer));
      return () => {
        abort.abort();
        window.clearTimeout(messageTimer);
      };
    }
    decisionRef.current = {
      kind: "branch-selection",
      repositoryEntryId: options.repositoryEntryId,
      baseRef: options.baseRef,
      headRef: options.headRef,
      mode: { kind: "start" },
    };
    setState({ kind: "ready", action: "start" });
  }, [checkAttempt, matchingReviewId, options.baseRef, options.headRef, options.repositoryEntryId, review]);

  /** 提交当前稳定决策；不可提交状态不产生导航副作用。 */
  const submit = useCallback(async (): Promise<BranchSelectionOpenResult> => {
    if (repositoryEntryId === undefined || !baseRef || !headRef || baseRef === headRef) {
      return { kind: "ignored" };
    }
    const decision = decisionRef.current;
    if (!decision || decision.repositoryEntryId !== repositoryEntryId
      || decision.baseRef !== baseRef || decision.headRef !== headRef) {
      return { kind: "ignored" };
    }
    const result = await openReview(decision);
    if (result.kind === "failed" && decision.mode.kind === "refresh"
      && decisionRef.current === decision) {
      setCheckAttempt((attempt) => attempt + 1);
    }
    return result;
  }, [baseRef, headRef, openReview, repositoryEntryId]);

  const visibleState = options.repositoryEntryId === undefined || !options.baseRef || !options.headRef
    ? { kind: "incomplete" } as const
    : options.baseRef === options.headRef
      ? { kind: "invalid" } as const
      : state;
  return { state: visibleState, submit };
}
