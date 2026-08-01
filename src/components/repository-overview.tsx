import type { ReactNode } from "react";
import { AlertCircleIcon, GitBranchIcon, GitPullRequestIcon, HistoryIcon, InformationCircleIcon } from "@hugeicons/core-free-icons";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useTranslation } from "react-i18next";
import { BranchRelation } from "@/components/branch-relation";
import type { DiscoveredPullRequest, PullRequestGroup, ReviewSummary, SavedReviewSummary } from "@/lib/review-types";
import { Icon } from "@/components/ui/icon";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { formatRelativeTime, normalizeLocale } from "@/lib/i18n";
export type { DiscoveredPullRequest, PullRequestGroup, SavedReviewSummary } from "@/lib/review-types";

/** Repository Entry 概览所需的外部状态与用户动作。 */
type Props = {
  repositoryName: string;
  repositoryPath: string;
  savedReviews: SavedReviewSummary[];
  pullRequestGroups: PullRequestGroup[];
  repositoryReviews: ReviewSummary[];
  discoveryLoading: boolean;
  discoveryError?: string;
  githubConnected: boolean;
  localBranches: ReactNode;
  initialTab?: "pull-requests" | "branches" | "history";
  onRetryDiscovery: () => void;
  onConnectGitHub: () => void;
  onOpenSavedReview: (review: SavedReviewSummary) => void;
  onOpenPullRequest: (
    repository: PullRequestGroup["repository"],
    pullRequest: DiscoveredPullRequest,
  ) => void;
};

/** 合并后用于渲染一个 Pull Request Selection 的行模型。 */
type PullRequestRow = {
  key: string;
  repository: PullRequestGroup["repository"];
  saved?: SavedReviewSummary;
  discovered?: DiscoveredPullRequest;
};

/** 将 GitHub 时间转换为毫秒时间戳，并在旧缓存缺失或值无效时使用明确回退。 */
function parseGitHubTimestamp(value?: string, fallback = 0) {
  const parsed = value ? Date.parse(value) : Number.NaN;
  return Number.isFinite(parsed) ? parsed : fallback;
}

