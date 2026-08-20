import { useEffect, useRef, useState, type CSSProperties, type MouseEvent, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useTranslation } from "react-i18next";
import { ArrowDown01Icon, ArrowRight02Icon, Folder01Icon, FolderOpenIcon } from "@hugeicons/core-free-icons";
import { BranchRelation, branchName } from "@/components/branch-relation";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
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
import type { RecentReviewSummary } from "@/lib/review-types";
import { cn } from "@/lib/utils";
import { Icon } from "@/components/ui/icon";
import narrantLogo from "../../assets/brand/narrant-logo.svg";
import narrantLogoDark from "../../assets/brand/narrant-logo-dark.svg";
import type { AppNavigationResult, AppShellNavigationBinding } from "@/app/use-app-shell-navigation";
import type { RepositorySummary } from "@/repository/repository-application";
import { tauriTransport } from "@/infrastructure/tauri/transport";

/** 首页侧边栏所需的持久化仓库投影。 */
export type HomeRepository = RepositorySummary;

type SidebarPreferences = {
  open: boolean;
  width: number;
  recentGroupOpen: boolean;
  repositoriesGroupOpen: boolean;
  recentExpanded: boolean;
  expandedRepositories: number[];
  allRepositoryReviews: number[];
};

/** 独立 HomePage 展示入口的兼容输入。 */
type HomePageProps = {
  reviews: RecentReviewSummary[];
  repositories?: HomeRepository[];
  error?: string;
  loading?: boolean;
  onOpenReview: (review: RecentReviewSummary) => void;
  onOpenRepository?: (repository: HomeRepository) => void;
  onOpenFolder: (path: string) => void;
  githubIdentityEntry?: ReactNode;
  children?: ReactNode;
  showHome?: boolean;
  onOpenHome?: () => void;
  enabled?: boolean;
  fallbackClassName?: string;
  activeRepositoryId?: number;
  activeReviewId?: string;
  pendingRepositoryId?: number;
  pendingReviewId?: string;
  reviewNavigationBusy?: boolean;
  workspace?: boolean;
};

type AppShellNavigation = Pick<AppShellNavigationBinding, "destination" | "navigate" | "registerAndOpen"> & {
  sidebar: Omit<AppShellNavigationBinding["sidebar"], "repositories"> & { repositories: HomeRepository[] };
};

type AppShellProps = {
  navigation: AppShellNavigation;
  githubIdentityEntry?: ReactNode;
  children?: ReactNode;
  enabled?: boolean;
  fallbackClassName?: string;
};

const preferenceKey = "narrant.sidebar-preferences";

/** 从路径中提取 Repository 的稳定可见名称。 */
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
    <span className="min-w-0 shrink truncate">{head}</span>
    <Icon icon={ArrowRight02Icon} className="size-3.5 shrink-0 text-muted-foreground" aria-hidden />
    <span className="min-w-0 shrink-[10] truncate">{base}</span>
  </span>;
}

