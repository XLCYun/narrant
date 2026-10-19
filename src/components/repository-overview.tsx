import { type ReactNode } from "react";
import { AlertCircleIcon, GitBranchIcon, GitMergeIcon, GitPullRequestClosedIcon, GitPullRequestIcon, HistoryIcon, InformationCircleIcon, ListViewIcon, Refresh01Icon } from "@hugeicons/core-free-icons";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useTranslation } from "react-i18next";
import { BranchRelation, branchName } from "@/components/branch-relation";
import type { DiscoveredPullRequest, PullRequestGroup, PullRequestStateFilter, RepositoryReviewSummary } from "@/lib/review-types";
import { Icon } from "@/components/ui/icon";
import { Spinner } from "@/components/ui/spinner";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { formatRelativeTime, normalizeLocale } from "@/lib/i18n";
export type { DiscoveredPullRequest, PullRequestGroup, RepositoryReviewSummary } from "@/lib/review-types";

/** Repository 概览所需的外部状态与用户动作。 */
type Props = {
  repositoryName: string;
  repositoryPath: string;
  reviews: RepositoryReviewSummary[];
  pullRequestGroups: PullRequestGroup[];
  discoveryLoading: boolean;
  discoveryLoadingMore: boolean;
  discoveryComplete: boolean;
  discoveryError?: string;
  pullRequestFilter: PullRequestStateFilter;
  githubConnected: boolean;
  localBranches: ReactNode;
  initialTab?: "pull-requests" | "branches" | "history";
  openingReviewKey?: string;
  reviewNavigationBusy?: boolean;
  onRetryDiscovery: () => void;
  onRefreshDiscovery: () => void;
  onLoadMoreDiscovery: () => void;
  onPullRequestFilterChange: (filter: PullRequestStateFilter) => void;
  onConnectGitHub: () => void;
  onOpenReview: (review: RepositoryReviewSummary) => void;
  onOpenPullRequest: (
    repository: PullRequestGroup["repository"],
    pullRequest: DiscoveredPullRequest,
  ) => void;
};

/** 合并后用于渲染一个 Pull Request Selection 的行模型。 */
type PullRequestRow = {
  key: string;
  repository: PullRequestGroup["repository"];
  saved?: RepositoryReviewSummary;
  discovered?: DiscoveredPullRequest;
};

/** 生命周期图标由筛选与行标签共用，颜色只用于行内胶囊标签。 */
const pullRequestStates = [
  {
    value: "open", icon: GitPullRequestIcon,
    badgeClassName: "border-emerald-600/20 bg-emerald-500/10 text-emerald-700 dark:border-emerald-400/25 dark:bg-emerald-400/10 dark:text-emerald-300",
  },
  {
    value: "closed", icon: GitPullRequestClosedIcon,
    badgeClassName: "border-rose-600/20 bg-rose-500/10 text-rose-700 dark:border-rose-400/25 dark:bg-rose-400/10 dark:text-rose-300",
  },
  {
    value: "merged", icon: GitMergeIcon,
    badgeClassName: "border-violet-600/20 bg-violet-500/10 text-violet-700 dark:border-violet-400/25 dark:bg-violet-400/10 dark:text-violet-300",
  },
] as const;
/** All 使用完整列表图标，筛选按钮继续沿用 Tabs 的默认样式。 */
const pullRequestStateFilters = [
  ...pullRequestStates,
  { value: "all", icon: ListViewIcon },
] as const;

/** 让筛选与行展示共用同一份元数据，在线发现结果优先于已保存的 Review。 */
function rowMetadata(row: PullRequestRow) {
  const savedSelection = row.saved?.selection.kind === "pull-request" ? row.saved.selection : undefined;
  return row.discovered ?? savedSelection!.pullRequest;
}

/** 将 GitHub 时间转换为毫秒时间戳，并在旧缓存缺失或值无效时使用明确回退。 */
function parseGitHubTimestamp(value?: string, fallback = 0) {
  const parsed = value ? Date.parse(value) : Number.NaN;
  return Number.isFinite(parsed) ? parsed : fallback;
}

/** 返回列表排序所使用的 GitHub 最近更新时间；旧缓存退回到 metadata 同步时间。 */
function rowUpdatedAt(row: PullRequestRow) {
  if (row.discovered) return parseGitHubTimestamp(row.discovered.githubUpdatedAt);
  const metadata = row.saved?.selection.kind === "pull-request"
    ? row.saved.selection.pullRequest
    : undefined;
  return parseGitHubTimestamp(
    metadata?.githubUpdatedAt,
    metadata?.synchronizedAtMillis ?? 0,
  );
}

