import { ArrowRight02Icon, Refresh01Icon } from "@hugeicons/core-free-icons";
import { useEffect, useId, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { branchName } from "@/components/branch-relation";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Combobox,
  ComboboxCollection,
  ComboboxContent,
  ComboboxEmpty,
  ComboboxGroup,
  ComboboxInput,
  ComboboxItem,
  ComboboxLabel,
  ComboboxList,
  ComboboxTrigger,
} from "@/components/ui/combobox";
import { Icon } from "@/components/ui/icon";
import { Spinner } from "@/components/ui/spinner";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";

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

/** 可搜索分支选择器中的一个稳定分组。 */
type BranchGroup = {
  value: string;
  items: string[];
};

/** 单个 Head 或 Base Combobox 的显示与选择参数。 */
type BranchComboboxProps = {
  role: "head" | "base";
  refs: string[];
  currentRef: string;
  defaultRef: string;
  value: string;
  onValueChange: (ref: string) => void;
};

const branchCollator = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/** 判断 canonical ref 是否表示本地分支。 */
function isLocalBranch(ref: string) {
  return ref.startsWith("refs/heads/");
}

/** 仅在短分支名确实被布局截断时展示完整短名称。 */
function TruncatedBranchName({ name, highlighted = false }: { name: string; highlighted?: boolean }) {
  const elementRef = useRef<HTMLSpanElement>(null);
  const [truncated, setTruncated] = useState(false);
  const [tooltipOpen, setTooltipOpen] = useState(false);

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

  useEffect(() => {
    if (!highlighted || !truncated) {
      setTooltipOpen(false);
      return;
    }
    const timer = window.setTimeout(() => setTooltipOpen(true), 400);
    return () => window.clearTimeout(timer);
  }, [highlighted, truncated]);

  return <Tooltip
    disabled={!truncated}
    open={tooltipOpen}
    onOpenChange={(open) => setTooltipOpen(open && truncated)}
  >
    <TooltipTrigger render={<span
      ref={elementRef}
      className="min-w-0 flex-1 truncate font-mono"
      onFocus={measureTruncation}
      onPointerEnter={measureTruncation}
    />}>
      {name}
    </TooltipTrigger>
    <TooltipContent side="right">{name}</TooltipContent>
  </Tooltip>;
}

/** 渲染一个分支选项，并解释当前分支和默认分支的来源。 */
function BranchOption({ refName, currentRef, defaultRef, highlighted = false }: {
  refName: string;
  currentRef: string;
  defaultRef: string;
  highlighted?: boolean;
}) {
  const { t } = useTranslation();
  return <span className="flex min-w-0 flex-1 items-center gap-2 overflow-hidden">
    <TruncatedBranchName name={branchName(refName)} highlighted={highlighted} />
    {refName === currentRef && <Badge className="h-5 shrink-0 py-0 leading-none" variant="secondary">{t("catalog:currentBranch")}</Badge>}
    {refName === defaultRef && <Badge className="h-5 shrink-0 py-0 leading-none" variant="secondary">{t("catalog:defaultBranch")}</Badge>}
  </span>;
}

