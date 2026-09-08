import { useEffect, useRef, type UIEvent } from "react";
import {
  ArrowDown01Icon,
  FileAddIcon,
  FileEditIcon,
  FileEmpty02Icon,
  FileRemoveIcon,
  FileSyncIcon,
  LayoutTwoColumnIcon,
  Logout02Icon,
  RefreshIcon,
} from "@hugeicons/core-free-icons";
import { useTranslation } from "react-i18next";

import { ReviewStructureView } from "@/components/code-structure/review-structure-view";
import { FileChangeLoading } from "@/components/file-change-loading";
import { BabysitDiffEditor } from "@/components/babysit-diff-editor";
import { FileTypeIcon } from "@/components/file-type-icon";
import { ReviewOverview } from "@/components/review-overview";
import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useBabysitReviewPresentation } from "@/components/use-babysit-review-presentation";
import { useReviewUpdatePresentation } from "@/components/use-review-update-presentation";
import { cn } from "@/lib/utils";
import type { ReviewReaderBinding } from "@/review/use-review-lifecycle";
import type { ChangeStatus, FileChangeSummary, ReviewDiffMode, ReviewReaderScrollPositions, ReviewView } from "@/review/types";
export type { ChangeStatus, FileChangeContent, FileChangeSummary, ReviewDiffMode as DiffMode, ReviewSnapshot as ReviewReaderSnapshot } from "@/review/types";

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
    narrow: boolean;
    editorTheme: string;
    onOpenExternal: (url: string) => void;
    onOpenApplicationSettings?: () => void;
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
    js: "javascript", jsx: "javascript", mjs: "javascript", ts: "typescript", tsx: "typescript", mts: "typescript",
    md: "markdown", json: "json", css: "css", html: "html", rs: "rust",
    py: "python", go: "go", java: "java", yml: "yaml", yaml: "yaml",
    c: "c", cpp: "cpp", h: "cpp", cs: "csharp", sh: "shell",
    sql: "sql", php: "php", rb: "ruby", kt: "kotlin", xml: "xml",
  };
  return extension ? languages[extension] ?? "plaintext" : "plaintext";
}