/** Pull Request 空状态与阻断错误共享的紧凑面板。 */
function PullRequestStatus({
  icon,
  title,
  description,
  action,
}: {
  icon: typeof InformationCircleIcon;
  title: string;
  description?: string;
  action?: ReactNode;
}) {
  return <div className="mx-auto flex max-w-[35rem] flex-col items-center px-6 py-12 text-center">
    <Icon icon={icon} className="mb-4 size-7 text-muted-foreground" aria-hidden />
    <p className="font-medium">{title}</p>
    {description && <p className="mt-2 max-w-md text-sm text-muted-foreground">{description}</p>}
    {action && <div className="mt-5">{action}</div>}
  </div>;
}

/** 按稳定的 Pull Request Selection identity 合并本地 Review 与后台 discovery 结果。 */
function mergePullRequests(reviews: RepositoryReviewSummary[], groups: PullRequestGroup[]): PullRequestRow[] {
  const rows = new Map<string, PullRequestRow>();
  for (const saved of reviews) {
    if (saved.selection.kind !== "pull-request") continue;
    const metadata = saved.selection.pullRequest;
    const repository = { owner: metadata.owner, name: metadata.repository };
    const key = `${repository.owner.toLowerCase()}/${repository.name.toLowerCase()}#${metadata.number}`;
    rows.set(key, { key, repository, saved });
  }
  for (const group of groups) {
    for (const discovered of group.pullRequests) {
      const key = `${group.repository.owner.toLowerCase()}/${group.repository.name.toLowerCase()}#${discovered.number}`;
      rows.set(key, { ...rows.get(key), key, repository: group.repository, discovered });
    }
  }
  return [...rows.values()].sort((left, right) => {
    return rowUpdatedAt(right) - rowUpdatedAt(left);
  });
}