/** 提供内容自适应、可搜索且保留分支身份解释的 Combobox。 */
function BranchCombobox(props: BranchComboboxProps) {
  const { t } = useTranslation();
  const labelId = useId();
  const wrapperRef = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [highlightedRef, setHighlightedRef] = useState<string>();
  const [triggerFocused, setTriggerFocused] = useState(false);
  const [horizontalLayout, setHorizontalLayout] = useState(false);

  const groups = useMemo<BranchGroup[]>(() => {
    /** 按短分支名执行不区分大小写的自然排序。 */
    const sortRefs = (refs: string[]) => [...refs].sort((left, right) => branchCollator.compare(branchName(left), branchName(right)));
    return [
      { value: t("catalog:localBranchesGroup"), items: sortRefs(props.refs.filter(isLocalBranch)) },
      { value: t("catalog:remoteTrackingBranchesGroup"), items: sortRefs(props.refs.filter((ref) => !isLocalBranch(ref))) },
    ].filter((group) => group.items.length > 0);
  }, [props.refs, t]);

  const label = props.role === "head" ? t("catalog:headBranch") : t("catalog:baseBranch");

  useLayoutEffect(() => {
    const row = wrapperRef.current?.parentElement;
    if (!row) return;
    /** 与 CSS container breakpoint 保持一致，决定 Base 弹层的锚定方向。 */
    function updateLayout() {
      setHorizontalLayout((row?.clientWidth ?? 0) >= 672);
    }
    updateLayout();
    const observer = new ResizeObserver(updateLayout);
    observer.observe(row);
    return () => observer.disconnect();
  }, []);

  /** 关闭弹层时清除本次搜索和高亮，避免下次打开继承旧过滤条件。 */
  function handleOpenChange(nextOpen: boolean) {
    setOpen(nextOpen);
    if (!nextOpen) {
      setQuery("");
      setHighlightedRef(undefined);
    }
  }

  return <div ref={wrapperRef} className="grid w-full min-w-[11.25rem] max-w-full shrink gap-2 text-sm font-medium @min-[42rem]/branches:w-fit">
    <span id={labelId}>{label}</span>
    <Combobox<string>
      items={groups}
      value={props.value || null}
      open={open}
      inputValue={query}
      onOpenChange={handleOpenChange}
      onInputValueChange={setQuery}
      onItemHighlighted={(ref) => setHighlightedRef(ref)}
      onValueChange={(ref) => {
        if (ref) props.onValueChange(ref);
      }}
      filter={(ref, search) => branchName(ref).toLocaleLowerCase().includes(search.trim().toLocaleLowerCase())}
      itemToStringLabel={branchName}
    >
      <ComboboxTrigger
        className="flex h-9 w-full min-w-[11.25rem] max-w-full items-center justify-between gap-1.5 overflow-hidden rounded-4xl border border-input bg-input/30 px-3 py-2 text-sm whitespace-nowrap outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50"
        aria-labelledby={labelId}
        onFocus={() => setTriggerFocused(true)}
        onBlur={() => setTriggerFocused(false)}
        onKeyDown={(event) => {
          if (event.key === " " || event.key === "Enter") {
            event.preventDefault();
            setQuery("");
            setOpen(true);
            return;
          }
          if (event.key.length !== 1 || event.key.trim() === "" || event.altKey || event.ctrlKey || event.metaKey) return;
          event.preventDefault();
          setQuery(event.key);
          setOpen(true);
        }}
      >
        {props.value
          ? <BranchOption refName={props.value} currentRef={props.currentRef} defaultRef={props.defaultRef} highlighted={triggerFocused} />
          : <span className="text-muted-foreground">{props.role === "head" ? t("catalog:selectHead") : t("catalog:selectBase")}</span>}
      </ComboboxTrigger>
      <ComboboxContent
        align={props.role === "base" && horizontalLayout ? "end" : "start"}
        className="w-[min(max(var(--anchor-width),28rem),35rem,var(--available-width))] max-w-[var(--available-width)]"
      >
        <ComboboxInput
          autoFocus
          showTrigger={false}
          showClear={query.length > 0}
          placeholder={t("catalog:searchBranches")}
          aria-label={t("catalog:searchBranches")}
        />
        <ComboboxEmpty>{t("catalog:noMatchingBranches")}</ComboboxEmpty>
        <ComboboxList className="max-h-[min(22rem,calc(var(--available-height)-3rem))]">
          {(group: BranchGroup) => <ComboboxGroup key={group.value} items={group.items}>
            <ComboboxLabel className="sticky top-0 z-10 bg-popover/95">{group.value}</ComboboxLabel>
            <ComboboxCollection>
              {(ref: string) => <ComboboxItem key={ref} value={ref}>
                <BranchOption
                  refName={ref}
                  currentRef={props.currentRef}
                  defaultRef={props.defaultRef}
                  highlighted={highlightedRef === ref}
                />
              </ComboboxItem>}
            </ComboboxCollection>
          </ComboboxGroup>}
        </ComboboxList>
      </ComboboxContent>
    </Combobox>
  </div>;
}

/** 提供创建或继续本地 Branch Selection 的任务面板。 */
export function BranchReviewSelector(props: Props) {
  const { t } = useTranslation();
  const selectionMissing = !props.headRef || !props.baseRef;
  const selectedRemoteBranches = [...new Set([props.headRef, props.baseRef]
    .filter((ref) => ref.startsWith("refs/remotes/"))
    .map(branchName))].join(", ");

  return <TooltipProvider delay={400}>
    <section aria-label={t("catalog:branchSelectionTitle")} className="@container/branches w-full max-w-[72rem] rounded-xl border bg-card/40 p-5">
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
    <div className="mt-5 flex flex-col items-stretch gap-3 @min-[42rem]/branches:flex-row @min-[42rem]/branches:items-end">
      <BranchCombobox role="head" refs={props.refs} currentRef={props.currentRef} defaultRef={props.defaultRef} value={props.headRef} onValueChange={props.onHeadChange} />
      <Icon icon={ArrowRight02Icon} className="mx-auto size-4 rotate-90 text-muted-foreground @min-[42rem]/branches:mx-0 @min-[42rem]/branches:mb-2.5 @min-[42rem]/branches:rotate-0" aria-hidden />
      <BranchCombobox role="base" refs={props.refs} currentRef={props.currentRef} defaultRef={props.defaultRef} value={props.baseRef} onValueChange={props.onBaseChange} />
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
          : <p className="min-w-0 flex-1 truncate text-sm text-muted-foreground">{t("catalog:mergeConfirmation", {
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
    </section>
  </TooltipProvider>;
}
