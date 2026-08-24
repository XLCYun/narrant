import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { Copy01Icon, GitCompareIcon, ListViewIcon, RefreshIcon } from "@hugeicons/core-free-icons";
import { useTranslation } from "react-i18next";

import type { BabysitExplanationFilter } from "@/components/babysit-diff-editor";
import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { isBabysitRunning } from "@/babysit/types";
import type {
  BabysitBinding,
  BabysitEligibilityReason,
  BabysitFileExplanations,
  BabysitState,
} from "@/babysit/types";
import { cn } from "@/lib/utils";

/** Review Reader 启用 Babysit 展示所需的最小文件上下文。 */
type BabysitReviewPresentationInput = {
  babysit: BabysitBinding | undefined;
  reviewId: string;
  snapshotId: string;
  selectedPath: string | undefined;
};

/** Babysit 对 Review Reader 暴露的紧凑展示投影。 */
type BabysitReviewPresentation = {
  headerActions: ReactNode;
  footer: ReactNode;
  explanations: BabysitFileExplanations | undefined;
  filter: BabysitExplanationFilter;
};

/** Babysit sticky footer 的状态、动作与本地化文本输入。 */
type BabysitFooterProps = {
  state: BabysitState | undefined;
  onCancel: () => void;
  onRetry: () => void;
  onSelectExecutable?: () => void;
  labels: {
    generating: string;
    repairing: (attempt: number) => string;
    failed: string;
    codexMissing: string;
    codexSignedOut: string;
    codexIncompatible: string;
    codexInitializationFailed: string;
    claudeMissing: string;
    claudeSignedOut: string;
    claudeIncompatible: string;
    claudeInitializationFailed: string;
    cancel: string;
    retry: string;
    selectExecutable: string;
    selectClaudeExecutable: string;
    copyDiagnostics: string;
    copiedDiagnostics: string;
  };
};

/** 在 Diff 底部呈现文件级 Babysit 状态和恢复操作，不打断普通代码阅读。 */
function BabysitFooter({ state, onCancel, onRetry, onSelectExecutable, labels }: BabysitFooterProps) {
  const [copied, setCopied] = useState(false);
  const errorCode = state?.kind === "failed" ? state.error.code : undefined;
  useEffect(() => setCopied(false), [state?.kind, errorCode]);
  if (!state || (state.kind !== "generating" && state.kind !== "repairing" && state.kind !== "failed")) return null;

  const running = state.kind === "generating" || state.kind === "repairing";
  const isClaudeFailure = state.kind === "failed" && state.error.providerId === "claude_code";
  const status = state.kind === "generating"
    ? labels.generating
    : state.kind === "repairing"
      ? labels.repairing(state.attempt)
      : isClaudeFailure && (state.error.code === "provider_not_found" || state.error.code === "provider_executable_invalid")
        ? labels.claudeMissing
        : isClaudeFailure && state.error.code === "provider_signed_out"
          ? labels.claudeSignedOut
          : isClaudeFailure && state.error.code === "provider_incompatible"
            ? labels.claudeIncompatible
            : isClaudeFailure && state.error.code === "provider_initialization_failed"
              ? labels.claudeInitializationFailed
      : state.error.code === "codex_not_found" || state.error.code === "codex_executable_invalid"
        ? labels.codexMissing
        : state.error.code === "codex_signed_out"
          ? labels.codexSignedOut
          : state.error.code === "codex_incompatible"
            ? labels.codexIncompatible
            : state.error.code === "codex_initialization_failed"
              ? labels.codexInitializationFailed
              : labels.failed;
  const diagnostics = state.kind === "failed"
    ? JSON.stringify({ category: "babysit", code: state.error.code })
    : "";

  /** 复制只包含稳定错误类别与 code 的脱敏诊断，不复制路径或 provider detail。 */
  function copyDiagnostics() {
    if (!diagnostics) return;
    setCopied(true);
    void navigator.clipboard?.writeText(diagnostics).catch(() => undefined);
  }

  return <footer data-testid="babysit-footer" className="sticky bottom-0 z-10 flex min-h-10 flex-wrap items-center gap-2 border-t bg-background/95 px-3 py-1.5 text-xs shadow-[0_-4px_12px_color-mix(in_oklab,var(--background),transparent_20%)] backdrop-blur" role="status" aria-live="polite">
    <span className={cn("min-w-0 flex-1 truncate", state.kind === "failed" && "text-destructive")}>{status}</span>
    {running
      ? <Button variant="outline" size="xs" onClick={onCancel}>{labels.cancel}</Button>
      : <>
        {state.kind === "failed" && (state.error.recovery === "select_codex_executable" || state.error.recovery === "select_provider_executable") && onSelectExecutable && <Button variant="outline" size="xs" onClick={onSelectExecutable}>{isClaudeFailure ? labels.selectClaudeExecutable : labels.selectExecutable}</Button>}
        <Button variant="outline" size="xs" onClick={onRetry}>{labels.retry}</Button>
        <Button variant="ghost" size="xs" onClick={copyDiagnostics} aria-label={copied ? labels.copiedDiagnostics : labels.copyDiagnostics}>
          <Icon icon={Copy01Icon} aria-hidden />
          <span className="hidden sm:inline">{copied ? labels.copiedDiagnostics : labels.copyDiagnostics}</span>
        </Button>
      </>}
  </footer>;
}

