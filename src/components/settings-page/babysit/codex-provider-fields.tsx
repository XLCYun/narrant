import { useEffect, useState } from "react";

import { Button } from "@/components/ui/button";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { ApplicationSettingsApplication, BabysitModelCatalogEntry } from "@/settings/application-settings";

import { errorDetail } from "./settings-error";
import { ProviderExecutableFields, type InstalledProviderFieldsProps } from "./provider-executable-fields";

/** Codex 编辑器消费动态模型目录，并保留目录 revision 约束。 */
export type CodexProviderFieldsProps = InstalledProviderFieldsProps & {
  application: ApplicationSettingsApplication;
  onError(detail: string): void;
};

/** 呈现 Codex 的延迟动态目录，并即时保存完整偏好组合。 */
export function CodexProviderFields({ application, preference, saving, onUpdate, onChooseExecutable, onError }: CodexProviderFieldsProps) {
  const [catalog, setCatalog] = useState<{ revision: string; models: BabysitModelCatalogEntry[] }>();
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [catalogError, setCatalogError] = useState<string>();
  useEffect(() => {
    let current = true;
    setCatalogLoading(true);
    setCatalogError(undefined);
    void application.listBabysitModels("codex").then((nextCatalog) => {
      if (current) setCatalog(nextCatalog);
    }).catch((reason) => {
      if (!current) return;
      const detail = errorDetail(reason, "无法获取模型目录");
      setCatalogError(detail);
      onError(detail);
    }).finally(() => { if (current) setCatalogLoading(false); });
    return () => { current = false; };
  }, [application, onError]);

  /** 目录读取失败后重新获取 Codex 的动态模型能力。 */
  async function loadCatalog() {
    setCatalogLoading(true);
    setCatalogError(undefined);
    try { setCatalog(await application.listBabysitModels("codex")); }
    catch (reason) {
      const detail = errorDetail(reason, "无法获取模型目录");
      setCatalogError(detail);
      onError(detail);
    } finally { setCatalogLoading(false); }
  }

  return <div className="grid min-w-0 gap-5" data-testid="codex-preferences">
    <ProviderExecutableFields label="Codex" preference={preference} saving={saving} onUpdate={onUpdate} onChooseExecutable={onChooseExecutable} />
    <label className="grid max-w-md gap-2 text-sm font-medium">模型<Select value={preference.model ?? "__default"} disabled={saving || !catalog} onValueChange={(value) => {
      if (!value || !catalog) return;
      const model = value === "__default" ? null : value;
      const supported = catalog.models.find((item) => item.id === model)?.supportedReasoningEfforts;
      const reasoningEffort = preference.reasoningEffort && supported?.includes(preference.reasoningEffort) ? preference.reasoningEffort : null;
      void onUpdate({ ...preference, model, reasoningEffort }, catalog.revision);
    }}><SelectTrigger aria-label="模型" className="h-11 w-full rounded-xl bg-background px-4"><SelectValue>{catalogLoading ? "正在获取模型…" : catalogError ? "暂时不可用" : preference.model === null ? "Default" : catalog?.models.find((model) => model.id === preference.model)?.name ?? preference.model}</SelectValue></SelectTrigger><SelectContent align="start" alignItemWithTrigger={false}><SelectItem value="__default">Default</SelectItem>{catalog?.models.map((model) => <SelectItem key={model.id} value={model.id}>{model.name}</SelectItem>)}</SelectContent></Select></label>
    <label className="grid max-w-md gap-2 text-sm font-medium">思考强度<Select value={preference.reasoningEffort ?? "default"} disabled={saving || !catalog || !preference.model} onValueChange={(value) => value && catalog && void onUpdate({ ...preference, reasoningEffort: value === "default" ? null : value }, catalog.revision)}><SelectTrigger aria-label="思考强度" className="h-11 w-full rounded-xl bg-background px-4"><SelectValue>{catalogLoading ? "正在获取能力…" : catalogError ? "暂时不可用" : preference.reasoningEffort === null ? "Default" : preference.reasoningEffort}</SelectValue></SelectTrigger><SelectContent align="start" alignItemWithTrigger={false}><SelectItem value="default">Default</SelectItem>{catalog?.models.find((model) => model.id === preference.model)?.supportedReasoningEfforts.map((effort) => <SelectItem key={effort} value={effort}>{effort}</SelectItem>)}</SelectContent></Select></label>
    {catalogLoading && <p role="status" className="text-sm text-muted-foreground">正在读取 Codex 模型目录…</p>}
    {catalogError && !catalogLoading && <div className="flex items-center gap-3"><p className="text-sm text-muted-foreground">已保留当前保存值。</p><Button variant="outline" size="sm" disabled={saving} onClick={() => void loadCatalog()}>重试获取模型</Button></div>}
    {catalog?.models.length === 0 && <p role="status" className="rounded-lg border p-3 text-sm text-muted-foreground">当前 Codex 未返回可用模型，已保留当前设置。</p>}
  </div>;
}
