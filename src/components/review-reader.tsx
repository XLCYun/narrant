import { useCallback, useEffect, useId, useRef, useState } from "react";
import {
  AiBrain01Icon,
  ArrowDown01Icon,
  Copy01Icon,
  FileAddIcon,
  FileEditIcon,
  FileRemoveIcon,
  FileSyncIcon,
  GitCompareIcon,
  LayoutTwoColumnIcon,
  ListViewIcon,
  Logout02Icon,
  ViewAgendaIcon,
} from "@hugeicons/core-free-icons";
import { useTranslation } from "react-i18next";

import { FileChangeLoading } from "@/components/file-change-loading";
import { BabysitDiffEditor, setBabysitExpansion, type BabysitExplanationFilter } from "@/components/babysit-diff-editor";
import { FileTypeIcon } from "@/components/file-type-icon";
import { ReviewOverview } from "@/components/review-overview";
import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import { Spinner } from "@/components/ui/spinner";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { cn } from "@/lib/utils";
import { isBabysitRunning } from "@/babysit/types";
import type { BabysitEligibilityReason, BabysitState } from "@/babysit/types";
import type { ReviewReaderBinding } from "@/review/use-review-lifecycle";
import type { ChangeStatus, FileChangeSummary } from "@/review/types";
export type { ChangeStatus, FileChangeContent, FileChangeSummary, ReviewSnapshot as ReviewReaderSnapshot } from "@/review/types";

/** Monaco Diff 阅读器支持的持久化展示布局。 */
export type DiffMode = "side-by-side" | "inline";

/** 文件树内部使用的目录节点，只表达层级，不承载阅读进度。 */
type DirectoryNode = {
  name: string;
  directories: DirectoryNode[];
  files: FileChangeSummary[];
};

/** Review Reader 的公开展示状态与用户动作。 */
type ReviewReaderProps = {
  review: ReviewReaderBinding;
  appearance: {
    diffMode: DiffMode;
    narrow: boolean;
    editorTheme: string;
    initialTreeWidth: number;
    onDiffModeChange: (mode: DiffMode) => void;
    onTreeWidthChange: (width: number) => void;
    onOpenExternal: (url: string) => void;
  };
};

/** 以不依赖系统 locale 的代码点顺序稳定排列 File Change 路径。 */
function comparePaths(left: string, right: string) {
  return left < right ? -1 : left > right ? 1 : 0;
}

/** 把扁平 File Change 列表投影为可折叠的稳定目录树。 */
function buildDirectoryTree(fileChanges: FileChangeSummary[]): DirectoryNode {
  const root: DirectoryNode = { name: "", directories: [], files: [] };
  for (const change of fileChanges) {
    const parts = change.path.split("/");
    let directory = root;
    for (const name of parts.slice(0, -1)) {
      let child = directory.directories.find((candidate) => candidate.name === name);
      if (!child) {
        child = { name, directories: [], files: [] };
        directory.directories.push(child);
      }
      directory = child;
    }
    directory.files.push(change);
  }
  /** 递归稳定排列每一级目录和文件。 */
  const sort = (directory: DirectoryNode) => {
    directory.directories.sort((left, right) => comparePaths(left.name, right.name));
    directory.files.sort((left, right) => comparePaths(left.path, right.path));
    directory.directories.forEach(sort);
  };
  sort(root);
  return root;
}

/** 为 Monaco 选择常见文件扩展名对应的语言标识。 */
function editorLanguage(path: string) {
  const extension = path.split(".").at(-1)?.toLowerCase();
  const languages: Record<string, string> = {
    js: "javascript", jsx: "javascript", ts: "typescript", tsx: "typescript",
    md: "markdown", json: "json", css: "css", html: "html", rs: "rust",
    py: "python", go: "go", java: "java", yml: "yaml", yaml: "yaml",
    c: "c", cpp: "cpp", h: "cpp", cs: "csharp", sh: "shell",
    sql: "sql", php: "php", rb: "ruby", kt: "kotlin", xml: "xml",
  };
  return extension ? languages[extension] ?? "plaintext" : "plaintext";
}

