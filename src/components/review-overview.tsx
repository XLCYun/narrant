import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useTranslation } from "react-i18next";
import { ExternalLinkIcon, GitMergeIcon, GitPullRequestClosedIcon, GitPullRequestDraftIcon, GitPullRequestIcon } from "@hugeicons/core-free-icons";

import { branchName } from "@/components/branch-relation";
import { CopyableReference } from "@/components/copyable-reference";
import { Badge } from "@/components/ui/badge";
import { Icon } from "@/components/ui/icon";
import { cn } from "@/lib/utils";
import type { PullRequestMetadata } from "@/lib/review-types";

/** Review 概览所需的来源无关 Snapshot 投影。 */
export type ReviewOverviewModel = {
  baseRef: string;
  baseRevision: string;
  headRef: string;
  headRevision: string;
  pullRequest?: PullRequestMetadata;
};

/** 在一个标签中组合可独立复制的用户分支名与精确 revision。 */
function RevisionReference({ branch, revision }: { branch: string; revision: string }) {
  const { t } = useTranslation("review");
  const safeRevision = revision ?? "";
  const referenceClassName = "cursor-pointer rounded-sm focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";
  return <span className="flex min-w-0 max-w-full items-center gap-2 rounded-lg border bg-muted/60 px-2.5 py-1.5 shadow-xs">
    <CopyableReference value={branch} copiedLabel={t("branchCopied")} copyFailedLabel={t("referenceCopyFailed")} className={cn(referenceClassName, "min-w-0 flex-1 truncate text-left font-mono text-sm text-foreground hover:text-primary")}>{branch}</CopyableReference>
    <CopyableReference value={safeRevision} copiedLabel={t("revisionCopied")} copyFailedLabel={t("referenceCopyFailed")} className={cn(referenceClassName, "shrink-0 font-mono text-xs text-muted-foreground hover:text-foreground")}>{safeRevision.slice(0, 7) || "—"}</CopyableReference>
  </span>;
}

/** 判断 Markdown 链接是否可交给系统默认浏览器。 */
function isExternalWebUrl(url?: string): url is string {
  if (!url) return false;
  try {
    const protocol = new URL(url).protocol;
    return protocol === "https:" || protocol === "http:";
  } catch {
    return false;
  }
}

/** 概览标签的单栏内容，安全呈现协作 metadata 与 Markdown 描述。 */
export function ReviewOverview({ review, onOpenExternal }: { review: ReviewOverviewModel; onOpenExternal: (url: string) => void }) {
  const { t } = useTranslation("review");
  const pullRequest = review.pullRequest;

  /** 阻止 WebView 导航，仅允许系统打开 HTTP/HTTPS 链接。 */
  function openExternal(event: React.MouseEvent<HTMLAnchorElement>, url?: string) {
    event.preventDefault();
    if (isExternalWebUrl(url)) onOpenExternal(url);
  }

  /** 打开已阻止加载的远程图片，保持应用内不自动请求第三方资源。 */
  function openRemoteImage(url?: string) {
    if (isExternalWebUrl(url)) onOpenExternal(url);
  }

  return <article className="mx-auto w-full max-w-3xl space-y-8 px-5 py-7 sm:px-8">
    <header className="space-y-3">
      <h2 className="text-xl font-semibold tracking-tight sm:text-2xl">{pullRequest?.title ?? t("localBranchReview")}</h2>
      {pullRequest && <div className="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
        {(() => {
          const state: "draft" | PullRequestMetadata["state"] = pullRequest.draft ? "draft" : pullRequest.state;
          const icons = { draft: GitPullRequestDraftIcon, open: GitPullRequestIcon, closed: GitPullRequestClosedIcon, merged: GitMergeIcon };
          const colors = { draft: "bg-amber-500/10 text-amber-700 dark:text-amber-300", open: "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300", closed: "bg-muted text-muted-foreground", merged: "bg-violet-500/10 text-violet-700 dark:text-violet-300" };
          return <Badge variant="secondary" className={cn("border-0", colors[state])}><Icon icon={icons[state]} data-icon="inline-start" aria-hidden />{pullRequest.draft ? t("draft") : t(`state.${pullRequest.state}`)}</Badge>;
        })()}
        <span>#{pullRequest.number}</span>
        <span>{pullRequest.author}</span>
      </div>}
    </header>

    <section aria-labelledby="review-overview-revisions" className="space-y-3 border-y py-4">
      <h3 id="review-overview-revisions" className="text-sm font-semibold">{t("revisions")}</h3>
      <div aria-label={t("revisionRelationLabel", { head: branchName(review.headRef), base: branchName(review.baseRef) })} className="flex min-w-0 flex-col items-stretch gap-2 text-sm sm:flex-row sm:items-center">
        <span className="shrink-0 text-muted-foreground">{t("mergeSentenceStart")}</span>
        <RevisionReference branch={branchName(review.headRef)} revision={review.headRevision} />
        <span className="shrink-0 text-muted-foreground">{t("mergeSentenceJoin")}</span>
        <RevisionReference branch={branchName(review.baseRef)} revision={review.baseRevision} />
      </div>
    </section>

    {pullRequest && <section aria-labelledby="review-overview-description" className="space-y-4">
      <h3 id="review-overview-description" className="text-sm font-semibold">{t("description")}</h3>
      <div className="review-markdown break-words">
        <ReactMarkdown remarkPlugins={[remarkGfm]} skipHtml components={{
          img: ({ alt, src }) => <button type="button" className="text-left text-muted-foreground underline underline-offset-4 hover:text-foreground" onClick={() => openRemoteImage(src)}>{alt ? t("blockedRemoteImageWithAlt", { alt }) : t("blockedRemoteImage")}</button>,
          a: ({ href, children }) => isExternalWebUrl(href) ? <a href={href} onClick={(event) => openExternal(event, href)}>{children}</a> : <span>{children}</span>,
        }}>{pullRequest.description}</ReactMarkdown>
      </div>
      <a href={pullRequest.url} className="inline-flex items-center gap-2 text-sm font-medium text-primary underline-offset-4 hover:underline" onClick={(event) => openExternal(event, pullRequest.url)}>{t("viewOnGitHub")}<Icon icon={ExternalLinkIcon} /></a>
    </section>}
  </article>;
}