/** Repository 的任务概览：缓存优先呈现 Pull Request，并隔离远端失败。 */
export function RepositoryOverview(props: Props) {
  const { t, i18n } = useTranslation(["repository", "common", "github", "review"]);
  const stateFilter = props.pullRequestFilter;
  const hasMore = props.pullRequestGroups.some((group) => !group.error && group.nextPage !== null);
  const rows = mergePullRequests(props.reviews, props.pullRequestGroups);
  const visibleRows = rows.filter((row) => stateFilter === "all" || rowMetadata(row).state === stateFilter);
  const groupErrors = props.pullRequestGroups.flatMap((group) => group.error ? [group.error] : []);
  const hasSuccessfulGroup = props.pullRequestGroups.some((group) => !group.error);
  const hasPartialFailure = hasSuccessfulGroup && groupErrors.length > 0;
  const blockingDiscoveryError = props.discoveryError ?? (hasPartialFailure ? undefined : groupErrors[0]);
  const complete = props.discoveryComplete && !props.discoveryLoading && !props.discoveryError && groupErrors.length === 0;
  const showConnect = rows.length === 0 && !props.githubConnected && !props.discoveryLoading && !hasSuccessfulGroup;
  const repositoryIdentities = new Set([
    ...props.pullRequestGroups.map((group) => `${group.repository.owner.toLowerCase()}/${group.repository.name.toLowerCase()}`),
    ...props.reviews.flatMap((review) => review.selection.kind === "pull-request"
      ? [`${review.selection.pullRequest.owner.toLowerCase()}/${review.selection.pullRequest.repository.toLowerCase()}`]
      : []),
  ]);
  const showRepositoryIdentity = repositoryIdentities.size > 1;
  const locale = normalizeLocale(i18n.resolvedLanguage);
  return <section className="min-w-0 max-w-6xl">
      <header className="mb-6">
        <Tooltip>
          <TooltipTrigger render={<button type="button" className="rounded-sm text-left focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring" />}>
            <h2 className="font-mono text-2xl">{props.repositoryName}</h2>
          </TooltipTrigger>
          <TooltipContent hideArrow side="right" className="max-w-xl whitespace-normal break-all">{props.repositoryPath}</TooltipContent>
        </Tooltip>
      </header>
      <Tabs key={props.initialTab} defaultValue={props.initialTab ?? "pull-requests"}>
        <TabsList aria-label={t("repository:tasks")} className="max-w-full flex-wrap group-data-horizontal/tabs:h-auto">
          <TabsTrigger value="pull-requests">
            {props.discoveryLoading ? <Spinner aria-hidden /> : <Icon icon={GitPullRequestIcon} />}
            {t("repository:tabs.pullRequests")}
          </TabsTrigger>
          <TabsTrigger value="branches"><Icon icon={GitBranchIcon} />{t("repository:tabs.branches")}</TabsTrigger>
          <TabsTrigger value="history"><Icon icon={HistoryIcon} />{t("repository:tabs.history")}</TabsTrigger>
        </TabsList>
        <TabsContent value="pull-requests" className="mt-5">
          <Tabs value={stateFilter} onValueChange={(value) => props.onPullRequestFilterChange(value as PullRequestStateFilter)} className="gap-0 rounded-lg border">
            <div className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b p-3 sm:p-4">
              <span className="text-sm text-muted-foreground">{t("repository:pullRequestFilter.label")}</span>
              <TabsList aria-label={t("repository:pullRequestFilter.accessibleLabel")} className="max-w-full flex-wrap group-data-horizontal/tabs:h-auto">
                {pullRequestStateFilters.map(({ value, icon }) => <TabsTrigger
                  key={value}
                  value={value}
                >
                  <Icon icon={icon} aria-hidden />
                  {t(`repository:pullRequestFilter.${value}`)}
                </TabsTrigger>)}
              </TabsList>
              <Button
                type="button"
                size="sm"
                variant="outline"
                className="ml-auto"
                disabled={props.discoveryLoading}
                aria-busy={props.discoveryLoading || undefined}
                onClick={props.onRefreshDiscovery}
              >
                {props.discoveryLoading ? <Spinner aria-hidden /> : <Icon icon={Refresh01Icon} aria-hidden />}
                {t(props.discoveryLoading && !props.discoveryLoadingMore ? "repository:refreshingPullRequests" : "repository:refreshPullRequests")}
              </Button>
            </div>
            <TabsContent value={stateFilter}>
              {hasPartialFailure && <div className="m-4 flex flex-wrap items-start gap-3 rounded-lg border bg-muted/30 p-3 text-sm text-muted-foreground" role="status">
                <Icon icon={AlertCircleIcon} className="mt-0.5 shrink-0" aria-hidden />
                <div className="min-w-0 flex-1 space-y-1 break-words">
                  {props.pullRequestGroups.filter((group) => group.error).map((group) => {
                    const identity = `${group.repository.owner}/${group.repository.name}`;
                    return <p key={identity}>{t("repository:partialFailure", { repository: identity, error: group.error })}</p>;
                  })}
                </div>
                <Button size="sm" variant="outline" disabled={props.discoveryLoading} aria-busy={props.discoveryLoading || undefined} onClick={props.onRetryDiscovery}>
                  {props.discoveryLoading && <Spinner aria-hidden />}{t("repository:retry")}
                </Button>
              </div>}
              {blockingDiscoveryError && rows.length > 0 && <div className="m-4 flex flex-wrap items-center gap-3 rounded-md border bg-muted/30 p-3 text-sm text-muted-foreground" role="status">
                <p className="min-w-0 flex-1 break-words">{blockingDiscoveryError}；{t("repository:stale")}</p>
                <Button size="sm" variant="outline" disabled={props.discoveryLoading} onClick={props.onRetryDiscovery}>{t("repository:retry")}</Button>
              </div>}
              {visibleRows.length > 0 && <ul className="divide-y">
                {visibleRows.map((row) => {
                  const savedSelection = row.saved?.selection.kind === "pull-request" ? row.saved.selection : undefined;
                  const metadata = rowMetadata(row);
                  const stateAppearance = pullRequestStates.find(({ value }) => value === metadata.state);
                  const baseRef = row.discovered?.base.ref ?? savedSelection?.baseRef;
                  const headRef = row.discovered?.head.ref ?? savedSelection?.headRef;
                  const updated = Boolean(row.saved && row.discovered && (
                    row.saved.currentSnapshot.baseRevision !== row.discovered.base.sha
                    || row.saved.currentSnapshot.headRevision !== row.discovered.head.sha
                  ));
                  const githubUpdatedAt = row.discovered?.githubUpdatedAt ?? savedSelection?.pullRequest.githubUpdatedAt;
                  const opening = props.openingReviewKey === row.key;
                  return <li key={row.key} className="flex flex-col items-stretch gap-3 p-4 sm:flex-row sm:items-center sm:gap-4">
                    <Icon icon={GitPullRequestIcon} className="hidden shrink-0 text-muted-foreground sm:block" aria-hidden />
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <p className="truncate font-medium">#{metadata.number} {metadata.title}</p>
                        {showRepositoryIdentity && <Badge variant="outline">{row.repository.owner}/{row.repository.name}</Badge>}
                        {metadata.isDraft && <Badge variant="secondary">{t("repository:draft")}</Badge>}
                        {updated && <Badge>{t("repository:updateAvailable")}</Badge>}
                      </div>
                      <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                        <span>{metadata.authorUsername}</span>
                        <Badge variant="secondary" className={stateAppearance?.badgeClassName}>
                          {stateAppearance && <Icon icon={stateAppearance.icon} data-icon="inline-start" aria-hidden />}
                          {stateAppearance ? t(`review:state.${stateAppearance.value}`) : metadata.state}
                        </Badge>
                        {baseRef && headRef && <BranchRelation baseRef={baseRef} headRef={headRef} />}
                        {githubUpdatedAt && <time dateTime={githubUpdatedAt}>{formatRelativeTime(rowUpdatedAt(row), Date.now(), locale)}</time>}
                      </div>
                    </div>
                    <Button className="self-start sm:self-auto" aria-label={`${opening ? t("common:openingReview") : row.saved ? t("common:continueReview") : t("common:startReview")} #${metadata.number} ${metadata.title}`} aria-busy={opening || undefined} disabled={props.reviewNavigationBusy} variant={row.saved ? "outline" : "default"} onClick={() => {
                      if (row.saved) props.onOpenReview(row.saved);
                      else if (row.discovered) props.onOpenPullRequest(row.repository, row.discovered);
                    }}>{opening && <Spinner aria-hidden />}{row.saved ? t("common:continueReview") : t("common:startReview")}</Button>
                  </li>;
                })}
              </ul>}
              {visibleRows.length === 0 && !showConnect && (rows.length > 0 || !blockingDiscoveryError) && <PullRequestStatus
                icon={InformationCircleIcon}
                title={t(props.discoveryLoading ? "repository:loadingPullRequests" : complete ? (stateFilter === "all" ? "repository:noPullRequests" : "repository:pullRequestFilter.empty") : (stateFilter === "all" ? "repository:pullRequestPagination.noLoadedResults" : "repository:pullRequestPagination.noLoadedMatches"), { state: t(`repository:pullRequestFilter.${stateFilter}`) })}
                description={props.discoveryLoading ? undefined : t(complete ? "repository:pullRequestFilter.emptyDescription" : "repository:pullRequestPagination.incomplete")}
              />}
              {rows.length === 0 && (blockingDiscoveryError || showConnect) && <PullRequestStatus
                icon={AlertCircleIcon}
                title={t(props.githubConnected ? "repository:loadFailed" : "repository:connectForPrivate")}
                description={blockingDiscoveryError}
                action={<div className="flex flex-wrap justify-center gap-2">
                  {blockingDiscoveryError && <Button variant="outline" disabled={props.discoveryLoading} aria-busy={props.discoveryLoading || undefined} onClick={props.onRetryDiscovery}>{props.discoveryLoading && <Spinner aria-hidden />}{t("repository:retry")}</Button>}
                  {!props.githubConnected && <Button onClick={props.onConnectGitHub}>{t("github:connect")}</Button>}
                </div>}
              />}
              {(hasMore || props.discoveryLoadingMore || (complete && visibleRows.length > 0)) && <div className="flex flex-wrap items-center justify-center gap-3 border-t p-4">
                {hasMore || props.discoveryLoadingMore ? <>
                  <p className="text-sm text-muted-foreground" role="status">{t("repository:pullRequestPagination.partial")}</p>
                  <Button size="sm" variant="outline" disabled={props.discoveryLoading || Boolean(props.discoveryError)} aria-busy={props.discoveryLoadingMore || undefined} onClick={props.onLoadMoreDiscovery}>
                    {props.discoveryLoadingMore && <Spinner aria-hidden />}
                    {t(props.discoveryLoadingMore ? "repository:pullRequestPagination.loadingMore" : "repository:pullRequestPagination.loadMore")}
                  </Button>
                </> : <p className="text-sm text-muted-foreground" role="status">{t("repository:pullRequestPagination.end")}</p>}
              </div>}
            </TabsContent>
          </Tabs>
        </TabsContent>
        <TabsContent value="branches" className="mt-5">{props.localBranches}</TabsContent>
        <TabsContent value="history" className="mt-5">
          {props.reviews.length === 0
            ? <p className="rounded-lg border border-dashed p-10 text-center text-muted-foreground">{t("repository:noHistory")}</p>
            : <ul className="divide-y rounded-lg border">{props.reviews.map((review) => {
              const selection = review.selection;
              const opening = props.openingReviewKey === `review:${review.reviewId}`;
              const mergeDirection = t("repository:mergeConfirmation", {
                head: branchName(selection.headRef),
                base: branchName(selection.baseRef),
              });
              return <li key={review.reviewId}>
                <Button
                  variant="ghost"
                  className="h-auto w-full justify-start rounded-none px-4 py-4"
                  aria-label={`${t(opening ? "common:openingReview" : "common:continueReview")}：${mergeDirection}`}
                  aria-busy={opening || undefined}
                  disabled={props.reviewNavigationBusy}
                  onClick={() => props.onOpenReview(review)}
                >
                  <BranchRelation baseRef={selection.baseRef} headRef={selection.headRef} interactive={false} />
                  {opening && <Spinner className="ml-auto" aria-hidden />}
                </Button>
              </li>;
            })}</ul>}
        </TabsContent>
      </Tabs>
  </section>;
}