/** 收拢 Babysit 的启动、筛选和状态 UI，让 Review Reader 只消费展示投影。 */
export function useBabysitReviewPresentation(input: BabysitReviewPresentationInput): BabysitReviewPresentation {
  const { t, i18n } = useTranslation(["common", "review"]);
  const { babysit, reviewId, snapshotId, selectedPath } = input;
  const state = babysit?.state;
  const running = state ? isBabysitRunning(state) : false;
  const [visible, setVisible] = useState(false);
  const [filter, setFilter] = useState<BabysitExplanationFilter>("changed");
  const explainedIdentity = useRef<string>();
  const reasonId = useId();
  const eligibility = babysit?.eligibility;
  const enabled = eligibility?.kind === "resolved" && Boolean(eligibility.result?.eligible);
  const locale = i18n.resolvedLanguage ?? i18n.language;
  const eligibilityLabels: Record<BabysitEligibilityReason, string> = {
    non_text: t("review:babysitUnsupportedBinary"),
    generated_or_vendored: t("review:babysitUnsupportedGenerated"),
    lock_minified_or_snapshot: t("review:babysitUnsupportedLock"),
    unsupported_language: t("review:babysitUnsupportedExtension"),
  };
  const eligibilityReason = eligibility?.kind === "resolved" && eligibility.result && !eligibility.result.eligible
    ? eligibilityLabels[eligibility.result.reason]
    : t("review:babysitMissingContent");

  /** 对当前文件只启动一次分析，首次点击与后续换文件共用同一入口。 */
  const start = useCallback(() => {
    if (!babysit || !enabled || !selectedPath || state?.kind !== "idle") return;
    const identity = `${reviewId}:${snapshotId}:${selectedPath}:${locale}`;
    if (explainedIdentity.current === identity) return;
    explainedIdentity.current = identity;
    babysit.explain();
  }, [babysit, enabled, locale, reviewId, selectedPath, snapshotId, state?.kind]);

  useEffect(() => {
    if (!visible) {
      explainedIdentity.current = undefined;
      return;
    }
    start();
  }, [start, visible]);

  useEffect(() => {
    if (!visible) setFilter("changed");
  }, [visible]);

  const headerActions = babysit ? <div className="ml-auto flex min-w-0 items-center gap-1.5">
    {visible && state?.kind === "completed" && <Button
      variant="outline"
      size="sm"
      aria-label={filter === "changed" ? t("review:babysitAll") : t("review:babysitChanged")}
      onClick={() => setFilter((current) => current === "changed" ? "all" : "changed")}
    >
      <Icon icon={filter === "changed" ? ListViewIcon : GitCompareIcon} aria-hidden />
      <span className="hidden sm:inline">{filter === "changed" ? t("review:babysitAll") : t("review:babysitChanged")}</span>
    </Button>}
    <div className={cn("inline-flex", visible && "overflow-hidden rounded-4xl bg-primary")}>
      <Tooltip>
        <TooltipTrigger render={<Button
          variant={visible ? "default" : "outline"}
          size="sm"
          aria-label={visible ? t("review:babysitDisable") : t("review:babysit")}
          aria-pressed={visible}
          aria-disabled={!enabled}
          aria-describedby={!enabled ? reasonId : undefined}
          aria-busy={running || undefined}
          className={cn(!enabled && "cursor-not-allowed opacity-50", visible && "rounded-r-none pr-0.5")}
          onClick={() => {
            if (!enabled) return;
            if (visible) {
              if (running) babysit.cancel();
              setVisible(false);
              return;
            }
            start();
            setVisible(true);
          }}
        />}>
          <img src="/icons/babysit-baby.svg" alt="" className="size-4 shrink-0 -translate-y-px object-contain" data-icon="inline-start" aria-hidden />
          <span className="hidden sm:inline">{visible ? t("review:babysitDisable") : t("review:babysit")}</span>
        </TooltipTrigger>
        <TooltipContent>{enabled ? (visible ? t("review:babysitDisable") : t("review:babysit")) : eligibilityReason}</TooltipContent>
      </Tooltip>
      {visible && <Tooltip>
        <TooltipTrigger render={<Button
          variant="default"
          size="icon-sm"
          className="w-8 justify-start rounded-l-none pl-0.5 pr-0"
          aria-label={t("review:babysitRegenerate")}
          disabled={state?.kind !== "completed"}
          aria-busy={running || undefined}
          onClick={() => babysit.regenerate()}
        />}>
          <Icon icon={RefreshIcon} className="-translate-y-px" aria-hidden />
        </TooltipTrigger>
        <TooltipContent>{t("review:babysitRegenerate")}</TooltipContent>
      </Tooltip>}
    </div>
    {!enabled && <span id={reasonId} className="sr-only">{eligibilityReason}</span>}
  </div> : null;

  const footer = <BabysitFooter
    state={visible ? state : undefined}
    onCancel={() => babysit?.cancel()}
    onRetry={() => babysit?.retry()}
    onSelectExecutable={babysit?.selectExecutable ? () => babysit.selectExecutable?.() : undefined}
    labels={{
      generating: t("review:babysitGenerating"),
      repairing: (attempt) => t("review:babysitRepairing", { attempt }),
      failed: t("review:babysitFailed"),
      codexMissing: t("review:babysitCodexMissing"),
      codexSignedOut: t("review:babysitCodexSignedOut"),
      codexIncompatible: t("review:babysitCodexIncompatible"),
      codexInitializationFailed: t("review:babysitCodexInitializationFailed"),
      claudeMissing: t("review:babysitClaudeMissing"),
      claudeSignedOut: t("review:babysitClaudeSignedOut"),
      claudeIncompatible: t("review:babysitClaudeIncompatible"),
      claudeInitializationFailed: t("review:babysitClaudeInitializationFailed"),
      cancel: t("review:babysitCancel"),
      selectExecutable: t("review:babysitSelectCodex"),
      selectClaudeExecutable: t("review:babysitSelectClaude"),
      retry: t("common:retry"),
      copyDiagnostics: t("review:babysitCopyDiagnostics"),
      copiedDiagnostics: t("review:babysitCopiedDiagnostics"),
    }}
  />;

  return {
    headerActions,
    footer,
    explanations: visible && state?.kind === "completed" ? state.explanations : undefined,
    filter,
  };
}
