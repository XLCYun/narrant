import { useCallback, useEffect, useId, useRef, useState, type ReactNode } from "react";
import { Copy01Icon, RefreshIcon, MoreHorizontalIcon } from "@hugeicons/core-free-icons";
import { useTranslation } from "react-i18next";

import type { BabysitChangedRange } from "@/babysit/types";
import type { ReviewFileIdentity } from "@/review/types";
import { Button } from "@/components/ui/button";
import { DropdownMenu, DropdownMenuTrigger, DropdownMenuContent, DropdownMenuItem } from "@/components/ui/dropdown-menu";
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
  onOpenApplicationSettings?: () => void;
};

/** Babysit 对 Review Reader 暴露的紧凑展示投影。 */
type BabysitReviewPresentation = {
  toolbarActions: ReactNode;
  footer: ReactNode;
  explanations: BabysitFileExplanations | undefined;
  onDiffChange(identity: ReviewFileIdentity, ranges: BabysitChangedRange[] | null): void;
};

/** Babysit sticky footer 的状态、动作与本地化文本输入。 */
type BabysitFooterProps = {
  state: BabysitState | undefined;
  onCancel: () => void;
  onRetry: () => void;
  onOpenApplicationSettings?: () => void;
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
    copyDiagnostics: string;
    copiedDiagnostics: string;
    openApplicationSettings: string;
  };
};

/** 在 Diff 底部呈现文件级 Babysit 状态和恢复操作，不打断普通代码阅读。 */
function BabysitFooter({ state, onCancel, onRetry, onOpenApplicationSettings, labels }: BabysitFooterProps) {
  const [copied, setCopied] = useState(false);
  const errorCode = state?.kind === "failed" ? state.error.code : undefined;
  useEffect(() => setCopied(false), [state?.kind, errorCode]);
  if (!state || (state.kind !== "generating" && state.kind !== "repairing" && state.kind !== "failed")) return null;

  const running = state.kind === "generating" || state.kind === "repairing";
  const providerLabels = state.kind === "failed" && state.error.providerId === "claude-code"
    ? {
      missing: labels.claudeMissing,
      signedOut: labels.claudeSignedOut,
      incompatible: labels.claudeIncompatible,
      initializationFailed: labels.claudeInitializationFailed,
    }
    : state.kind === "failed" && state.error.providerId === "codex"
      ? {
        missing: labels.codexMissing,
        signedOut: labels.codexSignedOut,
        incompatible: labels.codexIncompatible,
        initializationFailed: labels.codexInitializationFailed,
      }
      : undefined;
  const status = state.kind === "generating"
    ? labels.generating
    : state.kind === "repairing"
      ? labels.repairing(state.attempt)
      : providerLabels && (state.error.code === "provider_not_found" || state.error.code === "provider_executable_invalid")
        ? providerLabels.missing
        : providerLabels && state.error.code === "provider_signed_out"
          ? providerLabels.signedOut
          : providerLabels && state.error.code === "provider_incompatible"
            ? providerLabels.incompatible
            : providerLabels && state.error.code === "provider_initialization_failed"
              ? providerLabels.initializationFailed
              : labels.failed;
  const diagnostics = state.kind === "failed"
    ? JSON.stringify({
      category: "babysit",
      code: state.error.code,
      ...(state.error.diagnostic ? { provider: state.error.diagnostic } : {}),
    })
    : "";

  /** 复制稳定错误类别与 provider 白名单诊断，不复制路径或 provider detail。 */
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
        {state.kind === "failed" && state.error.recovery === "open_application_settings" && onOpenApplicationSettings && <Button variant="outline" size="xs" onClick={onOpenApplicationSettings}>{labels.openApplicationSettings}</Button>}
        <Button variant="outline" size="xs" onClick={onRetry}>{labels.retry}</Button>
        <Button variant="ghost" size="xs" onClick={copyDiagnostics} aria-label={copied ? labels.copiedDiagnostics : labels.copyDiagnostics}>
          <Icon icon={Copy01Icon} aria-hidden />
          <span className="hidden sm:inline">{copied ? labels.copiedDiagnostics : labels.copyDiagnostics}</span>
        </Button>
      </>}
  </footer>;
}

