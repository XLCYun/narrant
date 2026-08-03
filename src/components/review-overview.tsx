import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useTranslation } from "react-i18next";
import { ExternalLinkIcon } from "@hugeicons/core-free-icons";

import { branchName } from "@/components/branch-relation";
import { CopyableReference } from "@/components/copyable-reference";
import { Icon } from "@/components/ui/icon";
import type { PullRequestMetadata } from "@/lib/review-types";

/** Review 概览所需的来源无关 Snapshot 投影。 */
export type ReviewOverviewModel = {
  baseRef: string;
  baseRevision: string;
  headRef: string;
  headRevision: string;
  pullRequest?: PullRequestMetadata;
};

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

  const referenceClassName = "cursor-pointer rounded-md px-1 font-mono text-sm text-foreground hover:bg-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";
  return <article className="mx-auto w-full max-w-3xl space-y-8 px-5 py-7 sm:px-8">
    <header className="space-y-3">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-sm text-muted-foreground">
        {pullRequest && <><span>{t("pullRequest")} #{pullRequest.number}</span><span>{t("author")} {pullRequest.author}</span><span>{t("status")} {pullRequest.draft ? t("draft") : t(`state.${pullRequest.state}`)}</span></>}
        {!pullRequest && <span>{t("localBranchReview")}</span>}
      </div>
      <h3 className="text-xl font-semibold tracking-tight">{pullRequest?.title ?? t("localBranchReview")}</h3>
    </header>

    <section aria-labelledby="review-overview-revisions" className="space-y-3 border-y py-4">
      <h4 id="review-overview-revisions" className="text-sm font-semibold">{t("revisions")}</h4>
      <div className="flex flex-wrap items-center gap-x-2 gap-y-2 text-sm">
        <CopyableReference value={branchName(review.baseRef)} copiedLabel={t("branchCopied")} copyFailedLabel={t("referenceCopyFailed")} className={referenceClassName}>{branchName(review.baseRef)}</CopyableReference>
        <span className="text-muted-foreground">→</span>
        <CopyableReference value={branchName(review.headRef)} copiedLabel={t("branchCopied")} copyFailedLabel={t("referenceCopyFailed")} className={referenceClassName}>{branchName(review.headRef)}</CopyableReference>
      </div>
      <div className="flex flex-wrap items-center gap-x-2 gap-y-2 text-xs text-muted-foreground">
        <CopyableReference value={review.baseRevision} copiedLabel={t("revisionCopied")} copyFailedLabel={t("referenceCopyFailed")} className={referenceClassName}>{review.baseRevision.slice(0, 7)}</CopyableReference>
        <span>→</span>
        <CopyableReference value={review.headRevision} copiedLabel={t("revisionCopied")} copyFailedLabel={t("referenceCopyFailed")} className={referenceClassName}>{review.headRevision.slice(0, 7)}</CopyableReference>
      </div>
    </section>

    {pullRequest && <section aria-labelledby="review-overview-description" className="space-y-4">
      <h4 id="review-overview-description" className="text-sm font-semibold">{t("description")}</h4>
      <div className="prose prose-sm max-w-none break-words dark:prose-invert prose-pre:max-w-full prose-pre:overflow-x-auto prose-table:block prose-table:max-w-full prose-table:overflow-x-auto">
        <ReactMarkdown remarkPlugins={[remarkGfm]} skipHtml components={{
          img: ({ alt, src }) => <button type="button" className="text-left text-muted-foreground underline underline-offset-4 hover:text-foreground" onClick={() => openRemoteImage(src)}>{alt ? t("blockedRemoteImageWithAlt", { alt }) : t("blockedRemoteImage")}</button>,
          a: ({ href, children }) => isExternalWebUrl(href) ? <a href={href} onClick={(event) => openExternal(event, href)}>{children}</a> : <span>{children}</span>,
        }}>{pullRequest.description}</ReactMarkdown>
      </div>
      <a href={pullRequest.url} className="inline-flex items-center gap-2 text-sm font-medium text-primary underline-offset-4 hover:underline" onClick={(event) => openExternal(event, pullRequest.url)}>{t("viewOnGitHub")}<Icon icon={ExternalLinkIcon} /></a>
    </section>}
  </article>;
}
