import { DownloadCircle01Icon } from "@hugeicons/core-free-icons";
import { useTranslation } from "react-i18next";

import type { ApplicationUpdateBinding } from "@/application-update/use-application-update";
import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";

/** Application Settings 中的应用更新分区。 */
export function ApplicationUpdateSettings({ update }: { update: ApplicationUpdateBinding }) {
  const { t } = useTranslation("applicationUpdate");
  const snapshot = update.snapshot;
  if (!snapshot) return <section aria-labelledby="application-update-settings-title">
    <h2 id="application-update-settings-title" className="font-heading text-xl font-semibold">{t("settings.title")}</h2>
    <p role={update.commandError === "load" ? "alert" : "status"} className="mt-4 text-sm text-muted-foreground">
      {update.commandError === "load" ? t("settings.loadFailed") : t("settings.loading")}
    </p>
  </section>;

  const preferenceBusy = update.pending === "preference";
  const checking = snapshot.state.kind === "checking" || update.pending === "manual-check";
  return <section aria-labelledby="application-update-settings-title" className="space-y-7">
    <div className="flex items-center gap-3">
      <Icon icon={DownloadCircle01Icon} className="size-5" aria-hidden />
      <div>
        <h2 id="application-update-settings-title" className="font-heading text-xl font-semibold">{t("settings.title")}</h2>
        <p className="text-sm text-muted-foreground">{t("settings.description")}</p>
      </div>
    </div>
    <dl className="grid gap-3 text-sm sm:grid-cols-[10rem_minmax(0,1fr)]">
      <dt className="text-muted-foreground">{t("settings.currentVersion")}</dt>
      <dd className="min-w-0 break-words font-medium">{snapshot.currentVersion}</dd>
      <dt className="text-muted-foreground">{t("settings.status")}</dt>
      <dd className="min-w-0 break-words font-medium" role="status">{t(`state.${snapshot.state.kind}`)}</dd>
      {snapshot.candidate && <>
        <dt className="text-muted-foreground">{t("settings.availableVersion")}</dt>
        <dd className="min-w-0 break-words font-medium">{snapshot.candidate.version}</dd>
      </>}
    </dl>
    <div className="flex items-start justify-between gap-5 rounded-xl border p-4">
      <div className="min-w-0">
        <span id="application-update-automatic-download-label" className="block font-medium">{t("settings.automaticDownload")}</span>
        <span id="application-update-automatic-download-description" className="mt-1 block text-sm text-muted-foreground">{t("settings.automaticDownloadDescription")}</span>
      </div>
      <Switch checked={snapshot.automaticDownload} disabled={preferenceBusy} onCheckedChange={(checked) => void update.setAutomaticDownload(checked)} aria-labelledby="application-update-automatic-download-label" aria-describedby="application-update-automatic-download-description" />
    </div>
    {snapshot.failure && <p role="alert" className="rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">
      {t(`failure.${snapshot.failure.category}`)}
    </p>}
    {snapshot.recoveryWarning && <p role="status" className="rounded-lg border border-amber-500/30 bg-amber-500/5 p-3 text-sm text-amber-800 dark:text-amber-200">{t("warning.recovery")}</p>}
    {snapshot.preferenceWarning && <p role="status" className="rounded-lg border border-amber-500/30 bg-amber-500/5 p-3 text-sm text-amber-800 dark:text-amber-200">{t(`warning.preference.${snapshot.preferenceWarning.kind}`)}</p>}
    {update.commandError && update.commandError !== "load" && <p role="alert" className="text-sm text-destructive">{t(`commandError.${update.commandError}`)}</p>}
    <div className="flex flex-wrap gap-3">
      <Button disabled={checking} onClick={() => void update.manualCheck()}>
        {checking && <Spinner aria-hidden />}{checking ? t("action.checking") : t("action.check")}
      </Button>
      {snapshot.failure?.retryable && <Button variant="outline" disabled={Boolean(update.pending)} onClick={() => void update.retry()}>{t("action.retry")}</Button>}
      {snapshot.candidate && <Button variant="outline" onClick={update.openSurface}>{t("action.details")}</Button>}
    </div>
  </section>;
}