/** 收拢 Babysit 的启动、生成范围和状态 UI，让 Review Reader 只消费展示投影。 */
export function useBabysitReviewPresentation(input: BabysitReviewPresentationInput): BabysitReviewPresentation {
  const { t } = useTranslation(["common", "review"]);
  const { babysit, reviewId, snapshotId, selectedPath, onOpenApplicationSettings } = input;
  const state = babysit?.state;
  const running = state ? isBabysitRunning(state) : false;
  const [visible, setVisible] = useState(false);
  const [diff, setDiff] = useState<{ identity: string; ranges: BabysitChangedRange[] | null }>();
  const fileIdentity = JSON.stringify([reviewId, snapshotId, selectedPath]);
  const currentFileIdentity = useRef(fileIdentity);
  currentFileIdentity.current = fileIdentity;
  const ranges = diff?.identity === fileIdentity ? diff.ranges : null;
  /** 只接收当前固定文件的 Monaco 范围，拒绝卸载后旧回调。 */
  const onDiffChange = useCallback((identity: ReviewFileIdentity, ranges: BabysitChangedRange[] | null) => {
    const key = JSON.stringify([identity.reviewId, identity.snapshotId, identity.path]);
    if (key === currentFileIdentity.current) setDiff({ identity: key, ranges });
  }, []);
  const explainedIdentity = useRef<string>();
  const reasonId = useId();
  const eligibility = babysit?.eligibility;
  const enabled = eligibility?.kind === "resolved" && Boolean(eligibility.result?.eligible);
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
    if (!babysit || !enabled || !selectedPath || !ranges || state?.kind !== "idle") return;
    const identity = `${reviewId}:${snapshotId}:${selectedPath}`;
    if (explainedIdentity.current === identity) return;
    explainedIdentity.current = identity;
    babysit.explain(ranges);
  }, [babysit, enabled, reviewId, selectedPath, snapshotId, state?.kind, ranges]);

  useEffect(() => {
    if (!visible) {
      explainedIdentity.current = undefined;
      return;
    }
    start();
  }, [start, visible]);


  const toolbarActions = babysit ? <div className="flex shrink-0 items-center gap-2">
    <div className="inline-flex items-center gap-2">
      <Tooltip>
        <TooltipTrigger render={<Button
          variant={visible ? "default" : "outline"}
          size="icon-sm"
          aria-label={t("review:babysit")}
          aria-pressed={visible}
          aria-disabled={!enabled}
          aria-describedby={!enabled ? reasonId : undefined}
          aria-busy={running || undefined}
          className={cn("rounded-full", !enabled && "cursor-not-allowed opacity-50")}
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
          <img src="/icons/babysit-baby.svg" alt="" className="size-4 shrink-0 -translate-y-px object-contain" aria-hidden />
        </TooltipTrigger>
        <TooltipContent>{enabled ? (visible ? t("review:babysitDisable") : t("review:babysit")) : eligibilityReason}</TooltipContent>
      </Tooltip>
      {visible && <DropdownMenu>
        <DropdownMenuTrigger render={<Button variant="outline" size="icon-sm" className="rounded-full" aria-label={t("review:moreActions")} />}>
          <Icon icon={MoreHorizontalIcon} className="-translate-y-px" aria-hidden />
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          <DropdownMenuItem disabled={state?.kind !== "completed" || !ranges} onClick={() => ranges && babysit.regenerate(ranges)}>
            <Icon icon={RefreshIcon} aria-hidden />
            {t("review:babysitRegenerate")}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>}
    </div>
    {!enabled && <span id={reasonId} className="sr-only">{eligibilityReason}</span>}
  </div> : null;

  const footer = <BabysitFooter
    state={visible ? state : undefined}
    onCancel={() => babysit?.cancel()}
    onRetry={() => ranges && babysit?.retry(ranges)}
    onOpenApplicationSettings={onOpenApplicationSettings}
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
      retry: t("common:retry"),
      copyDiagnostics: t("review:babysitCopyDiagnostics"),
      copiedDiagnostics: t("review:babysitCopiedDiagnostics"),
      openApplicationSettings: t("review:babysitOpenApplicationSettings"),
    }}
  />;

  return {
    toolbarActions,
    footer,
    explanations: visible && state?.kind === "completed" ? state.explanations : undefined,
    onDiffChange,
  };
}
