import type { ReactNode } from "react";
import { GitBranchIcon, GitPullRequestIcon, HistoryIcon } from "@hugeicons/core-free-icons";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useTranslation } from "react-i18next";
import { BranchRelation } from "@/components/branch-relation";
import type { DiscoveredPullRequest, PullRequestGroup, ReviewSummary, SavedReviewSummary } from "@/lib/review-types";
import { Icon } from "@/components/ui/icon";
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
    const leftNumber = left.discovered?.number ?? left.saved?.pullRequest?.number ?? 0;
    const rightNumber = right.discovered?.number ?? right.saved?.pullRequest?.number ?? 0;
    return rightNumber - leftNumber;
  });
}

/** Repository Entry 的任务概览：缓存优先呈现 Pull Request，并隔离远端失败。 */
export function RepositoryOverview(props: Props) {
  const { t } = useTranslation(["catalog", "common", "github"]);
  const rows = mergePullRequests(props.savedReviews, props.pullRequestGroups);
  const groupErrors = props.pullRequestGroups.flatMap((group) => group.error ? [group.error] : []);
  const remoteError = props.discoveryError ?? groupErrors[0];
  return <section className="min-w-0">
      <header className="mb-6">
        <h2 className="font-heading text-2xl">{props.repositoryName}</h2>
        <p className="mt-1 break-all text-sm text-muted-foreground">{props.repositoryPath}</p>
      </header>
      <Tabs key={props.initialTab} defaultValue={props.initialTab ?? "pull-requests"}>
        <TabsList aria-label={t("catalog:tasks")}>
          <TabsTrigger value="pull-requests"><Icon icon={GitPullRequestIcon} />{t("catalog:tabs.pullRequests")}</TabsTrigger>
          <TabsTrigger value="branches"><Icon icon={GitBranchIcon} />{t("catalog:tabs.branches")}</TabsTrigger>
          <TabsTrigger value="history"><Icon icon={HistoryIcon} />{t("catalog:tabs.history")}</TabsTrigger>
        </TabsList>
        <TabsContent value="pull-requests" className="mt-5">
          {props.pullRequestGroups.length > 0 && <div className="mb-4 flex flex-wrap gap-2">
            {props.pullRequestGroups.map((group) => {
              const identity = `${group.repository.owner}/${group.repository.name}`;
              return <section key={identity} aria-label={identity} className="rounded-md border px-3 py-2">
                <h3 className="text-sm font-medium">{identity}</h3>
                <p className="text-xs text-muted-foreground">remotes: {group.remoteNames.join(", ")}</p>
                {group.error && <p className="text-xs text-destructive">{group.error}</p>}
              </section>;
            })}
          </div>}
          {(props.discoveryLoading || remoteError) && rows.length > 0 && <div className="mb-4 rounded-md border bg-muted/30 p-3 text-sm text-muted-foreground" role="status">
            {props.discoveryLoading ? `${t("catalog:refreshing")} ` : ""}
            {remoteError ? `${remoteError}；` : ""}{t("catalog:stale")}
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
              return <li key={row.key} className="flex items-center gap-4 p-4">
                <Icon icon={GitPullRequestIcon} className="shrink-0 text-muted-foreground" aria-hidden />
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <p className="truncate font-medium">#{metadata.number} {metadata.title}</p>
                    {metadata.draft && <Badge variant="secondary">{t("catalog:draft")}</Badge>}
                    {updated && <Badge>{t("catalog:updateAvailable")}</Badge>}
                  </div>
                  <div className="mt-1 flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
                    <span>{metadata.author}</span><span>{metadata.state}</span>
                    {baseRef && headRef && <BranchRelation baseRef={baseRef} headRef={headRef} />}
                  </div>
                </div>
                <Button aria-label={`${row.saved ? t("common:continueReview") : t("common:startReview")} #${metadata.number} ${metadata.title}`} variant={row.saved ? "outline" : "default"} onClick={() => {
                  if (row.saved) props.onOpenSavedReview(row.saved);
                  else if (row.discovered) props.onOpenPullRequest(row.repository, row.discovered);
                }}>{row.saved ? t("common:continueReview") : t("common:startReview")} #{metadata.number}</Button>
              </li>;
            })}
          </ul>}
          {rows.length === 0 && <div className="rounded-lg border border-dashed px-6 py-16 text-center">
            {remoteError
              ? <><p className="text-sm text-destructive">{remoteError}</p><Button className="mt-4" variant="outline" onClick={props.onRetryDiscovery}>{t("catalog:retry")}</Button></>
              : !props.githubConnected
                ? <><p>{t("catalog:connectForPrivate")}</p><Button className="mt-4" onClick={props.onConnectGitHub}>{t("github:connect")}</Button></>
                : <p>{t("catalog:noPullRequests")}</p>}
          </div>}
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
