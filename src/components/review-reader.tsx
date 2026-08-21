import { useState } from "react";
import { DiffEditor } from "@monaco-editor/react";
import {
  ArrowDown01Icon,
  FileAddIcon,
  FileEditIcon,
  FileRemoveIcon,
  FileSyncIcon,
  LayoutTwoColumnIcon,
  Logout02Icon,
  ViewAgendaIcon,
} from "@hugeicons/core-free-icons";
import { useTranslation } from "react-i18next";

import { FileChangeLoading } from "@/components/file-change-loading";
import { FileTypeIcon } from "@/components/file-type-icon";
import { ReviewOverview } from "@/components/review-overview";
import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import { Spinner } from "@/components/ui/spinner";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { isBabysitRunning } from "@/babysit/types";
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
  const babysitAction = babysitRunning ? "cancel" : babysitState?.kind === "failed" ? "retry" : "explain";
  const babysitLabel = babysitAction === "cancel"
    ? t("review:babysitCancel")
    : babysitAction === "retry"
      ? t("common:retry")
      : t("review:babysit");

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
        <ResizablePanel id="review-diff" minSize="45%">
          {props.fileChangeLoading && <FileChangeLoading label={t("review:loadingFileChange")} />}
          {props.fileChangeError && <div role="alert" className="grid min-h-72 place-content-center gap-3 px-6 text-center"><h3>{t("review:fileChangeError")}</h3><p className="text-sm text-muted-foreground">{props.fileChangeError}</p>{props.firstFileChangePath && <Button variant="outline" onClick={props.onRetryFile}>{t("common:retry")}</Button>}</div>}
          {props.fileChange && !props.fileChangeLoading && <section className="h-full min-w-0" aria-label={t("review:fileChange")} data-before={import.meta.env.VITE_WDIO ? props.fileChange.before ?? "" : undefined} data-after={import.meta.env.VITE_WDIO ? props.fileChange.after ?? "" : undefined}>
            <header className="flex h-9 items-center gap-2 border-b px-3"><h3 className="flex min-w-0 flex-1 items-center gap-2 truncate font-mono text-sm font-medium"><FileTypeIcon path={props.fileChange.path} kind="file" theme={fileTypeIconTheme} />{props.fileChange.path} {renderChangeStatus(props.fileChange.status)}</h3>{babysit && <Button variant="outline" size="sm" aria-label={babysitLabel} disabled={!babysit.enabled} aria-busy={babysitRunning} onClick={() => {
              if (babysitAction === "cancel") babysit.cancel();
              else if (babysitAction === "retry") babysit.retry();
              else babysit.explain();
            }}>{babysitLabel}</Button>}</header>
            {props.fileChange.contentKind === "binary" && <div className="grid min-h-72 place-content-center px-6 text-center"><p>{t("review:binary", { before: props.fileChange.beforeSize === null ? t("review:none") : t("review:bytes", { count: props.fileChange.beforeSize }), after: props.fileChange.afterSize === null ? t("review:none") : t("review:bytes", { count: props.fileChange.afterSize }) })}</p></div>}
            {props.fileChange.contentKind === "submodule" && <div className="grid min-h-72 place-content-center px-6 text-center"><h4 className="font-medium">{t("review:submodule")}</h4><p className="font-mono text-sm text-muted-foreground">{props.fileChange.before ?? t("review:none")} → {props.fileChange.after ?? t("review:none")}</p></div>}
            {props.fileChange.contentKind === "symlink" && <div className="grid min-h-72 place-content-center px-6 text-center"><h4 className="font-medium">{t("review:symlink")}</h4><p className="font-mono text-sm text-muted-foreground">{props.fileChange.before ?? t("review:none")} → {props.fileChange.after ?? t("review:none")}</p></div>}
            {props.fileChange.contentKind === "text" && <DiffEditor theme={props.editorTheme} height="calc(100% - 2.25rem)" language={editorLanguage(props.fileChange.path)} original={props.fileChange.before ?? ""} modified={props.fileChange.after ?? ""} options={{ readOnly: true, renderSideBySide: effectiveDiffMode === "side-by-side", ignoreTrimWhitespace: false, fontFamily: "'JetBrains Mono Variable', monospace" }} />}
            {babysitState?.kind === "completed" && <aside aria-label={t("review:babysitExplanations")} data-testid="babysit-explanations" className="max-h-48 overflow-auto border-t bg-muted/20 px-3 py-2 text-xs">
              <h4 className="font-semibold">{t("review:babysitExplanations")}</h4>
              {(["before", "after"] as const).map((side) => <div key={side} className="mt-1"><span className="font-medium">{t(`review:babysit${side === "before" ? "Before" : "After"}`)}</span><ul className="ml-4 list-disc">{Object.entries(babysitState.explanations[side]).map(([line, explanation]) => <li key={`${side}-${line}`}><span className="font-mono">{line}</span>{`: ${explanation}`}</li>)}</ul></div>)}
            </aside>}
          </section>}
        </ResizablePanel>
      </ResizablePanelGroup>}
    </TabsContent>
  </Tabs>;
}