/** 将前端 eligibility 原因映射为可读 Tooltip，避免把后端错误细节暴露到按钮之外。 */
function babysitEligibilityLabel(reason: BabysitEligibilityReason | undefined, labels: Record<BabysitEligibilityReason, string>) {
  return reason ? labels[reason] : undefined;
}

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
    cancel: string;
    retry: string;
    selectExecutable: string;
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
  const status = state.kind === "generating"
    ? labels.generating
    : state.kind === "repairing"
      ? labels.repairing(state.attempt)
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
        {state.kind === "failed" && state.error.recovery === "select_codex_executable" && onSelectExecutable && <Button variant="outline" size="xs" onClick={onSelectExecutable}>{labels.selectExecutable}</Button>}
        <Button variant="outline" size="xs" onClick={onRetry}>{labels.retry}</Button>
        <Button variant="ghost" size="xs" onClick={copyDiagnostics} aria-label={copied ? labels.copiedDiagnostics : labels.copyDiagnostics}>
          <Icon icon={Copy01Icon} aria-hidden />
          <span className="hidden sm:inline">{copied ? labels.copiedDiagnostics : labels.copyDiagnostics}</span>
        </Button>
      </>}
  </footer>;
}

/** 呈现满高、自由浏览且无审核进度语义的 Review Reader。 */
export function ReviewReader(input: ReviewReaderProps) {
  const { t } = useTranslation(["common", "review"]);
  const { review, appearance } = input;
  const refreshing = review.transition.kind === "refreshing";
  const props = {
    snapshot: review.snapshot,
    fileChange: review.fileChange.kind === "ready" ? review.fileChange.content : undefined,
    pendingFilePath: review.fileChange.kind === "loading" ? review.fileChange.path : undefined,
    fileChangeLoading: review.fileChange.kind === "loading",
    fileChangeError: review.fileChange.kind === "failed" ? review.fileChange.failure.detail : undefined,
    firstFileChangePath: review.fileChange.kind === "failed" ? review.fileChange.path : undefined,
    navigationBusy: review.transition.kind !== "stable",
    ...appearance,
    updateAction: review.update.kind === "available" ? <div role="status" className="flex items-center gap-2 text-sm">
      <span className="hidden text-amber-700 sm:inline dark:text-amber-300">{t(review.snapshot.pullRequest ? "review:pullRequestBranchesUpdated" : "review:selectedBranchesUpdated")}</span>
      <Button aria-label={refreshing ? t("review:refreshing") : t("review:refresh")} disabled={refreshing} aria-busy={refreshing || undefined} onClick={() => void review.refresh()}>{refreshing && <Spinner aria-hidden />}<span className="hidden sm:inline">{refreshing ? t("review:refreshing") : t("review:refresh")}</span></Button>
    </div> : undefined,
    notice: review.update.kind === "failed" ? <p role="alert" className="text-destructive">{review.update.failure.detail}</p> : undefined,
    onSelectFile: review.selectFileChange,
    onRetryFile: review.retryFileChange,
    onClose: () => void review.close(),
  };
  const [collapsedDirectories, setCollapsedDirectories] = useState<Set<string>>(() => new Set());
  const [activeView, setActiveView] = useState<"overview" | "file-changes">("overview");
  const selectedPath = props.pendingFilePath ?? props.fileChange?.path;
  const effectiveDiffMode: DiffMode = props.narrow ? "inline" : props.diffMode;
  const fileTypeIconTheme = props.editorTheme === "vs-dark" ? "dark" : "light";
  const babysit = review.babysit;
  const babysitState = babysit?.state;
  const babysitRunning = babysitState ? isBabysitRunning(babysitState) : false;
  const [babysitVisible, setBabysitVisible] = useState(false);
  const [babysitFilter, setBabysitFilter] = useState<BabysitExplanationFilter>("changed");
  const [expandedBabysitKeys, setExpandedBabysitKeys] = useState<Set<string>>(() => new Set());
  const explainedIdentity = useRef<string>();
  const babysitReasonId = useId();
  const eligibility = babysit?.eligibility;
  const babysitEnabled = eligibility?.kind === "resolved" && Boolean(eligibility.result?.eligible);
  const eligibilityLabels: Record<BabysitEligibilityReason, string> = {
    non_text: t("review:babysitUnsupportedBinary"),
    generated_or_vendored: t("review:babysitUnsupportedGenerated"),
    lock_minified_or_snapshot: t("review:babysitUnsupportedLock"),
    unsupported_language: t("review:babysitUnsupportedExtension"),
  };
  const eligibilityReason = eligibility?.kind === "resolved" && eligibility.result && !eligibility.result.eligible
    ? babysitEligibilityLabel(eligibility.result.reason, eligibilityLabels)
    : t("review:babysitMissingContent");

  /** 更新 Monaco view-zone 的单行展开集合，并为外部原生 DOM listener 提供稳定引用。 */
  const setBabysitExpanded = useCallback((key: string, expanded: boolean) => {
    setExpandedBabysitKeys((current) => setBabysitExpansion(current, key, expanded));
  }, []);

  /** 宝宝模式保持在当前 Review Reader 内；换文件时对新文件自动开始一次完整分析。 */
  useEffect(() => {
    if (!babysitVisible) {
      explainedIdentity.current = undefined;
      return;
    }
    if (!babysit || !babysitEnabled || !selectedPath || babysitState?.kind !== "idle") return;
    const identity = `${review.snapshot.reviewId}:${review.snapshot.snapshotId}:${selectedPath}`;
    if (explainedIdentity.current === identity) return;
    explainedIdentity.current = identity;
    babysit.explain();
  }, [babysit, babysitEnabled, babysitState?.kind, babysitVisible, review.snapshot.reviewId, review.snapshot.snapshotId, selectedPath]);

  /** 关闭当前解释展示时清掉 UI 展开状态，但不影响后端已完成的内存缓存。 */
  useEffect(() => {
    if (!babysitVisible) {
      setBabysitFilter("changed");
      setExpandedBabysitKeys(new Set());
    }
  }, [babysitVisible]);

  /** 切换文件只清掉行级展开状态；changed/all 过滤在同一 Review 内保持。 */
  useEffect(() => {
    setExpandedBabysitKeys(new Set());
  }, [selectedPath]);

  /** 使用图标、颜色与本地化名称共同表达 File Change 状态。 */
  function renderChangeStatus(status: ChangeStatus) {
    const icons: Record<ChangeStatus, typeof FileAddIcon> = {
      added: FileAddIcon, deleted: FileRemoveIcon, modified: FileEditIcon, renamed: FileSyncIcon,
    };
    const colors: Record<ChangeStatus, string> = {
      added: "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400",
      deleted: "bg-rose-500/10 text-rose-600 dark:text-rose-400",
      modified: "bg-blue-500/10 text-blue-600 dark:text-blue-400",
      renamed: "bg-violet-500/10 text-violet-600 dark:text-violet-400",
    };
    const icon = icons[status];
    if (!icon) return null;
    const label = t(`review:${status}`);
    return <Tooltip>
      <TooltipTrigger render={<span role="img" aria-label={label} className={cn("inline-flex size-5 shrink-0 items-center justify-center rounded-md", colors[status])} />}>
        <Icon icon={icon} className="size-3.5" aria-hidden />
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>;
  }

  /** 递归渲染可独立折叠的紧凑目录。 */
  function renderDirectory(directory: DirectoryNode, parentPath = "") {
    return <>
      {directory.directories.map((child) => {
        const path = parentPath ? `${parentPath}/${child.name}` : child.name;
        const collapsed = collapsedDirectories.has(path);
        return <li key={path}>
          <button type="button" aria-expanded={!collapsed}
            className="flex h-7 w-full items-center gap-1 rounded-md px-1.5 text-left text-xs font-medium hover:bg-muted focus-visible:ring-2 focus-visible:ring-ring"
            onClick={() => setCollapsedDirectories((value) => {
              const next = new Set(value);
              if (next.has(path)) next.delete(path); else next.add(path);
              return next;
            })}>
            <Icon icon={ArrowDown01Icon} className={cn("size-3.5 shrink-0 text-muted-foreground transition-transform", collapsed && "-rotate-90")} aria-hidden />
            <FileTypeIcon path={path} kind="directory" expanded={!collapsed} theme={fileTypeIconTheme} testId={`file-type-icon-${child.name}`} />
            <span className="truncate">{child.name}</span>
          </button>
          {!collapsed && <ul role="group" className="ml-3 border-l pl-1">{renderDirectory(child, path)}</ul>}
        </li>;
      })}
      {directory.files.map((change) => <li key={change.path}>
        <Button variant="ghost" type="button"
          aria-current={selectedPath === change.path ? "true" : undefined}
          aria-busy={props.pendingFilePath === change.path || undefined}
          disabled={props.navigationBusy}
          className={cn("h-7 w-full justify-start gap-1.5 rounded-md px-1.5 text-xs font-normal", selectedPath === change.path && "bg-primary/10 text-foreground hover:bg-primary/15")}
          onClick={() => props.onSelectFile(change.path)}>
          <FileTypeIcon path={change.path} kind="file" theme={fileTypeIconTheme} testId={`file-type-icon-${change.path.split("/").at(-1)}`} />
          <span className="min-w-0 flex-1 truncate text-left">{change.oldPath ? `${change.oldPath} → ${change.path}` : change.path.split("/").at(-1)}</span>
          {renderChangeStatus(change.status)}
        </Button>
      </li>)}
    </>;
  }

  return <Tabs defaultValue="overview" onValueChange={(value) => setActiveView(value as "overview" | "file-changes")} aria-label={t("review:reader")} className="h-full min-h-0 gap-0 overflow-hidden rounded-lg border bg-background">
    <div role="toolbar" aria-label={t("review:toolbar")} className="flex min-h-12 shrink-0 items-center gap-2 border-b px-3">
      <TabsList variant="line" aria-label={t("review:reviewViews")} className="h-full shrink-0 p-0">
        <TabsTrigger value="overview" className="px-3">{t("review:overview")}</TabsTrigger>
        <TabsTrigger value="file-changes" className="px-3">{t("review:fileChanges", { count: props.snapshot.fileChanges.length })}</TabsTrigger>
      </TabsList>
      <div className="ml-auto flex min-w-0 shrink-0 items-center justify-end gap-2">
        {props.updateAction}
        {activeView === "file-changes" && <Tooltip>
          <TooltipTrigger render={<span className="inline-flex" />}>
            <Button variant="outline" aria-label={effectiveDiffMode === "side-by-side" ? t("review:switchToInline") : t("review:switchToSideBySide")}
              aria-pressed={effectiveDiffMode === "side-by-side"} disabled={props.narrow}
              onClick={() => props.onDiffModeChange(effectiveDiffMode === "side-by-side" ? "inline" : "side-by-side")}>
              <Icon icon={effectiveDiffMode === "side-by-side" ? ViewAgendaIcon : LayoutTwoColumnIcon} className="-translate-y-px" data-icon="inline-start" aria-hidden />
              <span className="hidden sm:inline">{effectiveDiffMode === "side-by-side" ? t("review:inline") : t("review:sideBySide")}</span>
            </Button>
          </TooltipTrigger>
          <TooltipContent>{props.narrow ? t("review:narrowInlineOnly") : effectiveDiffMode === "side-by-side" ? t("review:switchToInline") : t("review:switchToSideBySide")}</TooltipContent>
        </Tooltip>}
        <Button variant="outline" aria-label={t("review:exit")} onClick={props.onClose}>
          <Icon icon={Logout02Icon} className="-translate-y-px" data-icon="inline-start" aria-hidden />
          <span className="hidden sm:inline">{t("review:exit")}</span>
        </Button>
      </div>
    </div>
    {props.notice && <div className="shrink-0 border-b px-3 py-2 text-xs">{props.notice}</div>}
    <TabsContent value="overview" className="min-h-0 overflow-y-auto"><ReviewOverview review={props.snapshot} onOpenExternal={props.onOpenExternal} /></TabsContent>
    <TabsContent value="file-changes" className="min-h-0 overflow-hidden">
    {props.snapshot.fileChanges.length === 0
      ? <div className="grid min-h-0 flex-1 place-content-center px-6 text-center"><h3 className="font-medium">{t("review:emptyTitle")}</h3><p className="mt-1 text-sm text-muted-foreground">{t("review:emptyDescription")}</p></div>
      : <ResizablePanelGroup orientation="horizontal" className="min-h-0 flex-1"
        defaultLayout={{ "review-file-tree": props.initialTreeWidth, "review-diff": 100 - props.initialTreeWidth }}
        onLayoutChanged={(layout, meta) => { if (meta.isUserInteraction) props.onTreeWidthChange(layout["review-file-tree"]); }}>
        <ResizablePanel id="review-file-tree" minSize="15%" maxSize="45%" defaultSize={`${props.initialTreeWidth}%`}>
          <nav aria-label={t("review:fileTree")} className="h-full overflow-auto border-r bg-muted/20">
            <div className="sticky top-0 z-10 border-b bg-background/95 px-2.5 py-2 backdrop-blur"><h3 className="text-xs font-semibold">{t("review:changeCount", { count: props.snapshot.fileChanges.length })}</h3></div>
            <ul aria-label={t("review:fileTree")} className="p-1">{renderDirectory(buildDirectoryTree(props.snapshot.fileChanges))}</ul>
          </nav>
        </ResizablePanel>
        <ResizableHandle withHandle aria-label={t("review:resizeFileTree")} />
        <ResizablePanel id="review-diff" minSize="45%" className="flex h-full min-h-0 flex-col">
          {props.fileChangeLoading && <FileChangeLoading label={t("review:loadingFileChange")} />}
          {props.fileChangeError && <div role="alert" className="grid min-h-72 place-content-center gap-3 px-6 text-center"><h3>{t("review:fileChangeError")}</h3><p className="text-sm text-muted-foreground">{props.fileChangeError}</p>{props.firstFileChangePath && <Button variant="outline" onClick={props.onRetryFile}>{t("common:retry")}</Button>}</div>}
          {props.fileChange && !props.fileChangeLoading && <section className="flex h-full min-w-0 flex-col" aria-label={t("review:fileChange")} data-before={import.meta.env.VITE_WDIO ? props.fileChange.before ?? "" : undefined} data-after={import.meta.env.VITE_WDIO ? props.fileChange.after ?? "" : undefined}>
            <header className="flex min-h-9 flex-wrap items-center gap-2 border-b px-3 py-1"><h3 className="flex min-w-0 flex-1 items-center gap-2 truncate font-mono text-sm font-medium"><FileTypeIcon path={props.fileChange.path} kind="file" theme={fileTypeIconTheme} />{props.fileChange.path} {renderChangeStatus(props.fileChange.status)}</h3>{babysit && <div className="ml-auto flex min-w-0 items-center gap-1.5">
              <Tooltip>
                <TooltipTrigger render={<Button
                  variant={babysitVisible ? "default" : "outline"}
                  size="sm"
                  aria-label={babysitVisible ? t("review:babysitDisable") : t("review:babysit")}
                  aria-pressed={babysitVisible}
                  aria-disabled={!babysitEnabled}
                  aria-describedby={!babysitEnabled ? babysitReasonId : undefined}
                  aria-busy={babysitRunning || undefined}
                  onClick={() => {
                    if (!babysitEnabled) return;
                    if (babysitVisible && babysitRunning) babysit.cancel();
                    setBabysitVisible((visible) => !visible);
                  }}
                />}>
                  <Icon icon={AiBrain01Icon} data-icon="inline-start" aria-hidden />
                  <span className="hidden sm:inline">{babysitVisible ? t("review:babysitDisable") : t("review:babysit")}</span>
                </TooltipTrigger>
                <TooltipContent>{babysitEnabled ? (babysitVisible ? t("review:babysitDisable") : t("review:babysit")) : eligibilityReason}</TooltipContent>
              </Tooltip>
              {!babysitEnabled && <span id={babysitReasonId} className="sr-only">{eligibilityReason}</span>}
              {babysitVisible && babysitState?.kind === "completed" && <ToggleGroup value={[babysitFilter]} onValueChange={(value) => value[0] && setBabysitFilter(value[0] as BabysitExplanationFilter)} aria-label={t("review:babysitFilter")} variant="outline" size="sm">
                <ToggleGroupItem value="changed" aria-label={t("review:babysitChanged")}><Icon icon={GitCompareIcon} aria-hidden /><span className="hidden sm:inline">{t("review:babysitChanged")}</span></ToggleGroupItem>
                <ToggleGroupItem value="all" aria-label={t("review:babysitAll")}><Icon icon={ListViewIcon} aria-hidden /><span className="hidden sm:inline">{t("review:babysitAll")}</span></ToggleGroupItem>
              </ToggleGroup>}
            </div>}</header>
            {props.fileChange.contentKind === "binary" && <div className="grid min-h-72 place-content-center px-6 text-center"><p>{t("review:binary", { before: props.fileChange.beforeSize === null ? t("review:none") : t("review:bytes", { count: props.fileChange.beforeSize }), after: props.fileChange.afterSize === null ? t("review:none") : t("review:bytes", { count: props.fileChange.afterSize }) })}</p></div>}
            {props.fileChange.contentKind === "submodule" && <div className="grid min-h-72 place-content-center px-6 text-center"><h4 className="font-medium">{t("review:submodule")}</h4><p className="font-mono text-sm text-muted-foreground">{props.fileChange.before ?? t("review:none")} → {props.fileChange.after ?? t("review:none")}</p></div>}
            {props.fileChange.contentKind === "symlink" && <div className="grid min-h-72 place-content-center px-6 text-center"><h4 className="font-medium">{t("review:symlink")}</h4><p className="font-mono text-sm text-muted-foreground">{props.fileChange.before ?? t("review:none")} → {props.fileChange.after ?? t("review:none")}</p></div>}
            {props.fileChange.contentKind === "text" && <div className="min-h-0 flex-1" data-testid="babysit-diff-editor">
              <BabysitDiffEditor
                theme={props.editorTheme}
                height="100%"
                language={editorLanguage(props.fileChange.path)}
                original={props.fileChange.before ?? ""}
                modified={props.fileChange.after ?? ""}
                renderSideBySide={effectiveDiffMode === "side-by-side"}
                explanations={babysitVisible && babysitState?.kind === "completed" ? babysitState.explanations : undefined}
                filter={babysitFilter}
                expandedKeys={expandedBabysitKeys}
                onSetExpanded={setBabysitExpanded}
                labels={{
                  before: t("review:babysitBefore"), after: t("review:babysitAfter"),
                  expand: t("review:babysitExpand"), collapse: t("review:babysitCollapse"), explanation: t("review:babysitExplanations"),
                }}
              />
            </div>}
            <BabysitFooter
              state={babysitVisible ? babysitState : undefined}
              onCancel={() => babysit?.cancel()}
              onRetry={() => babysit?.retry()}
              onSelectExecutable={() => void babysit?.selectExecutable?.()}
              labels={{
                generating: t("review:babysitGenerating"),
                repairing: (attempt) => t("review:babysitRepairing", { attempt }),
                failed: t("review:babysitFailed"),
                codexMissing: t("review:babysitCodexMissing"),
                codexSignedOut: t("review:babysitCodexSignedOut"),
                codexIncompatible: t("review:babysitCodexIncompatible"),
                codexInitializationFailed: t("review:babysitCodexInitializationFailed"),
                cancel: t("review:babysitCancel"),
                selectExecutable: t("review:babysitSelectCodex"),
                retry: t("common:retry"),
                copyDiagnostics: t("review:babysitCopyDiagnostics"),
                copiedDiagnostics: t("review:babysitCopiedDiagnostics"),
              }}
            />
          </section>}
        </ResizablePanel>
      </ResizablePanelGroup>}
    </TabsContent>
  </Tabs>;
}
