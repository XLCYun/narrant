import { useTranslation } from "react-i18next";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { Spinner } from "@/components/ui/spinner";
import { isPackActive, type PackId, type PackSnapshot } from "@/language-support/types";
import type { LanguageSupportBinding } from "@/language-support/use-language-support-packs";

/** 文件大小使用固定单位，下载总量未知时也能提供真实的已下载量。 */
function size(bytes: number) { return `${(bytes / 1024 / 1024).toFixed(1)} MB`; }

/** 同一逐包列表用于结构准备和设置管理；只有管理视图显示删除操作。 */
export function LanguageSupportPacks({ binding, ids, management = false, onRetry }: { binding: LanguageSupportBinding; ids?: PackId[]; management?: boolean; onRetry?: () => void }) {
  const { t } = useTranslation("languageSupport");
  const packs = binding.snapshot?.packs.filter(pack => management || (pack.required && (!ids || ids.includes(pack.id)))) ?? [];
  return <section className="w-full min-w-0 space-y-4 text-left" aria-label={t("title")}>
    <div><h2 className="font-heading text-lg font-semibold">{t("title")}</h2><p className="mt-1 text-sm text-muted-foreground">{t(management ? "description" : packs.some(pack => pack.phase === "failed") ? "preparationFailed" : "preparing")}</p></div>
    {binding.queryError && <div role="alert" className="flex flex-wrap items-center gap-2 text-sm text-destructive">{t("queryFailed")}<Button variant="outline" size="sm" onClick={binding.reload}>{t("retry")}</Button></div>}
    {binding.actionError && <p role="alert" className="whitespace-pre-wrap break-all text-sm text-destructive">{binding.actionError}</p>}
    {!binding.snapshot && !binding.queryError && <p className="flex items-center gap-2 text-sm text-muted-foreground"><Spinner />{t("loading")}</p>}
    <div className="space-y-3">{packs.map(pack => <PackRow key={`${pack.id}:${pack.version}`} pack={pack} binding={binding} management={management} onRetry={onRetry} />)}</div>
  </section>;
}

/** 仅下载阶段显示确定百分比，校验和安装使用阶段状态。 */
function PackRow({ pack, binding, management, onRetry }: { pack: PackSnapshot; binding: LanguageSupportBinding; management: boolean; onRetry?: () => void }) {
  const { t } = useTranslation("languageSupport");
  const active = isPackActive(pack);
  const name = ({ rust: "Rust", "typescript-javascript": "TypeScript / JavaScript", python: "Python" })[pack.id];
  const percentage = pack.totalBytes && pack.totalBytes > 0 ? Math.min(100, Math.floor(pack.downloadedBytes / pack.totalBytes * 100)) : null;
  return <article className="min-w-0 space-y-3 rounded-xl border bg-background/60 p-4" aria-label={`${name} ${pack.version}`}>
    <div className="flex min-w-0 flex-wrap items-start justify-between gap-2">
      <div className="min-w-0"><h3 className="break-words text-sm font-medium">{name}</h3><p className="mt-1 break-all text-xs text-muted-foreground">v{pack.version}{!pack.required && ` · ${t("previous")}`}</p></div>
      <span className="flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground">{active && <Spinner className="size-3" />}{t(`phase.${pack.phase}`)}</span>
    </div>
    {pack.phase === "downloading" && <div className="space-y-1.5">
      <Progress aria-label={t("downloadProgress", { name })} value={percentage} />
      <p className="text-xs tabular-nums text-muted-foreground">{size(pack.downloadedBytes)}{pack.totalBytes !== null && ` / ${size(pack.totalBytes)}`}{percentage !== null && ` · ${percentage}%`}</p>
    </div>}
    {pack.error && <p className="whitespace-pre-wrap break-all text-sm text-destructive">{pack.error}</p>}
    {(management || pack.phase === "failed") && <div className="flex flex-wrap items-center justify-between gap-2">
      {management && <span className="text-xs text-muted-foreground">{pack.installedBytes > 0 ? t("diskUsage", { size: size(pack.installedBytes) }) : t("onDemand")}{pack.inUse && ` · ${t("inUse")}`}</span>}
      <div className="ml-auto flex shrink-0 gap-2">
        {pack.phase === "failed" && pack.required && <Button size="sm" variant="outline" disabled={Boolean(binding.pending)} onClick={() => { if (onRetry) onRetry(); else void binding.retry(pack.id); }}>{t("retry")}</Button>}
        {management && (pack.installedBytes > 0 || !pack.required) && <Button size="sm" variant="outline" disabled={active || pack.inUse || Boolean(binding.pending)} onClick={() => { void binding.remove(pack.id, pack.version); }}>{t("remove")}</Button>}
      </div>
    </div>}
  </article>;
}

/** 离开结构页后仍提供可进入管理页的轻量后台状态。 */
export function LanguageSupportAttention({ binding, onOpen }: { binding: LanguageSupportBinding; onOpen: () => void }) {
  const { t } = useTranslation("languageSupport");
  const active = binding.snapshot?.packs.filter(isPackActive).length ?? 0;
  const failed = binding.snapshot?.packs.some(pack => pack.phase === "failed");
  if (!active && !failed && !binding.queryError) return null;
  return <Button variant="ghost" className="h-auto w-full min-w-0 justify-start gap-2 whitespace-normal py-2 text-left text-xs" onClick={onOpen}>
    {active > 0 && <Spinner className="size-3 shrink-0" />}<span>{t(binding.queryError ? "queryFailed" : active ? "background" : "backgroundFailed", { count: active })}</span>
  </Button>;
}