/** 返回列表排序所使用的 GitHub 最近更新时间；旧缓存退回到 metadata 同步时间。 */
function rowUpdatedAt(row: PullRequestRow) {
  if (row.discovered) return parseGitHubTimestamp(row.discovered.updatedAt);
  return parseGitHubTimestamp(
    row.saved?.pullRequest?.updatedAt,
    row.saved?.pullRequest?.synchronizedAt ?? 0,
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
  return <div className="mx-auto flex max-w-[35rem] flex-col items-center rounded-lg border bg-card px-6 py-12 text-center shadow-sm">
    <Icon icon={icon} className="mb-4 size-7 text-muted-foreground" aria-hidden />
    <p className="font-medium">{title}</p>
    {description && <p className="mt-2 max-w-md text-sm text-muted-foreground">{description}</p>}
    {action && <div className="mt-5">{action}</div>}
  </div>;
}

/** 按稳定的 Pull Request Selection identity 合并本地 Review 与后台 discovery 结果。 */
function mergePullRequests(savedReviews: SavedReviewSummary[], groups: PullRequestGroup[]): PullRequestRow[] {
  const rows = new Map<string, PullRequestRow>();
  for (const saved of savedReviews) {
    if (!saved.pullRequest) continue;
    const repository = { owner: saved.pullRequest.owner, name: saved.pullRequest.repository };
    const key = `${repository.owner.toLowerCase()}/${repository.name.toLowerCase()}#${saved.pullRequest.number}`;
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

/** Repository Entry 的任务概览：缓存优先呈现 Pull Request，并隔离远端失败。 */
export function RepositoryOverview(props: Props) {
  const { t, i18n } = useTranslation(["catalog", "common", "github"]);
  const rows = mergePullRequests(props.savedReviews, props.pullRequestGroups);
  const groupErrors = props.pullRequestGroups.flatMap((group) => group.error ? [group.error] : []);
  const hasSuccessfulGroup = props.pullRequestGroups.some((group) => !group.error);
  const hasPartialFailure = hasSuccessfulGroup && groupErrors.length > 0;
  const blockingDiscoveryError = props.discoveryError ?? (hasPartialFailure ? undefined : groupErrors[0]);
  const repositoryIdentities = new Set([
    ...props.pullRequestGroups.map((group) => `${group.repository.owner.toLowerCase()}/${group.repository.name.toLowerCase()}`),
    ...props.savedReviews.flatMap((review) => review.pullRequest
      ? [`${review.pullRequest.owner.toLowerCase()}/${review.pullRequest.repository.toLowerCase()}`]
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
          <TooltipContent className="max-w-xl whitespace-normal break-all">{props.repositoryPath}</TooltipContent>
        </Tooltip>
      </header>
      <Tabs key={props.initialTab} defaultValue={props.initialTab ?? "pull-requests"}>
        <TabsList aria-label={t("catalog:tasks")}>
          <TabsTrigger value="pull-requests"><Icon icon={GitPullRequestIcon} />{t("catalog:tabs.pullRequests")}</TabsTrigger>
          <TabsTrigger value="branches"><Icon icon={GitBranchIcon} />{t("catalog:tabs.branches")}</TabsTrigger>
          <TabsTrigger value="history"><Icon icon={HistoryIcon} />{t("catalog:tabs.history")}</TabsTrigger>
        </TabsList>
        <TabsContent value="pull-requests" className="mt-5">
          {hasPartialFailure && <div className="mb-4 flex items-start gap-3 rounded-lg border bg-muted/30 p-3 text-sm text-muted-foreground" role="status">
            <Icon icon={AlertCircleIcon} className="mt-0.5 shrink-0" aria-hidden />
            <div className="min-w-0 flex-1 space-y-1">
              {props.pullRequestGroups.filter((group) => group.error).map((group) => {
                const identity = `${group.repository.owner}/${group.repository.name}`;
                return <p key={identity}>{t("catalog:partialFailure", { repository: identity, error: group.error })}</p>;
              })}
            </div>
            <Button size="sm" variant="outline" onClick={props.onRetryDiscovery}>{t("catalog:retry")}</Button>
          </div>}
          {(props.discoveryLoading || props.discoveryError) && rows.length > 0 && <div className="mb-4 rounded-md border bg-muted/30 p-3 text-sm text-muted-foreground" role="status">
            {props.discoveryLoading ? `${t("catalog:refreshing")} ` : ""}
            {props.discoveryError ? `${props.discoveryError}；` : ""}{t("catalog:stale")}
          </div>}
          {rows.length > 0 && <ul className="divide-y rounded-lg border">
            {rows.map((row) => {
              const metadata = row.discovered ?? row.saved!.pullRequest!;
              const baseRef = row.discovered?.base.ref ?? row.saved?.pullRequest?.baseRef;
              const headRef = row.discovered?.head.ref ?? row.saved?.pullRequest?.headRef;
              const updated = Boolean(row.saved && row.discovered && (
                row.saved.baseRevision !== row.discovered.base.sha
                || row.saved.headRevision !== row.discovered.head.sha
              ));
              const updatedAt = row.discovered?.updatedAt ?? row.saved?.pullRequest?.updatedAt;
              return <li key={row.key} className="flex flex-col items-stretch gap-3 p-4 sm:flex-row sm:items-center sm:gap-4">
                <Icon icon={GitPullRequestIcon} className="hidden shrink-0 text-muted-foreground sm:block" aria-hidden />
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <p className="truncate font-medium">#{metadata.number} {metadata.title}</p>
                    {showRepositoryIdentity && <Badge variant="outline">{row.repository.owner}/{row.repository.name}</Badge>}
                    {metadata.draft && <Badge variant="secondary">{t("catalog:draft")}</Badge>}
                    {updated && <Badge>{t("catalog:updateAvailable")}</Badge>}
                  </div>
                  <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                    <span>{metadata.author}</span><span>{metadata.state}</span>
                    {baseRef && headRef && <BranchRelation baseRef={baseRef} headRef={headRef} />}
                    {updatedAt && <time dateTime={updatedAt}>{formatRelativeTime(rowUpdatedAt(row), Date.now(), locale)}</time>}
                  </div>
                </div>
                <Button className="self-start sm:self-auto" aria-label={`${row.saved ? t("common:continueReview") : t("common:startReview")} #${metadata.number} ${metadata.title}`} variant={row.saved ? "outline" : "default"} onClick={() => {
                  if (row.saved) props.onOpenSavedReview(row.saved);
                  else if (row.discovered) props.onOpenPullRequest(row.repository, row.discovered);
                }}>{row.saved ? t("common:continueReview") : t("common:startReview")}</Button>
              </li>;
            })}
          </ul>}
          {rows.length === 0 && (!props.githubConnected
            ? <PullRequestStatus icon={GitPullRequestIcon} title={t("catalog:connectForPrivate")} description={t("catalog:connectForPrivateDescription")} action={<Button onClick={props.onConnectGitHub}>{t("github:connect")}</Button>} />
            : blockingDiscoveryError
              ? <PullRequestStatus icon={AlertCircleIcon} title={t("catalog:loadFailed")} description={blockingDiscoveryError} action={<Button variant="outline" onClick={props.onRetryDiscovery}>{t("catalog:retry")}</Button>} />
              : <PullRequestStatus icon={InformationCircleIcon} title={t("catalog:noPullRequests")} description={t("catalog:noPullRequestsDescription")} />)}
        </TabsContent>
        <TabsContent value="branches" className="mt-5">{props.localBranches}</TabsContent>
        <TabsContent value="history" className="mt-5">
          {props.repositoryReviews.length === 0
            ? <p className="rounded-lg border border-dashed p-10 text-center text-muted-foreground">{t("catalog:noHistory")}</p>
            : <ul className="divide-y rounded-lg border">{props.repositoryReviews.map((review) => <li className="p-4" key={review.reviewId}><BranchRelation baseRef={review.baseRef} headRef={review.headRef} /></li>)}</ul>}
        </TabsContent>
      </Tabs>
  </section>;
}
