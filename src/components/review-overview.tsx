import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { useTranslation } from "react-i18next";
import { RiExternalLinkLine, RiInformationLine } from "@remixicon/react";

import { Button } from "@/components/ui/button";
import {
  Sheet,
  SheetClose,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
  SheetTrigger,
} from "@/components/ui/sheet";
import type { PullRequestMetadata } from "@/lib/review-types";

/** Review 概览所需的来源无关 Snapshot 投影。 */
export type ReviewOverviewModel = {
  baseRef: string;
  baseRevision: string;
  headRef: string;
  headRevision: string;
  pullRequest?: PullRequestMetadata;
};

/** Review 概览组件的公开交互参数。 */
type ReviewOverviewProps = {
  review: ReviewOverviewModel;
  onOpenExternal: (url: string) => void;
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

/** 将 Review 背景放进默认关闭的右侧 Sheet，避免长期占用 Diff 空间。 */
export function ReviewOverview({ review, onOpenExternal }: ReviewOverviewProps) {
  const { t } = useTranslation("review");
  const pullRequest = review.pullRequest;

  /** 阻止 WebView 导航，仅允许系统打开 HTTP/HTTPS 链接。 */
  function openExternal(event: React.MouseEvent<HTMLAnchorElement>, url?: string) {
    event.preventDefault();
    if (isExternalWebUrl(url)) onOpenExternal(url);
  }

  return (
    <Sheet>
      <SheetTrigger render={<Button size="xs" variant="outline" />}>
        <RiInformationLine />
        {t("openOverview")}
      </SheetTrigger>
      <SheetContent
        side="right"
        showCloseButton={false}
        aria-label={t("overview")}
        className="w-[min(90vw,30rem)] overflow-y-auto sm:max-w-[30rem]"
      >
        <SheetHeader className="border-b">
          <div className="flex items-start justify-between gap-4 pr-1">
            <div className="min-w-0">
              <SheetTitle>{t("overview")}</SheetTitle>
              <SheetDescription>
                {pullRequest ? t("pullRequestOverviewDescription") : t("branchOverviewDescription")}
              </SheetDescription>
            </div>
            <SheetClose render={<Button size="icon-sm" variant="ghost" aria-label={t("closeOverview")} />}>
              ×
            </SheetClose>
          </div>
        </SheetHeader>

        <div className="space-y-6 p-6">
          <section aria-labelledby="review-overview-identity" className="space-y-3">
            <h3 id="review-overview-identity" className="font-heading text-base font-medium">
              {pullRequest ? pullRequest.title : t("localBranchReview")}
            </h3>
            {pullRequest && (
              <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2 text-sm">
                <dt className="text-muted-foreground">{t("pullRequest")}</dt>
                <dd>#{pullRequest.number}</dd>
                <dt className="text-muted-foreground">{t("author")}</dt>
                <dd>{pullRequest.author}</dd>
                <dt className="text-muted-foreground">{t("status")}</dt>
                <dd>{pullRequest.draft ? `${t("draft")} · ` : ""}{t(`state.${pullRequest.state}`)}</dd>
              </dl>
            )}
          </section>

          <section aria-labelledby="review-overview-revisions" className="space-y-3">
            <h3 id="review-overview-revisions" className="text-sm font-semibold">{t("revisions")}</h3>
            <dl className="space-y-4 text-sm">
              <div>
                <dt className="text-xs text-muted-foreground">Base</dt>
                <dd className="mt-1 break-all">{review.baseRef}</dd>
                <dd className="mt-1 break-all font-mono text-xs">{review.baseRevision}</dd>
              </div>
              <div>
                <dt className="text-xs text-muted-foreground">Head</dt>
                <dd className="mt-1 break-all">{review.headRef}</dd>
                <dd className="mt-1 break-all font-mono text-xs">{review.headRevision}</dd>
              </div>
            </dl>
          </section>

          {pullRequest && (
            <>
              <section aria-labelledby="review-overview-description" className="space-y-3">
                <h3 id="review-overview-description" className="text-sm font-semibold">{t("description")}</h3>
                <div className="prose prose-sm max-w-none break-words dark:prose-invert">
                  <ReactMarkdown
                    remarkPlugins={[remarkGfm]}
                    skipHtml
                    components={{
                      img: ({ alt }) => (
                        <span className="text-muted-foreground">
                          {alt ? t("blockedRemoteImageWithAlt", { alt }) : t("blockedRemoteImage")}
                        </span>
                      ),
                      a: ({ href, children }) => isExternalWebUrl(href)
                        ? <a href={href} onClick={(event) => openExternal(event, href)}>{children}</a>
                        : <span>{children}</span>,
                    }}
                  >
                    {pullRequest.description}
                  </ReactMarkdown>
                </div>
              </section>
              <a
                href={pullRequest.url}
                className="inline-flex items-center gap-2 text-sm font-medium text-primary underline-offset-4 hover:underline"
                onClick={(event) => openExternal(event, pullRequest.url)}
              >
                {t("viewOnGitHub")}
                <RiExternalLinkLine />
              </a>
            </>
          )}
        </div>
      </SheetContent>
    </Sheet>
  );
}
