import { useEffect, useRef, useState } from "react";
import { Settings02Icon } from "@hugeicons/core-free-icons";
import { useTranslation } from "react-i18next";

import { Icon } from "@/components/ui/icon";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { cn } from "@/lib/utils";
import type { ApplicationSettingsApplication, BabysitProvider, BabysitProviderPreference, BabysitSettings, CustomProviderConnection, CustomProviderSettings } from "@/settings/application-settings";

import { ClaudeCodeProviderFields } from "./claude-code-provider-fields";
import { CodexProviderFields } from "./codex-provider-fields";
import { CustomProviderFields } from "./custom-provider-fields";
import { errorDetail } from "./settings-error";

/** Babysit 设置模块的稳定页面 interface。 */
export type BabysitProviderSettingsProps = {
  application: ApplicationSettingsApplication;
  initialCustomApiKeyVisible?: boolean;
  autoSaveCustomConnection?: boolean;
};

/** 通过公共设置 port 编排 Provider 切换、完整保存和失败恢复。 */
export function BabysitProviderSettings({ application, initialCustomApiKeyVisible = false, autoSaveCustomConnection = false }: BabysitProviderSettingsProps) {
  const { t } = useTranslation("settings");
  const [saved, setSaved] = useState<BabysitSettings>();
  const [error, setError] = useState<string>();
  const [saving, setSaving] = useState(false);
  const [customConnection, setCustomConnection] = useState<CustomProviderConnection>();
  const [customLoading, setCustomLoading] = useState(false);
  const [customLoaded, setCustomLoaded] = useState(false);
  const autoSaveStarted = useRef(false);

  useEffect(() => {
    let current = true;
    void application.load().then((settings) => {
      if (!current) return;
      setSaved(settings.babysit);
      if (settings.babysit.provider === "custom") void loadCustomConnection(settings.babysit.custom);
    }).catch((reason) => current && setError(errorDetail(reason, "无法读取 Babysit 设置")));
    return () => { current = false; };
  // 进入页面只读取一次设置；Custom 连接在选中时通过独立 seam 获取。
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [application]);

  /** 组合非敏感快照与短暂 API key，兼容旧版本缺失 custom 字段。 */
  function connectionFromSettings(settings?: Partial<CustomProviderSettings> | null): CustomProviderConnection {
    return {
      protocol: settings?.protocol === "anthropic_messages" ? "anthropic_messages" : "openai_chat_completions",
      baseUrl: settings?.baseUrl ?? "",
      apiKey: "",
      model: settings?.model ?? "",
      effort: settings?.effort === "low" || settings?.effort === "medium" || settings?.effort === "high" ? settings.effort : "provider_default",
    };
  }

  /** 读取当前 Custom 完整连接；失败时保留可恢复的非敏感快照。 */
  async function loadCustomConnection(settings?: CustomProviderSettings | null) {
    setCustomLoading(true);
    setCustomLoaded(false);
    setCustomConnection(connectionFromSettings(settings));
    try {
      const connection = await application.loadCustomConnection();
      setCustomConnection({ ...connectionFromSettings(connection), apiKey: connection.apiKey ?? "" });
    } catch (reason) {
      setError(errorDetail(reason, t("custom.loadFailed")));
    } finally {
      setCustomLoading(false);
      setCustomLoaded(true);
    }
  }

  useEffect(() => {
    if (!autoSaveCustomConnection || autoSaveStarted.current || !saved || saved.provider !== "custom" || !customLoaded || !customConnection) return;
    autoSaveStarted.current = true;
    void saveCustomConnection(customConnection);
  // 该 effect 是显式 Preview seam，只在一次连接读取完成后触发自动保存。
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [autoSaveCustomConnection, customConnection, customLoaded, saved]);

  if (!saved) return <section aria-labelledby="babysit-title"><h2 id="babysit-title" className="font-heading text-xl font-semibold">宝宝模式</h2><p role={error ? "alert" : "status"} className="mt-4 text-sm text-muted-foreground">{error ?? "正在读取设置…"}</p></section>;
  const provider = saved.provider;
  const key = provider === "codex" ? "codex" : provider === "claude-code" ? "claudeCode" : undefined;
  const preference = key ? saved[key] : undefined;

  /** 乐观提交新快照，失败时恢复先前权威值并显示错误。 */
  async function commit(next: BabysitSettings, operation: () => Promise<{ babysit: BabysitSettings }>) {
    if (saving) return;
    const previous = saved!;
    setSaved(next); setSaving(true); setError(undefined);
    try {
      const result = await operation();
      setSaved(result.babysit);
    } catch (reason) {
      setSaved(previous);
      setError(`${errorDetail(reason, "无法保存 Babysit 设置")}，已恢复原设置。`);
    } finally { setSaving(false); }
  }

  /** 更新当前 Provider 的完整偏好，避免产生中间不一致状态。 */
  function updatePreference(nextPreference: BabysitProviderPreference, revision: string | null = null) {
    if (!key) return Promise.resolve();
    const next = { ...saved!, [key]: nextPreference };
    return commit(next, () => application.updateBabysitPreferences(provider, nextPreference, revision));
  }

  /** 保存 Custom 完整连接，并在失败时保留表单中的原始输入。 */
  async function saveCustomConnection(connection: CustomProviderConnection): Promise<void> {
    const next = normalizeCustomConnection(connection);
    const validation = validateCustomConnection(next, t);
    if (validation) { setError(validation); return; }
    setSaving(true);
    setError(undefined);
    try {
      const result = await application.saveCustomConnection(next);
      setSaved(result.babysit);
      setCustomConnection(next);
    } catch (reason) {
      setError(`${errorDetail(reason, t("custom.saveFailed"))} ${t("custom.saveFailedPreserved")}`);
    } finally { setSaving(false); }
  }

  /** 选择并保存当前 Provider 的 executable，失败时恢复权威快照。 */
  async function chooseExecutable() {
    if (!key || !preference) return;
    const previous = saved!;
    setSaving(true); setError(undefined);
    try {
      const result = await application.chooseBabysitExecutable(provider, preference);
      if (result?.babysit) setSaved(result.babysit);
    } catch (reason) {
      setSaved(previous);
      setError(`${errorDetail(reason, "无法保存可执行文件")}，已恢复原设置。`);
    } finally { setSaving(false); }
  }

  return <section aria-labelledby="babysit-title">
    <h2 id="babysit-title" className="font-heading text-2xl font-semibold">宝宝模式</h2>
    {error && <p role="alert" className="mt-4 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">{error}</p>}
    <div className="mt-7 grid min-w-0 gap-5" aria-busy={saving || undefined}>
      <label className="grid max-w-md gap-2 text-base font-medium">使用的 Agent
        <Select value={provider} disabled={saving} onValueChange={(value) => {
          if (!value) return;
          const nextProvider = value as BabysitProvider;
          void commit({ ...saved, provider: nextProvider }, async () => {
            const result = await application.updateBabysitProvider(nextProvider);
            if (nextProvider === "custom") await loadCustomConnection(result.babysit.custom);
            return result;
          });
        }}><SelectTrigger aria-label="使用的 Agent" className="h-16! w-full rounded-2xl bg-background px-4 text-base"><SelectValue><ProviderIdentity provider={provider} /></SelectValue></SelectTrigger><SelectContent align="start" alignItemWithTrigger={false}><SelectItem value="codex"><ProviderIdentity provider="codex" /></SelectItem><SelectItem value="claude-code"><ProviderIdentity provider="claude-code" /></SelectItem><SelectItem value="custom"><ProviderIdentity provider="custom" /></SelectItem></SelectContent></Select>
      </label>
      {provider === "codex" && preference
        ? <CodexProviderFields application={application} preference={preference} saving={saving} onUpdate={updatePreference} onChooseExecutable={chooseExecutable} onError={setError} />
        : provider === "claude-code" && preference
          ? <ClaudeCodeProviderFields preference={preference} saving={saving} onUpdate={updatePreference} onChooseExecutable={chooseExecutable} />
          : <CustomProviderFields connection={customConnection ?? connectionFromSettings(saved.custom)} loading={customLoading} saving={saving} initialApiKeyVisible={initialCustomApiKeyVisible} onChange={setCustomConnection} onSave={saveCustomConnection} />}
    </div>
  </section>;
}

/** 将后端返回的 Custom 快照收敛为表单需要的完整、可编辑值。 */
function normalizeCustomConnection(connection: CustomProviderConnection): CustomProviderConnection {
  return {
    protocol: connection.protocol === "anthropic_messages" ? "anthropic_messages" : "openai_chat_completions",
    baseUrl: connection.baseUrl.trim(),
    apiKey: connection.apiKey ?? "",
    model: connection.model.trim(),
    effort: connection.effort === "low" || connection.effort === "medium" || connection.effort === "high" ? connection.effort : "provider_default",
  };
}

/** 在一次完整提交前校验 Custom 必填字段。 */
function validateCustomConnection(connection: CustomProviderConnection, t: (key: string) => string): string | undefined {
  const missing: string[] = [];
  if (!connection.baseUrl) missing.push(t("custom.validationBaseUrl"));
  if (!connection.model) missing.push(t("custom.validationModel"));
  return missing.length > 0 ? missing.join(" ") : undefined;
}

/** 使用官方品牌资产呈现 Agent 身份。 */
function ProviderIdentity({ provider }: { provider: BabysitProvider }) {
  const { t } = useTranslation("settings");
  const codex = provider === "codex";
  const custom = provider === "custom";
  return <span className="flex items-center gap-3">
    <span className="flex size-10 shrink-0 items-center justify-center" aria-hidden="true">
      {custom ? <Icon icon={Settings02Icon} className="size-7 text-primary" /> : <img src={codex ? "/icons/agents/codex.svg" : "/icons/agents/claude-code.svg"} alt="" className={cn("size-10 max-w-none object-contain", codex && "size-14 dark:invert")} />}
    </span>
    <span>{codex ? "Codex" : custom ? t("provider.custom") : "Claude Code"}</span>
  </span>;
}
