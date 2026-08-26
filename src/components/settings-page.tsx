import { useEffect, useState } from "react";
import { ArrowLeft02Icon, CheckmarkCircle02Icon, ComputerIcon, GithubIcon, Moon02Icon, Settings02Icon, Sun03Icon, ViewIcon, ViewOffIcon } from "@hugeicons/core-free-icons";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Icon } from "@/components/ui/icon";
import { Input } from "@/components/ui/input";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { githubIdentityAvatarUrl } from "@/components/github-identity-entry";
import type { GitHubIdentityLifecycleBinding } from "@/github/use-github-identity-lifecycle";
import type { ApplicationSettingsApplication, BabysitModelCatalogEntry, BabysitProvider, BabysitProviderPreference, BabysitSettings, CustomProviderConnection, CustomProviderSettings, ThemePreference } from "@/settings/application-settings";
import type { SettingsSection } from "@/settings/settings-section";
import { useApplicationTheme, type ApplicationThemeBinding } from "@/settings/use-application-theme";
import { cn } from "@/lib/utils";
import "@/lib/i18n";

const sections: Array<{ id: SettingsSection; label: string }> = [
  { id: "appearance", label: "外观" },
  { id: "github", label: "GitHub" },
  { id: "babysit", label: "宝宝模式" },
];

const themes: Array<{ value: ThemePreference; label: string; description: string; icon: typeof ComputerIcon }> = [
  { value: "system", label: "跟随系统", description: "随操作系统外观自动切换", icon: ComputerIcon },
  { value: "light", label: "浅色", description: "始终使用明亮外观", icon: Sun03Icon },
  { value: "dark", label: "深色", description: "始终使用深色外观", icon: Moon02Icon },
];

/** 独立 Application Settings 目的地，提供可扩展二级导航和主题偏好。 */
export function SettingsPage({ theme, application, githubIdentity, initialSection = "appearance", initialGitHubSignOutConfirmation = false, initialCustomApiKeyVisible = false, onBack }: { theme: ApplicationThemeBinding; application: ApplicationSettingsApplication; githubIdentity?: GitHubIdentityLifecycleBinding; initialSection?: SettingsSection; initialGitHubSignOutConfirmation?: boolean; initialCustomApiKeyVisible?: boolean; onBack(): void }) {
  const [section, setSection] = useState<SettingsSection>(initialSection);
  return <section aria-label="应用设置" className="mx-auto w-full max-w-[100rem] py-4 sm:py-8">
    <header className="mb-8 flex items-center gap-3 lg:mb-12">
      <Button variant="ghost" size="icon" aria-label="返回" onClick={onBack}><Icon icon={ArrowLeft02Icon} /></Button>
      <div><h1 className="font-heading text-3xl font-semibold">应用设置</h1><p className="text-base text-muted-foreground">管理此设备上的 Narrant 偏好</p></div>
    </header>
    <div className="grid gap-6 md:grid-cols-[15rem_minmax(0,1fr)] lg:gap-10">
      <nav role="tablist" aria-label="设置分区" className="flex gap-2 overflow-x-auto pb-1 md:flex-col md:pb-0">
        {sections.map((item) => <Button key={item.id} role="tab" aria-selected={section === item.id} variant={section === item.id ? "secondary" : "ghost"} className="h-11 shrink-0 justify-start text-base" onClick={() => setSection(item.id)}>{item.label}</Button>)}
      </nav>
      <div className="min-w-0 rounded-2xl border bg-card p-5 text-card-foreground sm:p-7 lg:min-h-[40rem] lg:p-9">
        {section === "appearance" && <section aria-labelledby="appearance-title">
          <div className="flex items-center gap-3"><Icon icon={Settings02Icon} className="size-5" /><h2 id="appearance-title" className="font-heading text-xl font-semibold">外观</h2></div>
          <p className="mt-1 text-sm text-muted-foreground">主题会同时应用到应用框架、代码阅读器与品牌图像。</p>
          {theme.error && <p role="alert" className="mt-4 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">{theme.error}</p>}
          <div role="radiogroup" aria-label="主题" aria-busy={!theme.loaded || undefined} className="mt-6 grid gap-3 sm:grid-cols-3">
            {themes.map((item) => <button key={item.value} type="button" role="radio" aria-checked={theme.preference === item.value} disabled={!theme.loaded || theme.saving} className={cn("rounded-xl border p-4 text-left outline-none transition-colors hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring", theme.preference === item.value && "border-primary bg-primary/5 ring-1 ring-primary")} onClick={() => void theme.choose(item.value)}>
              <Icon icon={item.icon} className="mb-3 size-5" /><strong className="block text-sm">{item.label}</strong><span className="mt-1 block text-xs text-muted-foreground">{item.description}</span>
            </button>)}
          </div>
        </section>}
        {section === "github" && (githubIdentity
          ? <GitHubSettings identity={githubIdentity} initiallyConfirming={initialGitHubSignOutConfirmation} />
          : <section><div className="flex items-center gap-3"><Icon icon={GithubIcon} className="size-5" /><h2 className="font-heading text-xl font-semibold">GitHub</h2></div><p className="mt-4 text-sm text-muted-foreground">GitHub Identity 设置将在后续版本中提供。</p></section>)}
        {section === "babysit" && <BabysitProviderSettings application={application} initialCustomApiKeyVisible={initialCustomApiKeyVisible} />}
      </div>
    </div>
  </section>;
}

