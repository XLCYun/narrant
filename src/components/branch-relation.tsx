import { ArrowLeft02Icon, ArrowRight02Icon } from "@hugeicons/core-free-icons";
import { Badge } from "@/components/ui/badge";
import { Icon } from "@/components/ui/icon";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
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
  direction = "base-to-head",
}: {
  baseRef: string;
  headRef: string;
  className?: string;
  direction?: "base-to-head" | "head-to-base";
}) {
  const base = branchName(baseRef);
  const head = branchName(headRef);
  const label = `将 Head 分支 ${head} 合入 Base 分支 ${base}`;
  const baseBadge = <Tooltip>
    <TooltipTrigger render={<Badge variant="secondary" className="h-auto max-w-full break-all border border-border bg-muted py-0.5 text-left font-mono font-normal text-foreground shadow-xs whitespace-normal hover:bg-muted" />}>{base}</TooltipTrigger>
    <TooltipContent>{baseRef}</TooltipContent>
  </Tooltip>;
  const headBadge = <Tooltip>
    <TooltipTrigger render={<Badge variant="secondary" className="h-auto max-w-full break-all border border-border bg-muted py-0.5 text-left font-mono font-normal text-foreground shadow-xs whitespace-normal hover:bg-muted" />}>{head}</TooltipTrigger>
    <TooltipContent>{headRef}</TooltipContent>
  </Tooltip>;
  return <span className={cn("inline-flex max-w-full min-w-0 flex-wrap items-center gap-1.5", className)} aria-label={label}>
    {direction === "head-to-base" ? headBadge : baseBadge}
    {direction === "head-to-base"
      ? <Icon icon={ArrowRight02Icon} className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
      : <Icon icon={ArrowLeft02Icon} className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />}
    {direction === "head-to-base" ? baseBadge : headBadge}
  </span>;
}
