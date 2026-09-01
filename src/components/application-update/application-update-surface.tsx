import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { openUrl } from "@tauri-apps/plugin-opener";
import { useTranslation } from "react-i18next";
import { useRef } from "react";

import type { ApplicationUpdateProgress } from "@/application-update/application-update";
import type { ApplicationUpdateBinding } from "@/application-update/use-application-update";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Progress, ProgressLabel, ProgressValue } from "@/components/ui/progress";

/** 只允许交给系统浏览器的普通 Web 链接。 */
function isSafeWebUrl(url?: string): url is string {
  if (!url) return false;
  try {
    return ["https:", "http:"].includes(new URL(url).protocol);
  } catch {
    return false;
  }
}

/** 把字节数格式化为紧凑、稳定且不依赖后端文本的显示值。 */
function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  if (value < 1024 ** 2) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 ** 2).toFixed(1)} MB`;
}

/** 呈现确定或不确定下载进度；unknown total 不伪造百分比。 */
function DownloadProgress({ progress }: { progress: ApplicationUpdateProgress }) {
  const { t } = useTranslation("applicationUpdate");
  if (progress.kind === "unknown_total") {
    return <div role="status" className="space-y-2" aria-label={t("progress.indeterminateLabel")}>
      <div className="flex items-center justify-between gap-3 text-sm">
        <span className="font-medium">{t("progress.downloading")}</span>
        <span className="tabular-nums text-muted-foreground">{formatBytes(progress.downloadedBytes)}</span>
      </div>
      <div className="h-3 overflow-hidden rounded-full bg-muted">
        <div className="h-full w-1/3 animate-pulse rounded-full bg-primary" />
      </div>
    </div>;
  }
  const maximum = Math.max(progress.totalBytes, 1);
  const value = Math.min(progress.downloadedBytes, maximum);
  const percentage = Math.round((value / maximum) * 100);
  return <Progress value={value} max={maximum} aria-label={t("progress.determinateLabel", { percentage })}>
    <ProgressLabel>{t("progress.downloading")}</ProgressLabel>
    <ProgressValue>{() => t("progress.known", { percentage, downloaded: formatBytes(value), total: formatBytes(progress.totalBytes) })}</ProgressValue>
  </Progress>;
}

/** 安全呈现 GFM Release Notes；忽略原始 HTML、阻止图片加载并过滤危险链接。 */
function ReleaseNotes({ notes }: { notes: string }) {
  const { t } = useTranslation("applicationUpdate");
  if (!notes.trim()) return <p className="text-sm text-muted-foreground">{t("surface.noNotes")}</p>;
  return <div className="review-markdown min-w-0 break-words text-sm">
    <ReactMarkdown remarkPlugins={[remarkGfm]} skipHtml components={{
      img: ({ alt }) => <span className="text-muted-foreground">{alt ? t("surface.blockedImageWithAlt", { alt }) : t("surface.blockedImage")}</span>,
      a: ({ href, children }) => isSafeWebUrl(href)
        ? <a href={href} onClick={(event) => { event.preventDefault(); void openUrl(href); }}>{children}</a>
        : <span>{children}</span>,
    }}>{notes}</ReactMarkdown>
  </div>;
}

/** Application Update 的唯一应用内详情与操作弹层。 */
export function ApplicationUpdateSurface({ update }: { update: ApplicationUpdateBinding }) {
  const { t } = useTranslation("applicationUpdate");
  const laterButton = useRef<HTMLButtonElement>(null);
  const snapshot = update.snapshot;
  if (!snapshot) return null;
  const staged = snapshot.state.kind === "staged";
  const retryable = snapshot.state.kind === "failed" && snapshot.failure?.retryable;
  return <Dialog open={update.surfaceOpen} onOpenChange={update.setSurfaceOpen}>
    <DialogContent initialFocus={laterButton} className="max-h-[calc(100vh-2rem)] min-w-0 overflow-hidden sm:max-w-2xl" data-slot="application-update-dialog">
      <DialogHeader className="pr-8">
        <DialogTitle className="text-xl">{t("surface.title")}</DialogTitle>
        <DialogDescription>{snapshot.candidate
          ? t("surface.version", { version: snapshot.candidate.version })
          : t(`state.${snapshot.state.kind}`)}</DialogDescription>
      </DialogHeader>
      <div className="min-w-0 space-y-4 overflow-y-auto pr-1">
        {snapshot.state.kind === "downloading" && <DownloadProgress progress={snapshot.state.progress} />}
        {snapshot.failure && <p role="alert" className="rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">{t(`failure.${snapshot.failure.category}`)}</p>}
        {snapshot.recoveryWarning && <p role="status" className="rounded-lg border border-amber-500/30 bg-amber-500/5 p-3 text-sm text-amber-800 dark:text-amber-200">{t("warning.recovery")}</p>}
        <section aria-labelledby="application-update-release-notes" className="min-w-0 overflow-hidden rounded-xl border">
          <h3 id="application-update-release-notes" className="border-b bg-muted/40 px-4 py-3 font-semibold">{t("surface.releaseNotes")}</h3>
          <div className="max-h-[45vh] min-w-0 overflow-y-auto px-4 py-1">
            <ReleaseNotes notes={snapshot.candidate?.releaseNotes ?? ""} />
          </div>
        </section>
      </div>
      <DialogFooter>
        <Button ref={laterButton} variant="outline" onClick={() => update.setSurfaceOpen(false)}>{t("surface.later")}</Button>
        {retryable && <Button disabled={Boolean(update.pending)} onClick={() => void update.retry()}>{t("action.retry")}</Button>}
        {staged && <Button disabled={Boolean(update.pending)} onClick={() => void update.restart()}>{t("surface.restart")}</Button>}
      </DialogFooter>
    </DialogContent>
  </Dialog>;
}
