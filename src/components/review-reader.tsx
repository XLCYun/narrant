import { useState, type ReactNode } from "react";
import { DiffEditor } from "@monaco-editor/react";
import {
  ArrowDown01Icon,
  FileAddIcon,
  FileEditIcon,
  FileRemoveIcon,
  FileSyncIcon,
  GitMergeIcon,
  GitPullRequestClosedIcon,
  GitPullRequestDraftIcon,
  GitPullRequestIcon,
  LayoutTwoColumnIcon,
  Logout02Icon,
  ViewAgendaIcon,
} from "@hugeicons/core-free-icons";
import { useTranslation } from "react-i18next";

import { BranchRelation } from "@/components/branch-relation";
import { FileChangeLoading } from "@/components/file-change-loading";
import { ReviewOverview } from "@/components/review-overview";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from "@/components/ui/resizable";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import type { PullRequestMetadata } from "@/lib/review-types";

/** File Change 在树和 Diff 标题中共享的状态。 */
export type ChangeStatus = "added" | "modified" | "deleted" | "renamed";

/** Review Reader 文件树所需的轻量 File Change 投影。 */
export type FileChangeSummary = {
  path: string;
  oldPath: string | null;
  status: ChangeStatus;
};

/** 当前选中文件的可展示内容。 */
export type FileChangeContent = FileChangeSummary & {
  contentKind: "text" | "binary" | "submodule" | "symlink";
  beforeSize: number | null;
  afterSize: number | null;
  before: string | null;
  after: string | null;
};

/** Review Reader 当前呈现的不可变 Snapshot。 */
export type ReviewReaderSnapshot = {
  reviewId: string;
  snapshotId: string;
  baseRef: string;
  baseRevision: string;
  comparisonBase: string;
  headRef: string;
  headRevision: string;
  fileChanges: FileChangeSummary[];
  pullRequest?: PullRequestMetadata;
};

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
  snapshot: ReviewReaderSnapshot;
  fileChange?: FileChangeContent;
  pendingFilePath?: string;
  fileChangeLoading?: boolean;
  fileChangeError?: string;
  firstFileChangePath?: string;
  navigationBusy?: boolean;
  diffMode: DiffMode;
  narrow: boolean;
  editorTheme: string;
  initialTreeWidth: number;
  updateAction?: ReactNode;
  notice?: ReactNode;
  onSelectFile: (path: string) => void;
  onRetryFile: (path: string) => void;
  onDiffModeChange: (mode: DiffMode) => void;
  onTreeWidthChange: (width: number) => void;
  onOpenExternal: (url: string) => void;
  onClose: () => void;
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
  };
  return extension ? languages[extension] ?? "plaintext" : "plaintext";
}

