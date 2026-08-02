import { ArrowRight02Icon, Refresh01Icon } from "@hugeicons/core-free-icons";
import { useLayoutEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { branchName } from "@/components/branch-relation";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
} from "@/components/ui/select";
import { Spinner } from "@/components/ui/spinner";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";

/** 描述分支选择器当前应提供的主操作及其降级状态。 */
export type BranchSelectorAction = "start" | "continue" | "invalid" | "checking" | "update" | "fallback";

/** 分支审核选择器所需的仓库状态与用户操作回调。 */
type Props = {
  refs: string[];
  currentRef: string;
  defaultRef: string;
  headRef: string;
  baseRef: string;
  action: BranchSelectorAction;
  showCheckingMessage?: boolean;
  reloading?: boolean;
  reloadError?: string;
  onHeadChange: (ref: string) => void;
  onBaseChange: (ref: string) => void;
  onReload: () => void;
  onSubmit: () => void;
};

/** 判断 canonical ref 是否表示本地分支。 */
function isLocalBranch(ref: string) {
  return ref.startsWith("refs/heads/");
}

/** 仅在短分支名确实被布局截断时展示完整短名称。 */
function TruncatedBranchName({ name }: { name: string }) {
  const elementRef = useRef<HTMLSpanElement>(null);
  const [truncated, setTruncated] = useState(false);

  /** 根据元素的实际布局宽度更新截断状态。 */
  function measureTruncation() {
    const element = elementRef.current;
    setTruncated(Boolean(element && element.scrollWidth > element.clientWidth));
  }

  useLayoutEffect(() => {
    measureTruncation();
    if (!elementRef.current || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measureTruncation);
    observer.observe(elementRef.current);
    return () => observer.disconnect();
  }, [name]);

  return <Tooltip disabled={!truncated}>
    <TooltipTrigger render={<span
      ref={elementRef}
      className="min-w-0 flex-1 truncate font-mono"
      onFocus={measureTruncation}
      onPointerEnter={measureTruncation}
    />}>
      {name}
    </TooltipTrigger>
    <TooltipContent>{name}</TooltipContent>
  </Tooltip>;
}

/** 渲染一个分支选项，并解释当前分支和默认分支的来源。 */
function BranchOption({ refName, currentRef, defaultRef }: {
  refName: string;
  currentRef: string;
  defaultRef: string;
}) {
  const { t } = useTranslation();
  return <span className="flex min-w-0 flex-1 items-center gap-2">
    <TruncatedBranchName name={branchName(refName)} />
    {refName === currentRef && <Badge variant="secondary">{t("catalog:currentBranch")}</Badge>}
    {refName === defaultRef && <Badge variant="secondary">{t("catalog:defaultBranch")}</Badge>}
  </span>;
}

