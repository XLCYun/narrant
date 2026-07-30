import { RiArrowLeftLine } from "@remixicon/react";
import { Badge } from "@/components/ui/badge";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

/** 将 canonical Git ref 转成适合界面展示的短分支名。 */
export function branchName(ref = "") {
  return ref.replace(/^refs\/(?:heads|remotes)\//, "");
}

/** 以 GitHub 的合入方向展示 Base 与 Head，并保留完整 ref 的可访问说明。 */
export function BranchRelation({
  baseRef,
  headRef,
  className,
}: {
  baseRef: string;
  headRef: string;
  className?: string;
}) {
  const base = branchName(baseRef);
  const head = branchName(headRef);
  const label = `将 Head 分支 ${head} 合入 Base 分支 ${base}`;
  return <span className={cn("inline-flex max-w-full min-w-0 flex-wrap items-center gap-1.5", className)} aria-label={label}>
    <Tooltip>
      <TooltipTrigger render={<Badge variant="secondary" className="h-auto max-w-full break-all border border-border bg-muted py-0.5 text-left font-mono font-normal text-foreground shadow-xs whitespace-normal hover:bg-muted" />}>{base}</TooltipTrigger>
      <TooltipContent>{baseRef}</TooltipContent>
    </Tooltip>
    <RiArrowLeftLine className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
    <Tooltip>
      <TooltipTrigger render={<Badge variant="secondary" className="h-auto max-w-full break-all border border-border bg-muted py-0.5 text-left font-mono font-normal text-foreground shadow-xs whitespace-normal hover:bg-muted" />}>{head}</TooltipTrigger>
      <TooltipContent>{headRef}</TooltipContent>
    </Tooltip>
  </span>;
}