/** 呈现满高、自由浏览且无审核进度语义的 Review Reader。 */
export function ReviewReader(input: ReviewReaderProps) {
  const { t } = useTranslation(["common", "review", "structure"]);
  const { review, appearance } = input;
  const updatePresentation = useReviewUpdatePresentation(review);
  const props = {
    snapshot: review.snapshot,
    fileChange: review.fileChange.kind === "ready" ? review.fileChange.content : undefined,
    pendingFilePath: review.fileChange.kind === "loading" ? review.fileChange.path : undefined,
    fileChangeLoading: review.fileChange.kind === "loading",
    fileChangeError: review.fileChange.kind === "failed" ? review.fileChange.failure.detail : undefined,
    firstFileChangePath: review.fileChange.kind === "failed" ? review.fileChange.path : undefined,
    navigationBusy: review.transition.kind !== "stable",
    ...appearance,
    updateAction: <Tooltip>
      <TooltipTrigger render={<span className="inline-flex" />}>
        <Button variant="outline" size="icon-sm" className="rounded-full" aria-label={updatePresentation.label}
          disabled={updatePresentation.busy} aria-busy={updatePresentation.busy || undefined} onClick={updatePresentation.run}>
          <Icon icon={RefreshIcon} className={cn("-translate-y-px", updatePresentation.busy && "animate-spin")} aria-hidden />
        </Button>
      </TooltipTrigger>
      <TooltipContent>{updatePresentation.label}</TooltipContent>
    </Tooltip>,
    notice: undefined,
    onSelectFile: review.selectFileChange,
    onRetryFile: review.retryFileChange,
    onClose: () => void review.close(),
  };
  const activeView = review.view;
  const collapsedDirectories = new Set(review.collapsedDirectories);
  const selectedPath = props.pendingFilePath ?? props.fileChange?.path;
  const effectiveDiffMode: ReviewDiffMode = props.narrow ? "inline" : review.diffMode;
  const fileTypeIconTheme = props.editorTheme === "vs-dark" ? "dark" : "light";
  const overviewScrollRef = useRef<HTMLDivElement>(null);
  const fileTreeScrollRef = useRef<HTMLElement>(null);
  const contentScrollRef = useRef<HTMLDivElement>(null);
  const diffReviewId = review.snapshot.reviewId;
  const diffSnapshotId = review.snapshot.snapshotId;
  const babysitPresentation = useBabysitReviewPresentation({
    babysit: review.babysit,
    reviewId: diffReviewId,
    snapshotId: diffSnapshotId,
    selectedPath,
    onOpenApplicationSettings: appearance.onOpenApplicationSettings,
  });
  /** 当 Review 或 peer view 再次呈现时，把 lifecycle 保存的位置写回真实 DOM。 */
  useEffect(() => {
    if (overviewScrollRef.current) overviewScrollRef.current.scrollTop = review.scrollPositions.overview;
    if (fileTreeScrollRef.current) fileTreeScrollRef.current.scrollTop = review.scrollPositions.fileTree;
    if (contentScrollRef.current) contentScrollRef.current.scrollTop = review.scrollPositions.content;
  }, [activeView, review.scrollPositions, review.snapshot.reviewId]);
  /** 将真实滚动事件投影到当前 Review。 */
  const saveScroll = (region: keyof ReviewReaderScrollPositions) => (event: UIEvent<HTMLElement>) => {
    review.setScrollPosition(region, event.currentTarget.scrollTop);
  };

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
            onClick={() => review.toggleDirectory(path)}>
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
          className={cn("h-7 w-full justify-start gap-1.5 rounded-md px-1.5 text-xs font-normal", selectedPath === change.path && "bg-primary/15 text-foreground shadow-[inset_2px_0_0_var(--primary)] hover:bg-primary/20 dark:bg-primary/25 dark:hover:bg-primary/30")}
          onClick={() => props.onSelectFile(change.path)}>
          <FileTypeIcon path={change.path} kind="file" theme={fileTypeIconTheme} testId={`file-type-icon-${change.path.split("/").at(-1)}`} />
          <span className="min-w-0 flex-1 truncate text-left">{change.oldPath ? `${change.oldPath} → ${change.path}` : change.path.split("/").at(-1)}</span>
          {renderChangeStatus(change.status)}
        </Button>
      </li>)}
    </>;
  }

  return <Tabs value={activeView} onValueChange={(value) => review.setView(value as ReviewView)} aria-label={t("review:reader")} className="h-full min-h-0 gap-0 overflow-hidden rounded-lg border bg-background">
    <div role="toolbar" aria-label={t("review:toolbar")} className="flex min-h-12 shrink-0 flex-wrap items-center gap-2 border-b px-3">
      <TabsList variant="line" aria-label={t("review:reviewViews")} className="!h-12 min-w-0 shrink-0 p-0">
        <TabsTrigger value="overview" className="h-full px-3 after:!bottom-[-1px] after:!h-px">{t("review:overview")}</TabsTrigger>
        <TabsTrigger value="file-changes" className="h-full px-3 after:!bottom-[-1px] after:!h-px">{t("review:fileChanges", { count: props.snapshot.fileChanges.length })}</TabsTrigger>
        {review.structure && <TabsTrigger value="code-structure" className="h-full px-3 after:!bottom-[-1px] after:!h-px">{t("structure:title")}</TabsTrigger>}
      </TabsList>
      <div className="ml-auto flex min-w-0 shrink-0 items-center justify-end gap-2">
        {props.updateAction}
        {(activeView === "file-changes" || (activeView === "code-structure" && review.structure?.peek)) && <Tooltip>
          <TooltipTrigger render={<span className="inline-flex" />}>
            <Button variant={effectiveDiffMode === "side-by-side" ? "default" : "outline"} size="icon-sm" className="rounded-full" aria-label={t("review:sideBySide")}
              aria-pressed={effectiveDiffMode === "side-by-side"} disabled={props.narrow}
              onClick={() => review.setDiffMode(effectiveDiffMode === "side-by-side" ? "inline" : "side-by-side")}>
              <Icon icon={LayoutTwoColumnIcon} className="-translate-y-px" aria-hidden />
            </Button>
          </TooltipTrigger>
          <TooltipContent>{props.narrow ? t("review:narrowInlineOnly") : effectiveDiffMode === "side-by-side" ? t("review:switchToInline") : t("review:switchToSideBySide")}</TooltipContent>
        </Tooltip>}
        {activeView === "file-changes" && props.fileChange && !props.fileChangeLoading && babysitPresentation.toolbarActions}
        <Tooltip>
          <TooltipTrigger render={<Button variant="outline" size="icon-sm" className="rounded-full" aria-label={t("review:exit")} onClick={props.onClose} />}>
            <Icon icon={Logout02Icon} className="-translate-y-px" aria-hidden />
          </TooltipTrigger>
          <TooltipContent>{t("review:exit")}</TooltipContent>
        </Tooltip>
      </div>
    </div>
    {props.notice && <div className="shrink-0 border-b px-3 py-2 text-xs">{props.notice}</div>}
    <TabsContent value="overview" className="min-h-0 overflow-hidden"><div ref={overviewScrollRef} onScroll={saveScroll("overview")} data-review-scroll-region="overview" className="h-full overflow-y-auto"><ReviewOverview review={props.snapshot} onOpenExternal={props.onOpenExternal} /></div></TabsContent>
    <TabsContent value="file-changes" className="flex min-h-0 flex-col overflow-hidden">
    {props.snapshot.fileChanges.length === 0
      ? <div data-empty-state className="flex min-h-0 flex-1 flex-col items-center px-6 pt-14 text-center sm:pt-20">
        <div className="mb-4 flex size-12 items-center justify-center rounded-full bg-muted text-muted-foreground">
          <Icon icon={FileEmpty02Icon} className="size-6" aria-hidden />
        </div>
        <h3 className="font-medium">{t("review:emptyTitle")}</h3>
        <p className="mt-1 text-sm text-muted-foreground">{t("review:emptyDescription")}</p>
      </div>
      : <ResizablePanelGroup orientation="horizontal" className="h-auto! min-h-0 flex-1"
        defaultLayout={{ "review-file-tree": review.treeWidth, "review-diff": 100 - review.treeWidth }}
        onLayoutChanged={(layout, meta) => { if (meta.isUserInteraction) review.setTreeWidth(layout["review-file-tree"]); }}>
        <ResizablePanel id="review-file-tree" minSize="15%" maxSize="45%" defaultSize={`${review.treeWidth}%`}>
          <nav aria-label={t("review:fileTree")} ref={fileTreeScrollRef} onScroll={saveScroll("fileTree")} data-review-scroll-region="fileTree" className="h-full overflow-auto border-r bg-muted/20">
            <div className="sticky top-0 z-10 border-b bg-background/95 px-2.5 py-2 backdrop-blur"><h3 className="text-xs font-semibold">{t("review:changeCount", { count: props.snapshot.fileChanges.length })}</h3></div>
            <ul aria-label={t("review:fileTree")} className="p-1">{renderDirectory(buildDirectoryTree(props.snapshot.fileChanges))}</ul>
          </nav>
        </ResizablePanel>
        <ResizableHandle withHandle aria-label={t("review:resizeFileTree")} />
        <ResizablePanel id="review-diff" minSize="45%"><div ref={contentScrollRef} onScroll={saveScroll("content")} data-review-scroll-region="content" className="flex h-full min-h-0 flex-col overflow-auto">
          {props.fileChangeLoading && <FileChangeLoading label={t("review:loadingFileChange")} />}
          {props.fileChangeError && <div role="alert" className="grid min-h-72 place-content-center gap-3 px-6 text-center"><h3>{t("review:fileChangeError")}</h3><p className="text-sm text-muted-foreground">{props.fileChangeError}</p>{props.firstFileChangePath && <Button variant="outline" onClick={props.onRetryFile}>{t("common:retry")}</Button>}</div>}
          {props.fileChange && !props.fileChangeLoading && <section className="flex h-full min-w-0 flex-col" aria-label={t("review:fileChange")} data-before={import.meta.env.VITE_WDIO ? props.fileChange.before ?? "" : undefined} data-after={import.meta.env.VITE_WDIO ? props.fileChange.after ?? "" : undefined}>
            <header className="flex min-h-9 flex-wrap items-center gap-2 border-b px-3 py-1"><h3 className="flex min-w-0 flex-1 items-center gap-2 truncate font-mono text-sm font-medium"><FileTypeIcon path={props.fileChange.path} kind="file" theme={fileTypeIconTheme} />{props.fileChange.path} {renderChangeStatus(props.fileChange.status)}</h3></header>
            {props.fileChange.contentKind === "binary" && <div className="grid min-h-72 place-content-center px-6 text-center"><p>{t("review:binary", { before: props.fileChange.beforeSize === null ? t("review:none") : t("review:bytes", { count: props.fileChange.beforeSize }), after: props.fileChange.afterSize === null ? t("review:none") : t("review:bytes", { count: props.fileChange.afterSize }) })}</p></div>}
            {props.fileChange.contentKind === "submodule" && <div className="grid min-h-72 place-content-center px-6 text-center"><h4 className="font-medium">{t("review:submodule")}</h4><p className="font-mono text-sm text-muted-foreground">{props.fileChange.before ?? t("review:none")} → {props.fileChange.after ?? t("review:none")}</p></div>}
            {props.fileChange.contentKind === "symlink" && <div className="grid min-h-72 place-content-center px-6 text-center"><h4 className="font-medium">{t("review:symlink")}</h4><p className="font-mono text-sm text-muted-foreground">{props.fileChange.before ?? t("review:none")} → {props.fileChange.after ?? t("review:none")}</p></div>}
            {props.fileChange.contentKind === "text" && <div className="min-h-0 flex-1" data-testid="babysit-diff-editor"><BabysitDiffEditor key={`${diffReviewId}:${diffSnapshotId}:${props.fileChange.path}`} identity={{ reviewId: diffReviewId, snapshotId: diffSnapshotId, path: props.fileChange.path }} viewState={review.fileViewState} onSaveViewState={review.setFileViewState} theme={props.editorTheme} height="100%" language={editorLanguage(props.fileChange.path)} original={props.fileChange.before ?? ""} modified={props.fileChange.after ?? ""} renderSideBySide={effectiveDiffMode === "side-by-side"} explanations={babysitPresentation.explanations} onDiffChange={babysitPresentation.onDiffChange} /></div>}
            {babysitPresentation.footer}
          </section>}
        </div></ResizablePanel>
      </ResizablePanelGroup>}
    </TabsContent>
    {review.structure && <TabsContent value="code-structure" className="flex min-h-0 flex-col overflow-hidden">
      <ReviewStructureView structure={review.structure} narrow={props.narrow} diffMode={effectiveDiffMode} editorTheme={props.editorTheme}
        onOpenFileChange={(path) => {
          if (!review.snapshot.fileChanges.some((change) => change.path === path)) return;
          review.selectFileChange(path);
          review.setView("file-changes");
        }} />
    </TabsContent>}
  </Tabs>;
}