/** 提供创建或继续本地 Branch Selection 的任务面板。 */
export function BranchReviewSelector(props: Props) {
  const { t } = useTranslation();
  const localRefs = props.refs.filter(isLocalBranch);
  const remoteTrackingRefs = props.refs.filter((ref) => !isLocalBranch(ref));
  const selectionMissing = !props.headRef || !props.baseRef;
  const selectedRemoteBranches = [...new Set([props.headRef, props.baseRef]
    .filter((ref) => ref.startsWith("refs/remotes/"))
    .map(branchName))].join(", ");

  /** 构造 Head 或 Base 的分组选择器。 */
  function renderSelect(role: "head" | "base") {
    const value = role === "head" ? props.headRef : props.baseRef;
    const onValueChange = role === "head" ? props.onHeadChange : props.onBaseChange;
    const label = role === "head" ? t("catalog:headBranch") : t("catalog:baseBranch");
    return <label className="grid min-w-0 gap-2 text-sm font-medium">
      {label}
      <Select value={value} onValueChange={(nextValue) => onValueChange(nextValue ?? "")}>
        <SelectTrigger className="w-full" aria-label={label}>
          {value
            ? <BranchOption refName={value} currentRef={props.currentRef} defaultRef={props.defaultRef} />
            : <span className="text-muted-foreground">{role === "head" ? t("catalog:selectHead") : t("catalog:selectBase")}</span>}
        </SelectTrigger>
        <SelectContent>
          {localRefs.length > 0 && <SelectGroup>
            <SelectLabel>{t("catalog:localBranchesGroup")}</SelectLabel>
            {localRefs.map((ref) => <SelectItem key={ref} value={ref}>
              <BranchOption refName={ref} currentRef={props.currentRef} defaultRef={props.defaultRef} />
            </SelectItem>)}
          </SelectGroup>}
          {remoteTrackingRefs.length > 0 && <SelectGroup>
            <SelectLabel>{t("catalog:remoteTrackingBranchesGroup")}</SelectLabel>
            {remoteTrackingRefs.map((ref) => <SelectItem key={ref} value={ref}>
              <BranchOption refName={ref} currentRef={props.currentRef} defaultRef={props.defaultRef} />
            </SelectItem>)}
          </SelectGroup>}
        </SelectContent>
      </Select>
    </label>;
  }

  return <section aria-label={t("catalog:branchSelectionTitle")} className="max-w-[48rem] rounded-xl border bg-card/40 p-5">
    <div className="flex items-center justify-between gap-3">
      <h3 className="font-heading text-base font-semibold">{t("catalog:branchSelectionTitle")}</h3>
      <Tooltip>
        <TooltipTrigger render={<Button
          type="button"
          size="icon-sm"
          variant="ghost"
          aria-label={props.reloading ? t("catalog:reloadingBranches") : t("catalog:reloadBranches")}
          disabled={props.reloading}
          onClick={props.onReload}
        />}>
          {props.reloading ? <Spinner aria-hidden /> : <Icon icon={Refresh01Icon} />}
        </TooltipTrigger>
        <TooltipContent>{t("catalog:reloadBranches")}</TooltipContent>
      </Tooltip>
    </div>
    {props.reloadError && <p role="alert" className="mt-3 text-sm text-destructive">{props.reloadError}</p>}
    <div className="mt-5 grid items-end gap-3 sm:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)]">
      {renderSelect("head")}
      <Icon icon={ArrowRight02Icon} className="mb-2.5 hidden size-4 text-muted-foreground sm:block" aria-hidden />
      {renderSelect("base")}
    </div>
    <div className="mt-5 flex flex-col gap-3 border-t pt-4 sm:flex-row sm:items-center sm:justify-between">
      {props.action === "invalid"
        ? <p role="status" className="text-sm text-amber-700 dark:text-amber-300">{t("common:sameBranchSelection")}</p>
        : props.action === "fallback"
          ? <div role="status" className="text-sm text-amber-700 dark:text-amber-300">
            <p>{selectedRemoteBranches
              ? t("catalog:remoteUpdateCheckFailed", { branch: selectedRemoteBranches })
              : t("catalog:branchUpdateCheckFailed")}</p>
            <p className="mt-1">{t("catalog:reviewMayBeStale")}</p>
          </div>
        : props.action === "update"
          ? <div role="status" className="text-sm">
            <p className="font-medium">{t("catalog:branchUpdateAvailable")}</p>
            <p className="mt-1 text-muted-foreground">{t("catalog:branchUpdateDescription")}</p>
          </div>
        : props.action === "checking" && props.showCheckingMessage
          ? <p role="status" className="text-sm text-muted-foreground">{t("catalog:checkingReviewUpdateMessage")}</p>
        : selectionMissing
          ? <p className="text-sm text-muted-foreground">{t("catalog:chooseBothBranches")}</p>
          : <p className="text-sm text-muted-foreground">{t("catalog:mergeConfirmation", {
            head: branchName(props.headRef),
            base: branchName(props.baseRef),
          })}</p>}
      <Button
        type="button"
        className="w-full sm:w-auto"
        aria-label={props.action === "checking" ? t("catalog:checkingReviewUpdate") : undefined}
        disabled={selectionMissing || props.action === "invalid" || props.action === "checking"}
        onClick={props.onSubmit}
      >
        {props.action === "checking" && <Spinner aria-hidden />}
        {props.action === "update"
          ? t("catalog:updateAndContinueReview")
          : props.action === "fallback" ? t("catalog:continueExistingReview")
          : props.action === "continue" || props.action === "checking"
            ? t("common:continueReview") : t("common:startReview")}
      </Button>
    </div>
  </section>;
}