/** 为测试与 Preview 提供从公共 Application Settings 端口开始的完整目的地入口。 */
export function ApplicationSettingsDestination({ application, githubIdentity, initialSection, initialGitHubSignOutConfirmation, initialCustomApiKeyVisible, onBack }: { application: ApplicationSettingsApplication; githubIdentity?: GitHubIdentityLifecycleBinding; initialSection?: SettingsSection; initialGitHubSignOutConfirmation?: boolean; initialCustomApiKeyVisible?: boolean; onBack(): void }) {
  const theme = useApplicationTheme(application);
  return <SettingsPage theme={theme} application={application} githubIdentity={githubIdentity} initialSection={initialSection} initialGitHubSignOutConfirmation={initialGitHubSignOutConfirmation} initialCustomApiKeyVisible={initialCustomApiKeyVisible} onBack={onBack} />;
}

const claudeModels = [
  { value: "", label: "Default" }, { value: "best", label: "Best（使用最强的可用模型）" },
  { value: "fable", label: "Fable" }, { value: "opus", label: "Opus" },
  { value: "sonnet", label: "Sonnet" }, { value: "haiku", label: "Haiku" }, { value: "custom", label: "Custom" },
];
const claudeEfforts = ["low", "medium", "high", "xhigh", "max"];

/** 通过公共设置端口呈现当前 provider，并对每次选择执行完整组合原子保存。 */
function BabysitProviderSettings({ application, initialCustomApiKeyVisible = false }: { application: ApplicationSettingsApplication; initialCustomApiKeyVisible?: boolean }) {
  const { t } = useTranslation("settings");
  const [saved, setSaved] = useState<BabysitSettings>();
  const [error, setError] = useState<string>();
  const [saving, setSaving] = useState(false);
  const [customConnection, setCustomConnection] = useState<CustomProviderConnection>();
  const [customLoading, setCustomLoading] = useState(false);

  useEffect(() => {
    let current = true;
    void application.load().then((settings) => {
      if (!current) return;
      setSaved(settings.babysit);
      if (settings.babysit.provider === "custom") {
        void loadCustomConnection(settings.babysit.custom);
      }
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
      effort: settings?.effort === "low" || settings?.effort === "high" ? settings.effort : "medium",
    };
  }

  /** 读取当前 Custom 完整连接；读取失败时保留编辑器可恢复的非敏感快照。 */
  async function loadCustomConnection(settings?: CustomProviderSettings | null) {
    setCustomLoading(true);
    setCustomConnection(connectionFromSettings(settings));
    try {
      const connection = await application.loadCustomConnection();
      setCustomConnection({ ...connectionFromSettings(connection), apiKey: connection.apiKey ?? "" });
    } catch (reason) {
      setError(errorDetail(reason, t("custom.loadFailed")));
    } finally {
      setCustomLoading(false);
    }
  }

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

  /** 更新当前 provider 的完整偏好，避免产生中间不一致状态。 */
  function updatePreference(nextPreference: BabysitProviderPreference, revision: string | null = null) {
    if (!key) return Promise.resolve();
    const next = { ...saved!, [key]: nextPreference };
    return commit(next, () => application.updateBabysitPreferences(provider, nextPreference, revision));
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
          : <CustomProviderFields
            connection={customConnection ?? connectionFromSettings(saved.custom)}
            loading={customLoading}
            saving={saving}
            initialApiKeyVisible={initialCustomApiKeyVisible}
            onChange={setCustomConnection}
            onSave={saveCustomConnection}
          />}
    </div>
  </section>;

  /** 保存 Custom 完整连接，并在失败时保留表单中的原始输入。 */
  async function saveCustomConnection(connection: CustomProviderConnection): Promise<void> {
    const next = normalizeCustomConnection(connection);
    const validation = validateCustomConnection(next, t);
    if (validation) {
      setError(validation);
      return;
    }
    setSaving(true);
    setError(undefined);
    try {
      const result = await application.saveCustomConnection(next);
      setSaved(result.babysit);
      setCustomConnection(next);
    } catch (reason) {
      setError(`${errorDetail(reason, t("custom.saveFailed"))} ${t("custom.saveFailedPreserved")}`);
    } finally {
      setSaving(false);
    }
  }

  /** 选择并保存当前 provider 的 executable，失败时恢复加载得到的权威快照。 */
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
}

/** 将后端返回的 Custom 快照收敛为表单需要的完整、可编辑值。 */
function normalizeCustomConnection(connection: CustomProviderConnection): CustomProviderConnection {
  return {
    protocol: connection.protocol === "anthropic_messages" ? "anthropic_messages" : "openai_chat_completions",
    baseUrl: connection.baseUrl.trim(),
    apiKey: connection.apiKey ?? "",
    model: connection.model.trim(),
    effort: connection.effort === "low" || connection.effort === "high" ? connection.effort : "medium",
  };
}

/** 在一次完整提交前校验 Custom 必填字段，避免产生半成品连接。 */
function validateCustomConnection(connection: CustomProviderConnection, t: (key: string) => string): string | undefined {
  const missing: string[] = [];
  if (!connection.baseUrl) missing.push(t("custom.validationBaseUrl"));
  if (!connection.model) missing.push(t("custom.validationModel"));
  return missing.length > 0 ? missing.join(" ") : undefined;
}

/** Custom 编辑器的受控值、异步状态与完整保存 seam。 */
type CustomProviderFieldsProps = {
  connection: CustomProviderConnection;
  loading: boolean;
  saving: boolean;
  initialApiKeyVisible?: boolean;
  onChange(connection: CustomProviderConnection): void;
  onSave(connection: CustomProviderConnection): Promise<void>;
};

/** 呈现 Custom 完整连接表单；API key 默认隐藏且只驻留组件状态。 */
function CustomProviderFields({ connection, loading, saving, initialApiKeyVisible = false, onChange, onSave }: CustomProviderFieldsProps) {
  const { t } = useTranslation("settings");
  const [keyVisible, setKeyVisible] = useState(initialApiKeyVisible);
  const disabled = loading || saving;
  const update = <K extends keyof CustomProviderConnection>(field: K, value: CustomProviderConnection[K]) => {
    onChange({ ...connection, [field]: value });
  };

  return <form
    data-testid="custom-provider-editor"
    className="grid min-w-0 gap-5"
    onSubmit={(event) => {
      event.preventDefault();
      void onSave(connection);
    }}
  >
    <div className="rounded-2xl border bg-card p-4 sm:p-5">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className="font-heading text-lg font-semibold">{t("custom.title")}</h3>
          <p className="mt-1 max-w-2xl text-sm text-muted-foreground">{t("custom.description")}</p>
        </div>
        {loading && <p role="status" className="text-sm text-muted-foreground">{t("custom.loading")}</p>}
      </div>
      <div className="mt-5 grid min-w-0 gap-5 sm:grid-cols-2">
        <label htmlFor="custom-protocol" className="grid min-w-0 gap-2 text-sm font-medium">
          {t("custom.protocol")}
          <Select value={connection.protocol} disabled={disabled} onValueChange={(value) => {
            if (value === "openai_chat_completions" || value === "anthropic_messages") update("protocol", value);
          }}>
            <SelectTrigger id="custom-protocol" aria-label={t("custom.protocol")} className="h-11 w-full rounded-xl bg-background px-4">
              <SelectValue>{connection.protocol === "anthropic_messages" ? t("custom.anthropicMessages") : t("custom.openAiChatCompletions")}</SelectValue>
            </SelectTrigger>
            <SelectContent align="start" alignItemWithTrigger={false}>
              <SelectItem value="openai_chat_completions">{t("custom.openAiChatCompletions")}</SelectItem>
              <SelectItem value="anthropic_messages">{t("custom.anthropicMessages")}</SelectItem>
            </SelectContent>
          </Select>
        </label>
        <label htmlFor="custom-effort" className="grid min-w-0 gap-2 text-sm font-medium">
          {t("custom.effort")}
          <Select value={connection.effort} disabled={disabled} onValueChange={(value) => {
            if (value === "low" || value === "medium" || value === "high") update("effort", value);
          }}>
            <SelectTrigger id="custom-effort" aria-label={t("custom.effort")} className="h-11 w-full rounded-xl bg-background px-4">
              <SelectValue>{t(`custom.${connection.effort}`)}</SelectValue>
            </SelectTrigger>
            <SelectContent align="start" alignItemWithTrigger={false}>
              <SelectItem value="low">{t("custom.low")}</SelectItem>
              <SelectItem value="medium">{t("custom.medium")}</SelectItem>
              <SelectItem value="high">{t("custom.high")}</SelectItem>
            </SelectContent>
          </Select>
        </label>
        <label htmlFor="custom-base-url" className="grid min-w-0 gap-2 text-sm font-medium sm:col-span-2">
          {t("custom.baseUrl")}
          <Input
            id="custom-base-url"
            aria-label={t("custom.baseUrl")}
            value={connection.baseUrl}
            placeholder={t("custom.baseUrlPlaceholder")}
            autoComplete="url"
            disabled={disabled}
            onChange={(event) => update("baseUrl", event.target.value)}
          />
        </label>
        <label htmlFor="custom-api-key" className="grid min-w-0 gap-2 text-sm font-medium sm:col-span-2">
          {t("custom.apiKey")}
          <div className="flex min-w-0 gap-2">
            <Input
              id="custom-api-key"
              data-testid="custom-api-key"
              aria-label={t("custom.apiKey")}
              type={keyVisible ? "text" : "password"}
              value={connection.apiKey}
              placeholder={t("custom.apiKeyPlaceholder")}
              autoComplete="new-password"
              disabled={disabled}
              onChange={(event) => update("apiKey", event.target.value)}
            />
            <Button
              type="button"
              variant="outline"
              size="icon"
              aria-label={keyVisible ? t("custom.hideApiKey") : t("custom.showApiKey")}
              aria-pressed={keyVisible}
              disabled={disabled}
              onClick={() => setKeyVisible((visible) => !visible)}
            >
              <Icon icon={keyVisible ? ViewOffIcon : ViewIcon} aria-hidden />
            </Button>
          </div>
          <span className="text-xs font-normal text-muted-foreground">{t("custom.apiKeyHint")}</span>
        </label>
        <label htmlFor="custom-model" className="grid min-w-0 gap-2 text-sm font-medium sm:col-span-2">
          {t("custom.model")}
          <Input
            id="custom-model"
            aria-label={t("custom.model")}
            value={connection.model}
            placeholder={t("custom.modelPlaceholder")}
            autoComplete="off"
            disabled={disabled}
            onChange={(event) => update("model", event.target.value)}
          />
        </label>
      </div>
      <div className="mt-6 flex flex-wrap items-center gap-3">
        <Button type="submit" disabled={disabled}>{saving ? t("custom.saving") : t("custom.save")}</Button>
        {!loading && <p className="text-xs text-muted-foreground">{t("custom.saveHint")}</p>}
      </div>
    </div>
  </form>;
}

/** 使用官方品牌资产呈现 Agent 身份，避免以通用图标混淆不同 provider。 */
function ProviderIdentity({ provider }: { provider: BabysitProvider }) {
  const { t } = useTranslation("settings");
  const codex = provider === "codex";
  const custom = provider === "custom";
  return <span className="flex items-center gap-3">
    <span className="flex size-10 shrink-0 items-center justify-center" aria-hidden="true">
      {custom
        ? <Icon icon={Settings02Icon} className="size-7 text-primary" />
        : <img src={codex ? "/icons/agents/codex.svg" : "/icons/agents/claude-code.svg"} alt="" className={cn("size-10 max-w-none object-contain", codex && "size-14 dark:invert")} />}
    </span>
    <span>{codex ? "Codex" : custom ? t("provider.custom") : "Claude Code"}</span>
  </span>;
}

type ProviderFieldsProps = {
  preference: BabysitProviderPreference;
  saving: boolean;
  onUpdate(preference: BabysitProviderPreference, revision?: string | null): Promise<void>;
  onChooseExecutable(): Promise<void>;
};

/** 呈现 provider 共用的 executable 值、原生选择与恢复自动发现操作。 */
function ProviderExecutableFields({ label, preference, saving, onUpdate, onChooseExecutable }: ProviderFieldsProps & { label: string }) {
  const automaticallyDiscovered = preference.executable === null;
  return <div className="min-w-0 rounded-2xl border bg-card p-4 sm:p-5">
    <h3 className="font-heading text-lg font-semibold">{label} 设置</h3>
    <p className="mt-5 text-sm font-medium">可执行文件</p>
    <div className="mt-2 flex min-w-0 flex-col gap-3 rounded-xl border bg-background/70 p-4 sm:flex-row sm:items-center">
      <div className="flex min-w-0 flex-1 items-center gap-3">
        <span className="flex size-9 shrink-0 items-center justify-center rounded-full bg-primary/10 text-primary" aria-hidden="true"><Icon icon={CheckmarkCircle02Icon} className="size-5" /></span>
        <div className="min-w-0">
          <p className="truncate font-mono text-sm" title={preference.executable ?? "由系统自动定位"}>{preference.executable ?? "由系统自动定位"}</p>
          <p className="mt-1 text-xs font-medium text-primary">{automaticallyDiscovered ? "自动发现" : "已选择"}</p>
        </div>
      </div>
      <Button variant="outline" className="self-start rounded-lg sm:self-auto" disabled={saving} onClick={() => void onChooseExecutable()}>更改</Button>
    </div>
    {preference.executable && <div className="mt-3 flex justify-end">
      <Button variant="ghost" size="sm" disabled={saving} onClick={() => void onUpdate({ ...preference, executable: null })}>恢复自动发现</Button>
    </div>
    }
  </div>;
}

/** 呈现 Codex 的延迟动态目录，并即时保存目录 revision 约束的完整组合。 */
function CodexProviderFields({ application, preference, saving, onUpdate, onChooseExecutable, onError }: ProviderFieldsProps & { application: ApplicationSettingsApplication; onError(detail: string): void }) {
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

/** 呈现 Claude Code 离线 alias、自定义模型与受模型约束的完整 effort 选项。 */
function ClaudeCodeProviderFields({ preference, saving, onUpdate, onChooseExecutable }: ProviderFieldsProps) {
  const customPreference = preference.model !== null && !claudeModels.some((item) => item.value === preference.model);
  const [customEditing, setCustomEditing] = useState(customPreference);
  const [customModel, setCustomModel] = useState(customPreference ? preference.model ?? "" : "");
  return <div className="grid min-w-0 gap-5" data-testid="claude-code-preferences">
    <ProviderExecutableFields label="Claude Code" preference={preference} saving={saving} onUpdate={onUpdate} onChooseExecutable={onChooseExecutable} />
    <label className="grid max-w-md gap-2 text-sm font-medium">模型<Select value={customEditing || customPreference ? "custom" : preference.model ?? "__default"} disabled={saving} onValueChange={(value) => {
      if (!value) return;
      if (value === "custom") { setCustomModel(customPreference ? preference.model ?? "" : ""); setCustomEditing(true); return; }
      setCustomEditing(false);
      const model = value === "__default" ? null : value;
      void onUpdate({ ...preference, model, reasoningEffort: model === "haiku" ? null : preference.reasoningEffort });
    }}><SelectTrigger aria-label="模型" className="h-11 w-full rounded-xl bg-background px-4"><SelectValue>{customEditing || customPreference ? "Custom" : claudeModels.find((item) => item.value === (preference.model ?? ""))?.label}</SelectValue></SelectTrigger><SelectContent align="start" alignItemWithTrigger={false}>{claudeModels.map((item) => <SelectItem key={item.value || "default"} value={item.value || "__default"}>{item.label}</SelectItem>)}</SelectContent></Select></label>
    {(customEditing || customPreference) && <label className="grid max-w-md gap-2 text-sm font-medium">自定义模型<input aria-label="自定义模型" className="h-11 rounded-xl border bg-background px-4" value={customModel} onChange={(event) => setCustomModel(event.target.value)} onBlur={() => customModel.trim() && void onUpdate({ ...preference, model: customModel.trim() })} /></label>}
    <label className="grid max-w-md gap-2 text-sm font-medium">思考强度<Select value={preference.reasoningEffort ?? "default"} disabled={saving || preference.model === "haiku"} onValueChange={(value) => value && void onUpdate({ ...preference, reasoningEffort: value === "default" ? null : value })}><SelectTrigger aria-label="思考强度" className="h-11 w-full rounded-xl bg-background px-4"><SelectValue>{preference.reasoningEffort === null ? "Default" : preference.reasoningEffort === "xhigh" ? "XHigh" : preference.reasoningEffort[0].toUpperCase() + preference.reasoningEffort.slice(1)}</SelectValue></SelectTrigger><SelectContent align="start" alignItemWithTrigger={false}><SelectItem value="default">Default</SelectItem>{preference.model !== "haiku" && claudeEfforts.map((effort) => <SelectItem key={effort} value={effort}>{effort === "xhigh" ? "XHigh" : effort[0].toUpperCase() + effort.slice(1)}</SelectItem>)}</SelectContent></Select></label>
  </div>;
}

/** 展示当前 GitHub Identity，并在解释本机数据影响后复用 lifecycle 退出。 */
function GitHubSettings({ identity, initiallyConfirming = false }: { identity: GitHubIdentityLifecycleBinding; initiallyConfirming?: boolean }) {
  const [confirming, setConfirming] = useState(initiallyConfirming);
  const profile = identity.status?.identity;
  const avatarUrl = githubIdentityAvatarUrl(profile?.avatarPath);
  const signingOut = identity.transition.kind === "signing-out";
  const displayName = profile?.name?.trim() || profile?.login || "已连接 GitHub";

  /** 确认后调用共享 lifecycle；失败时保留弹层外身份并允许再次提交。 */
  async function confirmSignOut() {
    const result = await identity.signOut();
    if (result.kind === "signed-out") setConfirming(false);
  }

  return <section aria-labelledby="github-settings-title">
    <div className="flex items-center gap-3"><Icon icon={GithubIcon} className="size-5" /><h2 id="github-settings-title" className="font-heading text-xl font-semibold">GitHub</h2></div>
    <p className="mt-1 text-sm text-muted-foreground">管理此设备当前授权的 GitHub Identity。</p>
    {identity.status?.signedIn && <div className="mt-6 flex flex-wrap items-center gap-4 rounded-xl border p-4">
      {profile ? <Avatar aria-label={`${displayName} 的 GitHub 头像`} className="size-12">{avatarUrl && <AvatarImage src={avatarUrl} alt="" />}<AvatarFallback>{displayName.slice(0, 2).toUpperCase()}</AvatarFallback></Avatar> : <Icon icon={GithubIcon} className="size-10" />}
      <div className="min-w-0 flex-1"><strong className="block truncate">{displayName}</strong>{profile && <span className="block truncate text-sm text-muted-foreground">@{profile.login}</span>}</div>
      <Button variant="destructive" onClick={() => setConfirming(true)}>退出 GitHub</Button>
    </div>}
    {identity.error?.operation === "sign-out" && <div className="mt-4 space-y-3 rounded-lg border border-destructive/30 bg-destructive/5 p-3"><p role="alert" className="text-sm text-destructive">退出失败：{identity.error.detail}</p><Button variant="outline" disabled={signingOut} onClick={() => void identity.signOut()}>{signingOut ? "正在退出…" : "重试退出"}</Button></div>}
    <Dialog open={confirming} onOpenChange={setConfirming}>
      <DialogContent>
        <DialogHeader><DialogTitle>确认退出 GitHub？</DialogTitle><DialogDescription>退出会删除此设备上的 GitHub 授权凭据以及身份和头像缓存。Repository、Review 与本地代码不会被删除。</DialogDescription></DialogHeader>
        <DialogFooter><DialogClose render={<Button variant="outline" />} disabled={signingOut}>取消</DialogClose><Button variant="destructive" disabled={signingOut} onClick={() => void confirmSignOut()}>{signingOut ? "正在退出…" : "确认退出"}</Button></DialogFooter>
      </DialogContent>
    </Dialog>
  </section>;
}

/** 从 typed command 结构化错误或普通异常中提取用户可见详情。 */
function errorDetail(error: unknown, fallback: string) {
  if (error instanceof Error) return error.message;
  if (typeof error === "string" && error.trim().length > 0) return error;
  if (typeof error === "object" && error && "detail" in error && typeof error.detail === "string") return error.detail;
  return fallback;
}
