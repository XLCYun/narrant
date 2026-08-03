import { ArrowLeft02Icon, ArrowRight02Icon } from "@hugeicons/core-free-icons";
import { Badge } from "@/components/ui/badge";
import { CopyableReference } from "@/components/copyable-reference";
import { Icon } from "@/components/ui/icon";
import { useTranslation } from "react-i18next";
import { cn } from "@/lib/utils";

/** 将 canonical Git ref 转成适合界面展示的短分支名。 */
export function branchName(ref = "") {
  return ref.replace(/^refs\/(?:heads|remotes)\//, "");
}

/** 展示 Base 与 Head 的关系，并保留完整 ref 的可访问说明。 */
export function BranchRelation({
  baseRef,
  headRef,
  className,
  direction = "head-to-base",
  interactive = true,
}: {
  baseRef: string;
  headRef: string;
  className?: string;
  direction?: "base-to-head" | "head-to-base";
  interactive?: boolean;
}) {
  const { t } = useTranslation("review");
  const base = branchName(baseRef);
  const head = branchName(headRef);
  const label = `将 Head 分支 ${head} 合入 Base 分支 ${base}`;
  const badgeClassName = "max-w-full cursor-pointer break-all whitespace-normal rounded-md border border-border bg-muted px-2 py-0.5 text-left font-mono text-sm font-normal text-foreground shadow-xs hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";
  const staticBadge = (name: string) => <Badge variant="secondary" className="max-w-full break-all whitespace-normal border border-border bg-muted text-left font-mono font-normal text-foreground shadow-xs hover:bg-muted">{name}</Badge>;
  const baseBadge = interactive ? <CopyableReference value={base} copiedLabel={t("branchCopied")} copyFailedLabel={t("referenceCopyFailed")} className={badgeClassName}>{base}</CopyableReference> : staticBadge(base);
  const headBadge = interactive ? <CopyableReference value={head} copiedLabel={t("branchCopied")} copyFailedLabel={t("referenceCopyFailed")} className={badgeClassName}>{head}</CopyableReference> : staticBadge(head);
  return <span className={cn("inline-flex max-w-full min-w-0 flex-wrap items-center gap-1.5", className)} aria-label={label}>
    {direction === "head-to-base" ? headBadge : baseBadge}
    {direction === "head-to-base"
      ? <Icon icon={ArrowRight02Icon} className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
      : <Icon icon={ArrowLeft02Icon} className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />}
    {direction === "head-to-base" ? baseBadge : headBadge}
  </span>;
}