/** 展示 Pull Request 导航行中被压缩掉的稳定辨认信息。 */
function PullRequestReviewDetails({
  pullRequest,
  repositoryPath,
  identity,
  openedAt,
}: {
  pullRequest: Extract<RecentReviewSummary["review"]["selection"], { kind: "pull-request" }>["pullRequest"];
  repositoryPath: string;
  identity: string;
  openedAt: string;
}) {
  const { t } = useTranslation();
  const details = [
    [t("review:author"), pullRequest.authorUsername],
    [
      t("mergeDirection"),
      `${branchName(pullRequest.headRef)} → ${branchName(pullRequest.baseRef)}`,
    ],
    [t("repository:repositoryPath"), repositoryPath],
    [t("lastOpened"), openedAt],
  ];
  return <div className="w-72 text-left">
    <p className="break-words font-medium">{identity}</p>
    <dl className="mt-2 grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1.5">
      {details.map(([label, value]) => <div className="contents" key={label}>
        <dt className="text-muted-foreground">{label}</dt>
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
  review: RecentReviewSummary;
  openedAt: string;
}) {
  const { t } = useTranslation();
  const details = [
    [t("mergeDirection"), `${branchName(review.review.selection.headRef)} → ${branchName(review.review.selection.baseRef)}`],
    [t("repository:repositoryPath"), review.repositoryPath],
    [t("lastOpened"), openedAt],
  ];
  return <dl className="grid w-72 grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-1.5 text-left">
    {details.map(([label, value]) => <div className="contents" key={label}>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0 break-words">{value}</dd>
    </div>)}
  </dl>;
}

/** Review 导航条目按 Selection 类型呈现紧凑身份与最近打开时间。 */
function ReviewMenuItem({
  review,
  showRepository,
  onOpen,
  active = false,
  pending = false,
  disabled = false,
}: {
  review: RecentReviewSummary;
  showRepository: boolean;
  onOpen: () => void;
  active?: boolean;
  pending?: boolean;
  disabled?: boolean;
}) {
  const { i18n } = useTranslation();
  const locale = normalizeLocale(i18n.resolvedLanguage);
  const time = formatRelativeTime(review.review.lastOpenedAt, Date.now(), locale);
  const pullRequest = review.review.selection.kind === "pull-request" ? review.review.selection.pullRequest : undefined;
  const identity = pullRequest ? `#${pullRequest.number} ${pullRequest.title}` : undefined;
  const openedAt = formatAbsoluteTime(review.review.lastOpenedAt, locale);
  const tooltip = {
    hidden: false,
    side: "right" as const,
    align: "start" as const,
    sideOffset: 16,
    className: "block max-w-xs border border-border bg-popover px-3 py-2.5 text-popover-foreground shadow-lg [&>[data-side]]:hidden",
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
      isActive={active}
      onClick={onOpen}
      aria-current={active ? "page" : undefined}
      data-review-id={review.review.reviewId}
      disabled={pending || disabled}
      aria-busy={pending || undefined}
    >
      <span className="flex min-w-0 flex-1 items-center gap-2 text-xs">
        {showRepository && <Badge
          variant="secondary"
          className="h-5 shrink-0 rounded-sm border-0 bg-muted px-1.5 text-foreground"
        >
          {repositoryName(review.repositoryPath)}
        </Badge>}
        {identity
          ? <span className="min-w-0 flex-1 truncate">{identity}</span>
          : <CompactBranchRelation baseRef={review.review.selection.baseRef} headRef={review.review.selection.headRef} />}
        {pending ? <Spinner className="mr-3 shrink-0" aria-hidden /> : <span className="mr-3 shrink-0 text-[11px] text-muted-foreground">{time}</span>}
      </span>
    </SidebarMenuButton>
  </SidebarMenuItem>;
}

/** 最近审核首页只消费 AppShell navigation 投影，不参与主视图事务。 */
export function HomeView({
  reviews,
  error,
  loading,
  navigationBusy,
  pendingReviewId,
  onChooseFolder,
  onOpenReview,
}: {
  reviews: RecentReviewSummary[];
  error?: string;
  loading: boolean;
  navigationBusy: boolean;
  pendingReviewId?: string;
  onChooseFolder(): void;
  onOpenReview(review: RecentReviewSummary): void;
}) {
  const { t } = useTranslation();
  return <>
    {!reviews.length && <header className="flex items-center gap-3 border-b pb-5">
      <div><h1 className="font-brand text-xl">{t("brand")}</h1><p className="text-sm text-muted-foreground">{t("subtitle")}</p></div>
    </header>}
    {error && <p role="alert" className="mx-auto mt-6 max-w-3xl rounded-lg border border-destructive/30 bg-destructive/5 p-4 text-sm text-destructive">{error}</p>}
    {loading
      ? <section role="status" aria-label={t("loadingRecent")} className="mx-auto max-w-3xl space-y-4 py-10"><Skeleton className="h-8 w-40" /><Skeleton className="h-16 w-full" /><Skeleton className="h-16 w-full" /></section>
      : reviews.length
        ? <section className="mx-auto max-w-3xl py-10" aria-label={t("recent")}>
          <div className="flex items-center justify-between gap-4">
            <h2 className="font-heading text-2xl">{t("recent")}</h2>
            <Button onClick={onChooseFolder}><Icon icon={FolderOpenIcon} />{t("openFolder")}</Button>
          </div>
          <ul className="mt-5 divide-y rounded-lg border">
            {reviews.slice(0, 10).map((review) => <li key={review.review.reviewId}>
              <Button variant="ghost" className="h-auto w-full justify-start rounded-none px-4 py-4" disabled={navigationBusy} aria-busy={pendingReviewId === review.review.reviewId || undefined} onClick={() => onOpenReview(review)}>
                <span className="flex min-w-0 flex-col items-start gap-2">
                  <span className="flex gap-3">
                    <span className="font-medium">{repositoryName(review.repositoryPath)}</span>
                    <span className="truncate">{review.review.selection.kind === "pull-request"
                      ? `#${review.review.selection.pullRequest.number} ${review.review.selection.pullRequest.title}`
                      : t("localBranch")}</span>
                  </span>
                  <BranchRelation baseRef={review.review.selection.baseRef} headRef={review.review.selection.headRef} direction="head-to-base" interactive={false} />
                </span>
                {pendingReviewId === review.review.reviewId && <Spinner className="ml-auto" aria-hidden />}
              </Button>
            </li>)}
          </ul>
        </section>
        : <section className="mx-auto flex max-w-xl flex-col items-start gap-5 py-24"><h2 className="font-brand text-3xl">{t("emptyTitle")}</h2><p className="text-muted-foreground">{t("emptyDescription")}</p><Button size="lg" onClick={onChooseFolder}><Icon icon={FolderOpenIcon} />{t("openFolder")}</Button></section>}
  </>;
}

/** 最近审核首页，以及仓库和 Review 历史的常驻导航。 */
export function AppShell({
  navigation,
  githubIdentityEntry,
  children,
  enabled = true,
  fallbackClassName,
}: AppShellProps) {
  const { t } = useTranslation();
  const { destination, sidebar } = navigation;
  const {
    reviews,
    repositories,
    error,
    loading,
    activeRepositoryId,
    activeReviewId,
    pendingRepositoryId,
    pendingReviewId,
    busy: reviewNavigationBusy,
  } = sidebar;
  const showHome = destination.kind === "home";
  const workspace = destination.kind === "review";
  const [preferences, setPreferences] = useState<SidebarPreferences>(readPreferences);
  const [resizingSidebar, setResizingSidebar] = useState(false);
  const seenActiveRepositories = useRef(new Set<number>());
  const openSidebar = preferences.open !== false;
  const sidebarWidth = typeof preferences.width === "number" ? preferences.width : 280;

  useEffect(() => {
    window.localStorage?.setItem(preferenceKey, JSON.stringify(preferences));
  }, [preferences]);

  useEffect(() => {
    if (!activeReviewId || activeRepositoryId === undefined) return;
    setPreferences((value) => value.expandedRepositories.includes(activeRepositoryId)
      ? value
      : { ...value, expandedRepositories: [...value.expandedRepositories, activeRepositoryId] });
  }, [activeRepositoryId, activeReviewId]);

  useEffect(() => {
    if (activeRepositoryId === undefined || seenActiveRepositories.current.has(activeRepositoryId)) return;
    seenActiveRepositories.current.add(activeRepositoryId);
    setPreferences((value) => value.expandedRepositories.includes(activeRepositoryId)
      ? value
      : { ...value, expandedRepositories: [...value.expandedRepositories, activeRepositoryId] });
  }, [activeRepositoryId]);

  useEffect(() => {
    if (!activeReviewId) return;
    const selected = [...document.querySelectorAll<HTMLElement>("[data-review-id]")]
      .find((item) => item.dataset.reviewId === activeReviewId && item.getAttribute("aria-current") === "page");
    selected?.scrollIntoView?.({ block: "nearest" });
  }, [activeReviewId, preferences.expandedRepositories]);

  /** 拖动侧边栏边界；仅保留最小宽度，允许按需扩展到任意宽度。 */
  function resizeSidebar(event: MouseEvent<HTMLDivElement>) {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = sidebarWidth;
    setResizingSidebar(true);
    const resize = (move: globalThis.MouseEvent) => setPreferences((value) => ({
      ...value,
      width: Math.max(220, startWidth + move.clientX - startX),
    }));
    const stop = () => {
      setResizingSidebar(false);
      document.removeEventListener("mousemove", resize);
      document.removeEventListener("mouseup", stop);
    };
    document.addEventListener("mousemove", resize);
    document.addEventListener("mouseup", stop);
  }

  /** 使用系统目录选择器；WDIO 构建从 Rust 侧读取隔离 fixture，避免跨 WebView 注入。 */
  async function chooseFolder() {
    const path = import.meta.env.VITE_WDIO === "true"
      ? await tauriTransport.invoke("choose_e2e_repository_directory", {})
      : await open({ directory: true, multiple: false });
    if (typeof path === "string") void navigation.registerAndOpen(path);
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
  function openRecentReview(review: RecentReviewSummary) {
    setPreferences((value) => ({
      ...value,
      expandedRepositories: value.expandedRepositories.includes(review.repositoryId)
        ? value.expandedRepositories
        : [...value.expandedRepositories, review.repositoryId],
    }));
    void navigation.navigate({ kind: "review", repositoryId: review.repositoryId, target: { kind: "saved-review", reviewId: review.review.reviewId } });
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

  if (!enabled) return <main className={fallbackClassName}>{children}</main>;

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
        className="absolute left-3 top-1.5 z-10 bg-transparent hover:bg-sidebar-accent supports-[padding:max(0px)]:left-[max(0.75rem,env(titlebar-area-x,0px)+5.5rem)]"
      />
    </div>
    <div
      aria-hidden
      data-slot="main-elevation-shadow"
      className={cn(
        "pointer-events-none fixed inset-y-0 z-40 w-5 -translate-x-full bg-linear-to-r from-transparent to-black/[0.03] opacity-100 transition-[left,opacity] duration-200 ease-linear dark:to-black/[0.06]",
        !openSidebar && "opacity-0",
        resizingSidebar && "transition-none",
      )}
      style={{ left: openSidebar ? `${sidebarWidth}px` : 0 }}
    />
    <Sidebar aria-label={t("sidebar")} collapsible="offcanvas" className="pt-12">
      <SidebarHeader className="px-3 py-3">
        <button
          type="button"
          aria-label={t("brand")}
          aria-current={showHome ? "page" : undefined}
          className="flex w-full cursor-pointer items-center gap-3 rounded-lg px-1 py-1 text-left outline-none hover:bg-sidebar-accent focus-visible:ring-2 focus-visible:ring-sidebar-ring"
          onClick={() => void navigation.navigate({ kind: "home" })}
        >
          <picture className="size-12 shrink-0">
            <source media="(prefers-color-scheme: dark)" srcSet={narrantLogoDark} />
            <img className="size-12" src={narrantLogo} alt={t("brand")} />
          </picture>
          <span className="min-w-0">
            <span className="block font-brand text-lg">{t("brand")}</span>
            <span className="block truncate text-xs text-muted-foreground">{t("subtitle")}</span>
          </span>
        </button>
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
            <Icon icon={ArrowDown01Icon} className={cn(
              "ml-1 size-4 opacity-0 transition-[transform,opacity] group-hover/section-label:opacity-100 group-focus-visible/section-label:opacity-100",
              !preferences.recentGroupOpen && "-rotate-90",
            )} />
          </button>
          {preferences.recentGroupOpen && <SidebarGroupContent>
            <SidebarMenu>
              {reviews.slice(0, preferences.recentExpanded ? 10 : 5).map((review) =>
                <ReviewMenuItem key={review.review.reviewId} review={review} showRepository pending={pendingReviewId === review.review.reviewId} disabled={reviewNavigationBusy} onOpen={() => openRecentReview(review)} />)}
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
            <Icon icon={ArrowDown01Icon} className={cn(
              "ml-1 size-4 opacity-0 transition-[transform,opacity] group-hover/section-label:opacity-100 group-focus-visible/section-label:opacity-100",
              !preferences.repositoriesGroupOpen && "-rotate-90",
            )} />
          </button>
          {preferences.repositoriesGroupOpen && <SidebarGroupContent>
            <SidebarMenu>
              {repositories.map((repository) => {
                const expanded = preferences.expandedRepositories.includes(repository.id);
                const all = preferences.allRepositoryReviews.includes(repository.id);
                const repositoryReviews = reviews.filter((review) => review.repositoryId === repository.id);
                const pending = pendingRepositoryId === repository.id;
                return <SidebarMenuItem key={repository.id}>
                  <div className="group/repository relative flex items-center">
                    <SidebarMenuButton
                      className={cn("min-w-0 flex-1 pr-9", repository.availability.kind !== "available" && "text-muted-foreground")}
                      tooltip={repository.canonicalPath}
                      isActive={activeRepositoryId === repository.id && !activeReviewId}
                      aria-expanded={expanded}
                      aria-current={activeRepositoryId === repository.id && !activeReviewId ? "page" : undefined}
                      onClick={() => toggleRepository(repository.id)}
                    >
                      {expanded ? <Icon icon={FolderOpenIcon} /> : <Icon icon={Folder01Icon} />}
                      <span className="truncate">{repositoryName(repository.canonicalPath)}</span>
                    </SidebarMenuButton>
                    <Button
                      variant="outline"
                      size="icon-xs"
                      className="pointer-events-none absolute inset-y-0 right-1 z-10 my-auto rounded-md border-sidebar-border bg-sidebar opacity-0 shadow-xs transition-opacity hover:bg-sidebar-accent active:translate-y-0! group-hover/repository:pointer-events-auto group-hover/repository:opacity-100 group-focus-within/repository:pointer-events-auto group-focus-within/repository:opacity-100"
                      aria-label={t(pending ? "openingRepository" : "openRepository", { name: repositoryName(repository.canonicalPath) })}
                      disabled={pending}
                      aria-busy={pending || undefined}
                      onClick={() => void navigation.navigate({ kind: "repository", repositoryId: repository.id })}
                    >
                      {pending ? <Spinner aria-hidden /> : <Icon icon={ArrowRight02Icon} />}
                    </Button>
                  </div>
                  {expanded && <SidebarMenu className="ml-5 mt-1 w-auto pl-2">
                    {repositoryReviews.slice(0, all ? undefined : 10).map((review) =>
                      <ReviewMenuItem key={review.review.reviewId} review={review} showRepository={false} active={activeReviewId === review.review.reviewId} pending={pendingReviewId === review.review.reviewId} disabled={reviewNavigationBusy} onOpen={() => void navigation.navigate({ kind: "review", repositoryId: review.repositoryId, target: { kind: "saved-review", reviewId: review.review.reviewId } })} />)}
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
    <main className={cn(
      "relative z-20 w-full bg-background px-8 pt-14",
      workspace ? "h-screen overflow-hidden pb-4" : "min-h-screen pb-8",
    )}>
      {!showHome ? children : <HomeView
        reviews={reviews}
        error={error}
        loading={loading}
        navigationBusy={reviewNavigationBusy}
        pendingReviewId={pendingReviewId}
        onChooseFolder={() => void chooseFolder()}
        onOpenReview={openRecentReview}
      />}
    </main>
  </SidebarProvider>;
}

/** 为首页组件测试与独立展示保留的最近审核首页入口。 */
export function HomePage({
  reviews,
  repositories = [],
  error,
  loading = false,
  onOpenReview,
  onOpenRepository = () => undefined,
  onOpenFolder,
  githubIdentityEntry,
  children,
  showHome = true,
  onOpenHome = () => undefined,
  enabled = true,
  fallbackClassName,
  activeRepositoryId,
  activeReviewId,
  pendingRepositoryId,
  pendingReviewId,
  reviewNavigationBusy = false,
}: HomePageProps) {
  const destination = showHome
    ? { kind: "home" as const }
    : activeReviewId && activeRepositoryId !== undefined
      ? { kind: "review" as const, repositoryId: activeRepositoryId, reviewId: activeReviewId }
      : { kind: "repository" as const, repositoryId: activeRepositoryId ?? 0, initialTab: "pull-requests" as const };
  const navigated = (): Promise<AppNavigationResult> => Promise.resolve({ kind: "navigated" });
  return <AppShell
    navigation={{
      destination,
      sidebar: {
        reviews,
        repositories,
        error,
        loading,
        activeRepositoryId,
        activeReviewId,
        pendingRepositoryId,
        pendingReviewId,
        busy: reviewNavigationBusy,
      },
      navigate(intent) {
        if (intent.kind === "home") onOpenHome();
        if (intent.kind === "repository") {
          const repository = repositories.find((candidate) => candidate.id === intent.repositoryId);
          if (repository) onOpenRepository(repository);
        }
        if (intent.kind === "review" && intent.target.kind === "saved-review") {
          const reviewId = intent.target.reviewId;
          const review = reviews.find((candidate) => candidate.review.reviewId === reviewId);
          if (review) onOpenReview(review);
        }
        return navigated();
      },
      registerAndOpen(path) {
        onOpenFolder(path);
        return navigated();
      },
    }}
    githubIdentityEntry={githubIdentityEntry}
    enabled={enabled}
    fallbackClassName={fallbackClassName}
  >{children}</AppShell>;
}