/** 呈现满高、自由浏览且无审核进度语义的 Review Reader。 */
export function ReviewReader(props: ReviewReaderProps) {
  const { t } = useTranslation(["common", "review"]);
  const [collapsedDirectories, setCollapsedDirectories] = useState<Set<string>>(() => new Set());
  const selectedPath = props.pendingFilePath ?? props.fileChange?.path;
  const effectiveDiffMode: DiffMode = props.narrow ? "inline" : props.diffMode;

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

  /** 使用统一 Badge 呈现 Pull Request 的协作状态。 */
  function renderPullRequestStatus() {
    const pullRequest = props.snapshot.pullRequest;
    if (!pullRequest) return null;
    const state: "draft" | PullRequestMetadata["state"] = pullRequest.draft ? "draft" : pullRequest.state;
    const icons = {
      draft: GitPullRequestDraftIcon,
      open: GitPullRequestIcon,
      closed: GitPullRequestClosedIcon,
      merged: GitMergeIcon,
    };
    const colors = {
      draft: "bg-amber-500/10 text-amber-700 dark:text-amber-300",
      open: "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
      closed: "bg-muted text-muted-foreground",
      merged: "bg-violet-500/10 text-violet-700 dark:text-violet-300",
    };
    return <Badge variant="secondary" className={cn("border-0", colors[state])}>
      <Icon icon={icons[state]} data-icon="inline-start" aria-hidden />
      {pullRequest.draft ? t("review:draft") : t(`review:state.${pullRequest.state}`)}
    </Badge>;
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
          <span className="min-w-0 flex-1 truncate text-left">{change.oldPath ? `${change.oldPath} → ${change.path}` : change.path.split("/").at(-1)}</span>
          {renderChangeStatus(change.status)}
        </Button>
      </li>)}
    </>;
  }

  return <section aria-label={t("review:reader")} className="flex h-full min-h-0 flex-col overflow-hidden rounded-lg border bg-background">
    <header className="flex min-h-14 shrink-0 flex-wrap items-center justify-between gap-3 border-b px-3 py-2">
      <div className="min-w-0 flex-1">
        <div className="min-w-0">
          <div className="flex min-w-0 items-center gap-2">
            <h2 className="truncate text-sm font-semibold">{props.snapshot.pullRequest ? `#${props.snapshot.pullRequest.number} ${props.snapshot.pullRequest.title}` : t("review:localBranchReview")}</h2>
            {renderPullRequestStatus()}
          </div>
          <BranchRelation baseRef={props.snapshot.baseRef} headRef={props.snapshot.headRef} className="mt-1.5 flex-nowrap [&_[data-slot=badge]]:truncate [&_[data-slot=badge]]:whitespace-nowrap" />
        </div>
      </div>
      <div className="flex shrink-0 flex-wrap items-center justify-end gap-2">
        {props.updateAction}
        <ReviewOverview review={props.snapshot} onOpenExternal={props.onOpenExternal} />
        <Tooltip>
          <TooltipTrigger render={<span className="inline-flex" />}>
            <Button variant="outline" aria-label={effectiveDiffMode === "side-by-side" ? t("review:switchToInline") : t("review:switchToSideBySide")}
              aria-pressed={effectiveDiffMode === "side-by-side"} disabled={props.narrow}
              onClick={() => props.onDiffModeChange(effectiveDiffMode === "side-by-side" ? "inline" : "side-by-side")}>
              <Icon icon={effectiveDiffMode === "side-by-side" ? LayoutTwoColumnIcon : ViewAgendaIcon} data-icon="inline-start" aria-hidden />
              {effectiveDiffMode === "side-by-side" ? t("review:sideBySide") : t("review:inline")}
            </Button>
          </TooltipTrigger>
          <TooltipContent>{props.narrow ? t("review:narrowInlineOnly") : effectiveDiffMode === "side-by-side" ? t("review:switchToInline") : t("review:switchToSideBySide")}</TooltipContent>
        </Tooltip>
        <Button variant="outline" onClick={props.onClose}>
          <Icon icon={Logout02Icon} data-icon="inline-start" aria-hidden />
          {t("review:exit")}
        </Button>
      </div>
    </header>
    {props.notice && <div className="shrink-0 border-b px-3 py-2 text-xs">{props.notice}</div>}
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
          {props.fileChangeError && <div role="alert" className="grid min-h-72 place-content-center gap-3 px-6 text-center"><h3>{t("review:fileChangeError")}</h3><p className="text-sm text-muted-foreground">{props.fileChangeError}</p>{props.firstFileChangePath && <Button variant="outline" onClick={() => props.onRetryFile(props.firstFileChangePath!)}>{t("common:retry")}</Button>}</div>}
          {props.fileChange && !props.fileChangeLoading && <section className="h-full min-w-0" aria-label={t("review:fileChange")} data-before={import.meta.env.VITE_WDIO ? props.fileChange.before ?? "" : undefined} data-after={import.meta.env.VITE_WDIO ? props.fileChange.after ?? "" : undefined}>
            <header className="flex h-9 items-center border-b px-3"><h3 className="flex items-center gap-2 truncate font-mono text-sm font-medium">{props.fileChange.path} {renderChangeStatus(props.fileChange.status)}</h3></header>
            {props.fileChange.contentKind === "binary" && <div className="grid min-h-72 place-content-center px-6 text-center"><p>{t("review:binary", { before: props.fileChange.beforeSize === null ? t("review:none") : t("review:bytes", { count: props.fileChange.beforeSize }), after: props.fileChange.afterSize === null ? t("review:none") : t("review:bytes", { count: props.fileChange.afterSize }) })}</p></div>}
            {props.fileChange.contentKind === "submodule" && <div className="grid min-h-72 place-content-center px-6 text-center"><h4 className="font-medium">{t("review:submodule")}</h4><p className="font-mono text-sm text-muted-foreground">{props.fileChange.before ?? t("review:none")} → {props.fileChange.after ?? t("review:none")}</p></div>}
            {props.fileChange.contentKind === "symlink" && <div className="grid min-h-72 place-content-center px-6 text-center"><h4 className="font-medium">{t("review:symlink")}</h4><p className="font-mono text-sm text-muted-foreground">{props.fileChange.before ?? t("review:none")} → {props.fileChange.after ?? t("review:none")}</p></div>}
            {props.fileChange.contentKind === "text" && <DiffEditor theme={props.editorTheme} height="calc(100% - 2.25rem)" language={editorLanguage(props.fileChange.path)} original={props.fileChange.before ?? ""} modified={props.fileChange.after ?? ""} options={{ readOnly: true, renderSideBySide: effectiveDiffMode === "side-by-side", ignoreTrimWhitespace: false, fontFamily: "'JetBrains Mono Variable', monospace" }} />}
          </section>}
        </ResizablePanel>
      </ResizablePanelGroup>}
  </section>;
}
