import { useState } from "react";
import { ViewIcon, ViewOffIcon } from "@hugeicons/core-free-icons";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import type { CustomProviderConnection } from "@/settings/application-settings";

/** Custom 编辑器的受控值、异步状态与完整保存 seam。 */
export type CustomProviderFieldsProps = {
  connection: CustomProviderConnection;
  loading: boolean;
  saving: boolean;
  initialApiKeyVisible?: boolean;
  onChange(connection: CustomProviderConnection): void;
  onSave(connection: CustomProviderConnection): Promise<void>;
};

/** 呈现 Custom 完整连接表单；API key 默认隐藏且只驻留组件状态。 */
export function CustomProviderFields({ connection, loading, saving, initialApiKeyVisible = false, onChange, onSave }: CustomProviderFieldsProps) {
  const { t } = useTranslation("settings");
  const [keyVisible, setKeyVisible] = useState(initialApiKeyVisible);
  const disabled = loading || saving;
  const update = <K extends keyof CustomProviderConnection>(field: K, value: CustomProviderConnection[K]) => {
    onChange({ ...connection, [field]: value });
  };

  return <form data-testid="custom-provider-editor" className="grid min-w-0 gap-5" onSubmit={(event) => {
    event.preventDefault();
    void onSave(connection);
  }}>
    <div className="rounded-2xl border bg-card p-4 sm:p-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div><h3 className="font-heading text-lg font-semibold">{t("custom.title")}</h3><p className="mt-1 max-w-2xl text-sm text-muted-foreground">{t("custom.description")}</p></div>
        {loading && <p role="status" className="text-sm text-muted-foreground">{t("custom.loading")}</p>}
      </div>
      <div className="mt-5 grid min-w-0 gap-5 sm:grid-cols-2">
        <label htmlFor="custom-protocol" className="grid min-w-0 gap-2 text-sm font-medium">{t("custom.protocol")}
          <Select value={connection.protocol} disabled={disabled} onValueChange={(value) => {
            if (value === "openai_chat_completions" || value === "anthropic_messages") update("protocol", value);
          }}><SelectTrigger id="custom-protocol" aria-label={t("custom.protocol")} className="h-11 w-full rounded-xl bg-background px-4"><SelectValue>{connection.protocol === "anthropic_messages" ? t("custom.anthropicMessages") : t("custom.openAiChatCompletions")}</SelectValue></SelectTrigger><SelectContent align="start" alignItemWithTrigger={false}><SelectItem value="openai_chat_completions">{t("custom.openAiChatCompletions")}</SelectItem><SelectItem value="anthropic_messages">{t("custom.anthropicMessages")}</SelectItem></SelectContent></Select>
        </label>
        <label htmlFor="custom-effort" className="grid min-w-0 gap-2 text-sm font-medium">{t("custom.effort")}
          <Select value={connection.effort} disabled={disabled} onValueChange={(value) => {
            if (value === "provider_default" || value === "low" || value === "medium" || value === "high") update("effort", value);
          }}><SelectTrigger id="custom-effort" aria-label={t("custom.effort")} className="h-11 w-full rounded-xl bg-background px-4"><SelectValue>{connection.effort === "provider_default" ? t("custom.providerDefault") : t(`custom.${connection.effort}`)}</SelectValue></SelectTrigger><SelectContent align="start" alignItemWithTrigger={false}><SelectItem value="provider_default">{t("custom.providerDefault")}</SelectItem><SelectItem value="low">{t("custom.low")}</SelectItem><SelectItem value="medium">{t("custom.medium")}</SelectItem><SelectItem value="high">{t("custom.high")}</SelectItem></SelectContent></Select>
          <span className="text-xs font-normal text-muted-foreground">{t("custom.effortHint")}</span>
        </label>
        <label htmlFor="custom-base-url" className="grid min-w-0 gap-2 text-sm font-medium sm:col-span-2">{t("custom.baseUrl")}
          <Input id="custom-base-url" aria-label={t("custom.baseUrl")} value={connection.baseUrl} placeholder={t("custom.baseUrlPlaceholder")} autoComplete="url" disabled={disabled} onChange={(event) => update("baseUrl", event.target.value)} />
        </label>
        <label htmlFor="custom-api-key" className="grid min-w-0 gap-2 text-sm font-medium sm:col-span-2">{t("custom.apiKey")}
          <div className="flex min-w-0 gap-2">
            <Input id="custom-api-key" data-testid="custom-api-key" aria-label={t("custom.apiKey")} type={keyVisible ? "text" : "password"} value={connection.apiKey} placeholder={t("custom.apiKeyPlaceholder")} autoComplete="new-password" disabled={disabled} onChange={(event) => update("apiKey", event.target.value)} />
            <Button type="button" variant="outline" size="icon" aria-label={keyVisible ? t("custom.hideApiKey") : t("custom.showApiKey")} aria-pressed={keyVisible} disabled={disabled} onClick={() => setKeyVisible((visible) => !visible)}><Icon icon={keyVisible ? ViewOffIcon : ViewIcon} aria-hidden /></Button>
          </div>
          <span className="text-xs font-normal text-muted-foreground">{t("custom.apiKeyHint")}</span>
        </label>
        <label htmlFor="custom-model" className="grid min-w-0 gap-2 text-sm font-medium sm:col-span-2">{t("custom.model")}
          <Input id="custom-model" aria-label={t("custom.model")} value={connection.model} placeholder={t("custom.modelPlaceholder")} autoComplete="off" disabled={disabled} onChange={(event) => update("model", event.target.value)} />
        </label>
      </div>
      <div className="mt-6"><Button type="submit" disabled={disabled}>{saving ? t("custom.saving") : t("custom.save")}</Button></div>
    </div>
  </form>;
}
