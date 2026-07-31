import { useEffect, useMemo, useState, type CSSProperties, type MouseEvent, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import { RiArrowDownSLine, RiArrowLeftLine, RiArrowRightLine, RiFolderLine, RiFolderOpenLine } from "@remixicon/react";
import { BranchRelation, branchName } from "@/components/branch-relation";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import {
  Sidebar,
  SidebarContent,
  SidebarFooter,
  SidebarGroup,
  SidebarGroupContent,
  SidebarHeader,
  SidebarMenu,
  SidebarMenuButton,
  SidebarMenuItem,
  SidebarProvider,
  SidebarTrigger,
} from "@/components/ui/sidebar";
import { formatAbsoluteTime, formatRelativeTime, normalizeLocale } from "@/lib/i18n";
import type { ReviewSummary } from "@/lib/review-types";
import { cn } from "@/lib/utils";
import narrantLogo from "../../assets/brand/narrant-logo.svg";
import narrantLogoDark from "../../assets/brand/narrant-logo-dark.svg";

/** 首页侧边栏所需的持久化仓库投影。 */
export type HomeRepository = {
  id: number;
  canonicalPath: string;
  lastOpenedAt: number;
  available: boolean;
};

type SidebarPreferences = {
  open: boolean;
  width: number;
  recentGroupOpen: boolean;
  repositoriesGroupOpen: boolean;
  recentExpanded: boolean;
  expandedRepositories: number[];
  allRepositoryReviews: number[];
};

type Props = {
  reviews: ReviewSummary[];
  repositories?: HomeRepository[];
  error?: string;
  loading?: boolean;
  onOpenReview: (review: ReviewSummary) => void;
  onOpenRepository?: (repository: HomeRepository) => void;
  onOpenFolder: (path: string) => void;
  githubIdentityEntry?: ReactNode;
};

const preferenceKey = "narrant.sidebar-preferences";

/** 从路径中提取 Repository Entry 的稳定可见名称。 */
function repositoryName(path: string) {
  return path.split(/[\\/]/).filter(Boolean).at(-1) ?? path;
}

/** 安全读取主侧边栏偏好，损坏的本地数据会回退到默认值。 */
function readPreferences(): SidebarPreferences {
  const defaults: SidebarPreferences = {
    open: window.innerWidth >= 1200,
    width: 280,
    recentGroupOpen: true,
    repositoriesGroupOpen: true,
    recentExpanded: false,
    expandedRepositories: [],
    allRepositoryReviews: [],
  };
  try {
    return { ...defaults, ...JSON.parse(window.localStorage?.getItem(preferenceKey) ?? "{}") };
  } catch {
    return defaults;
  }
}

/** 在紧凑列表中以单行文本呈现 Branch Selection 的合入方向。 */
function CompactBranchRelation({ baseRef, headRef }: { baseRef: string; headRef: string }) {
  const base = branchName(baseRef);
  const head = branchName(headRef);
  return <span
    className="flex min-w-0 flex-1 items-center gap-1 font-mono text-xs"
    aria-label={`将 Head 分支 ${head} 合入 Base 分支 ${base}`}
  >
    <span className="truncate">{base}</span>
    <RiArrowLeftLine className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
    <span className="truncate">{head}</span>
  </span>;
}

/** 展示 Pull Request 导航行中被压缩掉的稳定辨认信息。 */
function PullRequestReviewDetails({
  pullRequest,
  repositoryPath,
  identity,
  openedAt,
}: {
  pullRequest: NonNullable<ReviewSummary["pullRequest"]>;
  repositoryPath: string;
  identity: string;
  openedAt: string;
}) {
  const { t } = useTranslation();
  const details = [
    [t("review:author"), pullRequest.author],
    [
      t("mergeDirection"),
      `${branchName(pullRequest.headRef)} → ${branchName(pullRequest.baseRef)}`,
    ],
    [t("catalog:repositoryPath"), repositoryPath],
    [t("lastOpened"), openedAt],
  ];
  return <div className="w-72 text-left">
    <p className="break-words font-medium">{identity}</p>
    <dl className="mt-2 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1.5">
      {details.map(([label, value]) => <div className="contents" key={label}>
        <dt className="text-background/65">{label}</dt>
        <dd className="min-w-0 break-words">{value}</dd>
      </div>)}
    </dl>
  </div>;
}

/** 展示本地 Branch Selection 的合入方向、仓库位置与最近打开时间。 */
function BranchReviewDetails({
  review,
  openedAt,
}: {
  review: ReviewSummary;
  openedAt: string;
}) {
  const { t } = useTranslation();
  const details = [
    [t("mergeDirection"), `${branchName(review.headRef)} → ${branchName(review.baseRef)}`],
    [t("catalog:repositoryPath"), review.repositoryPath],
    [t("lastOpened"), openedAt],
  ];
  return <dl className="grid w-72 grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1.5 text-left">
    {details.map(([label, value]) => <div className="contents" key={label}>
      <dt className="text-background/65">{label}</dt>
      <dd className="min-w-0 break-words">{value}</dd>
    </div>)}
  </dl>;
}

/** Review 导航条目按 Selection 类型呈现紧凑身份与最近打开时间。 */
function ReviewMenuItem({
  review,
  showRepository,
  onOpen,
}: {
  review: ReviewSummary;
  showRepository: boolean;
  onOpen: () => void;
}) {
  const { i18n } = useTranslation();
  const locale = normalizeLocale(i18n.resolvedLanguage);
  const time = formatRelativeTime(review.lastOpenedAt, Date.now(), locale);
  const pullRequest = review.pullRequest;
  const identity = pullRequest ? `#${pullRequest.number} ${pullRequest.title}` : undefined;
  const openedAt = formatAbsoluteTime(review.lastOpenedAt, locale);
  const tooltip = {
    hidden: false,
    side: "right" as const,
    align: "start" as const,
    sideOffset: 16,
    className: "block max-w-xs px-3 py-2.5 [&>[data-side]]:hidden",
    children: pullRequest && identity
      ? <PullRequestReviewDetails
          pullRequest={pullRequest}
          repositoryPath={review.repositoryPath}
          identity={identity}
          openedAt={openedAt}
        />
      : <BranchReviewDetails review={review} openedAt={openedAt} />,
  };
  return <SidebarMenuItem>
    <SidebarMenuButton
      className="h-auto min-w-0 rounded-lg px-3 py-2"
      tooltip={tooltip}
      onClick={onOpen}
    >
      <span className="flex min-w-0 flex-1 items-center gap-2 text-xs">
        {showRepository && <span className="max-w-24 shrink truncate font-medium">{repositoryName(review.repositoryPath)}</span>}
        {identity
          ? <span className="min-w-0 flex-1 truncate">{identity}</span>
          : <CompactBranchRelation baseRef={review.baseRef} headRef={review.headRef} />}
        <span className="mr-3 shrink-0 text-[11px] text-muted-foreground">{time}</span>
      </span>
    </SidebarMenuButton>
  </SidebarMenuItem>;
}

/** 最近审核首页，以及仓库和 Review 历史的常驻导航。 */
export function HomePage({
  reviews,
  repositories = [],
  error,
  loading = false,
  onOpenReview,
  onOpenRepository = () => undefined,
  onOpenFolder,
  githubIdentityEntry,
}: Props) {
  const { t } = useTranslation();
  const [preferences, setPreferences] = useState<SidebarPreferences>(readPreferences);
  const [resizingSidebar, setResizingSidebar] = useState(false);
  const openSidebar = preferences.open !== false;
  const sidebarWidth = typeof preferences.width === "number" ? preferences.width : 280;
  const sortedRepositories = useMemo(
    () => [...repositories].sort((left, right) => right.lastOpenedAt - left.lastOpenedAt),
    [repositories],
  );

  useEffect(() => {
    window.localStorage?.setItem(preferenceKey, JSON.stringify(preferences));
  }, [preferences]);

  /** 拖动侧边栏边界并将宽度限制在可用范围内。 */
  function resizeSidebar(event: MouseEvent<HTMLDivElement>) {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = sidebarWidth;
    setResizingSidebar(true);
    const resize = (move: globalThis.MouseEvent) => setPreferences((value) => ({
      ...value,
      width: Math.min(480, Math.max(220, startWidth + move.clientX - startX)),
    }));
    const stop = () => {
      setResizingSidebar(false);
      document.removeEventListener("mousemove", resize);
      document.removeEventListener("mouseup", stop);
    };
    document.addEventListener("mousemove", resize);
    document.addEventListener("mouseup", stop);
  }

  /** 使用系统目录选择器；E2E 构建只替换无法由 WebDriver 操作的系统窗口返回值。 */
  async function chooseFolder() {
    const e2ePath = (window as Window & { __NARRANT_E2E_DIRECTORY__?: string }).__NARRANT_E2E_DIRECTORY__;
    const path = import.meta.env.VITE_WDIO === "true" && e2ePath
      ? e2ePath
      : await open({ directory: true, multiple: false });
    if (typeof path === "string") onOpenFolder(path);
  }

  /** 独立切换一个仓库的 Review 历史，不触发仓库导航。 */
  function toggleRepository(id: number) {
    setPreferences((value) => ({
      ...value,
      expandedRepositories: value.expandedRepositories.includes(id)
        ? value.expandedRepositories.filter((candidate) => candidate !== id)
        : [...value.expandedRepositories, id],
    }));
  }

  /** 从最近审核进入时展开所属仓库，帮助用户建立导航位置感。 */
  function openRecentReview(review: ReviewSummary) {
    setPreferences((value) => ({
      ...value,
      expandedRepositories: value.expandedRepositories.includes(review.repositoryEntryId)
        ? value.expandedRepositories
        : [...value.expandedRepositories, review.repositoryEntryId],
    }));
    onOpenReview(review);
  }

  /** 在非交互顶栏上按下主鼠标键时，显式请求 Tauri 移动原生窗口。 */
  function startWindowDrag(event: MouseEvent<HTMLDivElement>) {
    if (
      event.button !== 0
      || event.target !== event.currentTarget
      || !("__TAURI_INTERNALS__" in window)
    ) return;
    event.preventDefault();
    void getCurrentWindow().startDragging();
  }

  return <SidebarProvider
    open={openSidebar}
    onOpenChange={(nextOpen) => setPreferences((value) => ({ ...value, open: nextOpen }))}
    data-resizing={resizingSidebar || undefined}
    className={cn(
      resizingSidebar && "select-none [&_[data-slot=sidebar-container]]:transition-none [&_[data-slot=sidebar-gap]]:transition-none",
    )}
    style={{ "--sidebar-width": `${sidebarWidth}px` } as CSSProperties}
  >
    <div
      data-slot="window-titlebar"
      className="fixed inset-x-0 top-0 z-30 h-12"
      onMouseDown={startWindowDrag}
    >
      <div
        aria-hidden
        className={cn(
          "pointer-events-none absolute inset-y-0 left-0 bg-sidebar transition-[width] duration-200 ease-linear",
          openSidebar && "border-r border-sidebar-border",
          resizingSidebar && "transition-none",
        )}
        style={{ width: openSidebar ? `${sidebarWidth}px` : 0 }}
      />
      <SidebarTrigger
        aria-label={openSidebar ? t("collapse") : t("expand")}
        className="absolute left-3 top-2 z-10 bg-transparent hover:bg-sidebar-accent supports-[padding:max(0px)]:left-[max(0.75rem,env(titlebar-area-x,0px)+5.5rem)]"
      />
    </div>
    <Sidebar aria-label={t("sidebar")} collapsible="offcanvas" className="pt-12">
      <SidebarHeader className="flex-row items-center gap-3 px-4 py-3">
        <picture className="size-12 shrink-0">
          <source media="(prefers-color-scheme: dark)" srcSet={narrantLogoDark} />
          <img className="size-12" src={narrantLogo} alt={t("brand")} />
        </picture>
        <div className="min-w-0">
          <p className="font-heading text-lg">{t("brand")}</p>
          <p className="truncate text-xs text-muted-foreground">{t("subtitle")}</p>
        </div>
      </SidebarHeader>
      <SidebarContent className="gap-0">
        <SidebarGroup className="py-1">
          <button
            type="button"
            className="group/section-label flex h-9 w-full items-center rounded-md px-3 text-left text-sm font-medium text-sidebar-foreground/80 outline-none hover:text-sidebar-foreground focus-visible:ring-2 focus-visible:ring-sidebar-ring"
            aria-expanded={preferences.recentGroupOpen}
            onClick={() => setPreferences((value) => ({ ...value, recentGroupOpen: !value.recentGroupOpen }))}
          >
            <span>{t("recent")}</span>
            <RiArrowDownSLine className={cn(
              "ml-1 size-4 opacity-0 transition-[transform,opacity] group-hover/section-label:opacity-100 group-focus-visible/section-label:opacity-100",
              !preferences.recentGroupOpen && "-rotate-90",
            )} />
          </button>
          {preferences.recentGroupOpen && <SidebarGroupContent>
            <SidebarMenu>
              {reviews.slice(0, preferences.recentExpanded ? 10 : 5).map((review) =>
                <ReviewMenuItem key={review.reviewId} review={review} showRepository onOpen={() => openRecentReview(review)} />)}
              {!reviews.length && <p className="px-2 text-sm text-muted-foreground">{t("noRecent")}</p>}
              {reviews.length > 5 && <SidebarMenuItem>
                <SidebarMenuButton
                  className="text-xs text-muted-foreground hover:bg-transparent active:bg-transparent"
                  onClick={() => setPreferences((value) => ({ ...value, recentExpanded: !value.recentExpanded }))}
                >
                  {preferences.recentExpanded ? t("showLess") : t("viewAll")}
                </SidebarMenuButton>
              </SidebarMenuItem>}
            </SidebarMenu>
          </SidebarGroupContent>}
        </SidebarGroup>
        <SidebarGroup className="py-1">
          <button
            type="button"
            className="group/section-label flex h-9 w-full items-center rounded-md px-3 text-left text-sm font-medium text-sidebar-foreground/80 outline-none hover:text-sidebar-foreground focus-visible:ring-2 focus-visible:ring-sidebar-ring"
            aria-expanded={preferences.repositoriesGroupOpen}
            onClick={() => setPreferences((value) => ({ ...value, repositoriesGroupOpen: !value.repositoriesGroupOpen }))}
          >
            <span>{t("repositories")}</span>
            <RiArrowDownSLine className={cn(
              "ml-1 size-4 opacity-0 transition-[transform,opacity] group-hover/section-label:opacity-100 group-focus-visible/section-label:opacity-100",
              !preferences.repositoriesGroupOpen && "-rotate-90",
            )} />
          </button>
          {preferences.repositoriesGroupOpen && <SidebarGroupContent>
            <SidebarMenu>
              {sortedRepositories.map((repository) => {
                const expanded = preferences.expandedRepositories.includes(repository.id);
                const all = preferences.allRepositoryReviews.includes(repository.id);
                const repositoryReviews = reviews.filter((review) => review.repositoryEntryId === repository.id);
                return <SidebarMenuItem key={repository.id}>
                  <div className="relative flex items-center">
                    <SidebarMenuButton
                      className={cn("min-w-0 flex-1 pr-9", !repository.available && "text-muted-foreground")}
                      tooltip={repository.canonicalPath}
                      aria-expanded={expanded}
                      onClick={() => toggleRepository(repository.id)}
                    >
                      {expanded ? <RiFolderOpenLine /> : <RiFolderLine />}
                      <span className="truncate">{repositoryName(repository.canonicalPath)}</span>
                    </SidebarMenuButton>
                    <Button
                      variant="outline"
                      size="icon-xs"
                      className="absolute inset-y-0 right-1 z-10 my-auto rounded-md border-sidebar-border bg-sidebar shadow-xs hover:bg-sidebar-accent active:translate-y-0!"
                      aria-label={t("openRepository", { name: repositoryName(repository.canonicalPath) })}
                      onClick={() => onOpenRepository(repository)}
                    >
                      <RiArrowRightLine />
                    </Button>
                  </div>
                  {expanded && <SidebarMenu className="ml-5 mt-1 w-auto pl-2">
                    {repositoryReviews.slice(0, all ? undefined : 10).map((review) =>
                      <ReviewMenuItem key={review.reviewId} review={review} showRepository={false} onOpen={() => onOpenReview(review)} />)}
                    {!repositoryReviews.length && <p className="px-2 py-1 text-xs text-muted-foreground">{t("noRecent")}</p>}
                    {repositoryReviews.length > 10 && <SidebarMenuItem>
                      <SidebarMenuButton
                        className="text-xs text-muted-foreground hover:bg-transparent active:bg-transparent"
                        onClick={() => setPreferences((value) => ({
                          ...value,
                          allRepositoryReviews: all
                            ? value.allRepositoryReviews.filter((id) => id !== repository.id)
                            : [...value.allRepositoryReviews, repository.id],
                        }))}
                      >
                        {all ? t("showLess") : t("viewAll")}
                      </SidebarMenuButton>
                    </SidebarMenuItem>}
                  </SidebarMenu>}
                </SidebarMenuItem>;
              })}
            </SidebarMenu>
          </SidebarGroupContent>}
        </SidebarGroup>
      </SidebarContent>
      {githubIdentityEntry && <SidebarFooter className="border-t p-3">{githubIdentityEntry}</SidebarFooter>}
      <div aria-label={t("resizeSidebar")} className="absolute right-0 top-12 h-[calc(100%-3rem)] w-1 cursor-col-resize" onMouseDown={resizeSidebar} />
    </Sidebar>
    <main className="min-h-screen w-full bg-background px-8 pb-8 pt-14">
      {!reviews.length && <header className="flex items-center gap-3 border-b pb-5">
        <div><h1 className="font-heading text-xl">{t("brand")}</h1><p className="text-sm text-muted-foreground">{t("subtitle")}</p></div>
      </header>}
      {error && <p role="alert" className="mx-auto mt-6 max-w-3xl rounded-lg border border-destructive/30 bg-destructive/5 p-4 text-sm text-destructive">{error}</p>}
      {loading
        ? <section role="status" aria-label={t("loadingRecent")} className="mx-auto max-w-3xl space-y-4 py-10"><Skeleton className="h-8 w-40" /><Skeleton className="h-16 w-full" /><Skeleton className="h-16 w-full" /></section>
        : reviews.length
          ? <section className="mx-auto max-w-3xl py-10" aria-label={t("recent")}>
            <div className="flex items-center justify-between gap-4">
              <h2 className="font-heading text-2xl">{t("recent")}</h2>
              <Button onClick={() => void chooseFolder()}><RiFolderOpenLine />{t("openFolder")}</Button>
            </div>
            <ul className="mt-5 divide-y rounded-lg border">
              {reviews.slice(0, 10).map((review) => <li key={review.reviewId}>
                <Button variant="ghost" className="h-auto w-full justify-start rounded-none px-4 py-4" onClick={() => openRecentReview(review)}>
                  <span className="flex min-w-0 flex-col items-start gap-2">
                    <span className="flex gap-3">
                      <span className="font-medium">{repositoryName(review.repositoryPath)}</span>
                      <span className="truncate">{review.pullRequest ? `#${review.pullRequest.number} ${review.pullRequest.title}` : t("localBranch")}</span>
                    </span>
                    <BranchRelation baseRef={review.baseRef} headRef={review.headRef} />
                  </span>
                </Button>
              </li>)}
            </ul>
          </section>
          : <section className="mx-auto flex max-w-xl flex-col items-start gap-5 py-24"><h2 className="font-heading text-3xl">{t("emptyTitle")}</h2><p className="text-muted-foreground">{t("emptyDescription")}</p><Button size="lg" onClick={() => void chooseFolder()}><RiFolderOpenLine />{t("openFolder")}</Button></section>}
    </main>
  </SidebarProvider>;
}
