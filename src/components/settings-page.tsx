import { useEffect, useState } from "react";
import { ArrowLeft02Icon, ComputerIcon, GithubIcon, Moon02Icon, Settings02Icon, Sun03Icon } from "@hugeicons/core-free-icons";

import { Button } from "@/components/ui/button";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Icon } from "@/components/ui/icon";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { githubIdentityAvatarUrl } from "@/components/github-identity-entry";
import type { GitHubIdentityLifecycleBinding } from "@/github/use-github-identity-lifecycle";
import type { ApplicationSettingsApplication, BabysitModelCatalogEntry, BabysitProvider, BabysitProviderPreference, BabysitSettings, ThemePreference } from "@/settings/application-settings";
import type { SettingsSection } from "@/settings/settings-section";
import { useApplicationTheme, type ApplicationThemeBinding } from "@/settings/use-application-theme";
import { cn } from "@/lib/utils";

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
export function SettingsPage({ theme, application, githubIdentity, initialSection = "appearance", initialGitHubSignOutConfirmation = false, onBack }: { theme: ApplicationThemeBinding; application: ApplicationSettingsApplication; githubIdentity?: GitHubIdentityLifecycleBinding; initialSection?: SettingsSection; initialGitHubSignOutConfirmation?: boolean; onBack(): void }) {
  const [section, setSection] = useState<SettingsSection>(initialSection);
  return <section aria-label="应用设置" className="mx-auto w-full max-w-6xl py-4 sm:py-8">
    <header className="mb-6 flex items-center gap-3">
      <Button variant="ghost" size="icon" aria-label="返回" onClick={onBack}><Icon icon={ArrowLeft02Icon} /></Button>
      <div><h1 className="font-heading text-2xl font-semibold">应用设置</h1><p className="text-sm text-muted-foreground">管理此设备上的 Narrant 偏好</p></div>
    </header>
    <div className="grid gap-6 md:grid-cols-[13rem_minmax(0,1fr)]">
      <nav role="tablist" aria-label="设置分区" className="flex gap-2 overflow-x-auto md:flex-col">
        {sections.map((item) => <Button key={item.id} role="tab" aria-selected={section === item.id} variant={section === item.id ? "secondary" : "ghost"} className="shrink-0 justify-start" onClick={() => setSection(item.id)}>{item.label}</Button>)}
      </nav>
      <div className="min-w-0 rounded-xl border bg-card p-5 text-card-foreground sm:p-7">
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
        {section === "babysit" && <BabysitProviderSettings application={application} />}
      </div>
    </div>
  </section>;
}

/** 为测试与 Preview 提供从公共 Application Settings 端口开始的完整目的地入口。 */
export function ApplicationSettingsDestination({ application, githubIdentity, initialSection, initialGitHubSignOutConfirmation, onBack }: { application: ApplicationSettingsApplication; githubIdentity?: GitHubIdentityLifecycleBinding; initialSection?: SettingsSection; initialGitHubSignOutConfirmation?: boolean; onBack(): void }) {
  const theme = useApplicationTheme(application);
  return <SettingsPage theme={theme} application={application} githubIdentity={githubIdentity} initialSection={initialSection} initialGitHubSignOutConfirmation={initialGitHubSignOutConfirmation} onBack={onBack} />;
}

const claudeModels = [
  { value: "", label: "Default" }, { value: "best", label: "Best（使用最强的可用模型）" },
  { value: "fable", label: "Fable" }, { value: "opus", label: "Opus" },
  { value: "sonnet", label: "Sonnet" }, { value: "haiku", label: "Haiku" }, { value: "custom", label: "Custom" },
];
const claudeEfforts = ["low", "medium", "high", "xhigh", "max"];

/** 通过公共设置端口呈现当前 provider，并对每次选择执行完整组合原子保存。 */
function BabysitProviderSettings({ application }: { application: ApplicationSettingsApplication }) {
  const [saved, setSaved] = useState<BabysitSettings>();
  const [error, setError] = useState<string>();
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    let current = true;
    void application.load().then((settings) => {
      if (!current) return;
      setSaved(settings.babysit);
    }).catch((reason) => current && setError(errorDetail(reason, "无法读取 Babysit 设置")));
    return () => { current = false; };
  }, [application]);

  if (!saved) return <section aria-labelledby="babysit-title"><h2 id="babysit-title" className="font-heading text-xl font-semibold">宝宝模式</h2><p role={error ? "alert" : "status"} className="mt-4 text-sm text-muted-foreground">{error ?? "正在读取设置…"}</p></section>;
  const provider = saved.provider;
  const key = provider === "codex" ? "codex" : "claudeCode";
  const preference = saved[key];

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
    const next = { ...saved!, [key]: nextPreference };
    return commit(next, () => application.updateBabysitPreferences(provider, nextPreference, revision));
  }

  return <section aria-labelledby="babysit-title">
    <h2 id="babysit-title" className="font-heading text-xl font-semibold">宝宝模式</h2>
    <p className="mt-1 text-sm text-muted-foreground">选择未来逐行解释使用的 Agent Provider；每套偏好独立保留。</p>
    {error && <p role="alert" className="mt-4 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">{error}</p>}
    <div className="mt-6 grid gap-5" aria-busy={saving || undefined}>
      <label className="grid gap-2 text-sm font-medium">Provider
        <Select value={provider} disabled={saving} onValueChange={(value) => {
          if (!value) return;
          const nextProvider = value as BabysitProvider;
          void commit({ ...saved, provider: nextProvider }, () => application.updateBabysitProvider(nextProvider));
        }}><SelectTrigger aria-label="Provider"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="codex">Codex</SelectItem><SelectItem value="claude-code">Claude Code</SelectItem></SelectContent></Select>
      </label>
      {provider === "codex"
        ? <CodexProviderFields application={application} preference={preference} saving={saving} onUpdate={updatePreference} onChooseExecutable={chooseExecutable} onError={setError} />
        : <ClaudeCodeProviderFields preference={preference} saving={saving} onUpdate={updatePreference} onChooseExecutable={chooseExecutable} />}
    </div>
  </section>;

  /** 选择并保存当前 provider 的 executable，失败时恢复加载得到的权威快照。 */
  async function chooseExecutable() {
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

type ProviderFieldsProps = {
  preference: BabysitProviderPreference;
  saving: boolean;
  onUpdate(preference: BabysitProviderPreference, revision?: string | null): Promise<void>;
  onChooseExecutable(): Promise<void>;
};

/** 呈现 provider 共用的 executable 值、原生选择与恢复自动发现操作。 */
function ProviderExecutableFields({ label, preference, saving, onUpdate, onChooseExecutable }: ProviderFieldsProps & { label: string }) {
  return <div className="rounded-xl border bg-background p-4">
    <p className="text-sm font-medium">{label} 可执行文件</p>
    <p className="mt-1 break-all font-mono text-xs text-muted-foreground">{preference.executable ?? "自动发现"}</p>
    <div className="mt-3 flex gap-2">
      <Button variant="outline" disabled={saving} onClick={() => void onChooseExecutable()}>选择 {label} 可执行文件</Button>
      {preference.executable && <Button variant="ghost" disabled={saving} onClick={() => void onUpdate({ ...preference, executable: null })}>恢复自动发现</Button>}
    </div>
  </div>;
}

/** 呈现 Codex 的延迟动态目录，并即时保存目录 revision 约束的完整组合。 */
function CodexProviderFields({ application, preference, saving, onUpdate, onChooseExecutable, onError }: ProviderFieldsProps & { application: ApplicationSettingsApplication; onError(detail: string): void }) {
  const [catalog, setCatalog] = useState<{ revision: string; models: BabysitModelCatalogEntry[] }>();
  const [catalogLoading, setCatalogLoading] = useState(false);
  const [catalogError, setCatalogError] = useState<string>();
  /** 按需读取最新 Codex 目录，失败后保留显式重试入口。 */
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
  return <div className="grid gap-4" data-testid="codex-preferences">
    <ProviderExecutableFields label="Codex" preference={preference} saving={saving} onUpdate={onUpdate} onChooseExecutable={onChooseExecutable} />
    {!catalog && <Button variant="outline" className="justify-self-start" disabled={saving || catalogLoading} onClick={() => void loadCatalog()}>{catalogLoading ? "正在获取模型…" : catalogError ? "重试获取模型" : "修改模型偏好"}</Button>}
    {catalogLoading && <p role="status" className="text-sm text-muted-foreground">正在读取 Codex 模型目录…</p>}
    {catalogError && !catalogLoading && <p className="text-sm text-muted-foreground">已保留当前保存值，可重试获取模型目录。</p>}
    {catalog?.models.length === 0 && <p role="status" className="rounded-lg border p-3 text-sm text-muted-foreground">当前 Codex 未返回可用模型，已保留当前设置。</p>}
    {catalog && <>
      <label className="grid gap-2 text-sm font-medium">模型<Select value={preference.model ?? "__default"} disabled={saving} onValueChange={(value) => {
        if (!value) return;
        const model = value === "__default" ? null : value;
        const supported = catalog.models.find((item) => item.id === model)?.supportedReasoningEfforts;
        const reasoningEffort = preference.reasoningEffort && supported?.includes(preference.reasoningEffort) ? preference.reasoningEffort : null;
        void onUpdate({ ...preference, model, reasoningEffort }, catalog.revision);
      }}><SelectTrigger aria-label="模型"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="__default">Default</SelectItem>{catalog.models.map((model) => <SelectItem key={model.id} value={model.id}>{model.name}</SelectItem>)}</SelectContent></Select></label>
      <label className="grid gap-2 text-sm font-medium">思考强度<Select value={preference.reasoningEffort ?? "default"} disabled={saving || !preference.model} onValueChange={(value) => value && void onUpdate({ ...preference, reasoningEffort: value === "default" ? null : value }, catalog.revision)}><SelectTrigger aria-label="思考强度"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="default">Default</SelectItem>{catalog.models.find((model) => model.id === preference.model)?.supportedReasoningEfforts.map((effort) => <SelectItem key={effort} value={effort}>{effort}</SelectItem>)}</SelectContent></Select></label>
    </>}
  </div>;
}

/** 呈现 Claude Code 离线 alias、自定义模型与受模型约束的完整 effort 选项。 */
function ClaudeCodeProviderFields({ preference, saving, onUpdate, onChooseExecutable }: ProviderFieldsProps) {
  const customPreference = preference.model !== null && !claudeModels.some((item) => item.value === preference.model);
  const [customEditing, setCustomEditing] = useState(customPreference);
  const [customModel, setCustomModel] = useState(customPreference ? preference.model ?? "" : "");
  return <div className="grid gap-4" data-testid="claude-code-preferences">
    <ProviderExecutableFields label="Claude Code" preference={preference} saving={saving} onUpdate={onUpdate} onChooseExecutable={onChooseExecutable} />
    <label className="grid gap-2 text-sm font-medium">模型<Select value={customEditing || customPreference ? "custom" : preference.model ?? "__default"} disabled={saving} onValueChange={(value) => {
      if (!value) return;
      if (value === "custom") { setCustomModel(customPreference ? preference.model ?? "" : ""); setCustomEditing(true); return; }
      setCustomEditing(false);
      const model = value === "__default" ? null : value;
      void onUpdate({ ...preference, model, reasoningEffort: model === "haiku" ? null : preference.reasoningEffort });
    }}><SelectTrigger aria-label="模型"><SelectValue>{customEditing || customPreference ? "Custom" : claudeModels.find((item) => item.value === (preference.model ?? ""))?.label}</SelectValue></SelectTrigger><SelectContent>{claudeModels.map((item) => <SelectItem key={item.value || "default"} value={item.value || "__default"}>{item.label}</SelectItem>)}</SelectContent></Select></label>
    {(customEditing || customPreference) && <label className="grid gap-2 text-sm font-medium">自定义模型<input aria-label="自定义模型" className="h-9 rounded-md border bg-transparent px-3" value={customModel} onChange={(event) => setCustomModel(event.target.value)} onBlur={() => customModel.trim() && void onUpdate({ ...preference, model: customModel.trim() })} /></label>}
    <label className="grid gap-2 text-sm font-medium">思考强度<Select value={preference.reasoningEffort ?? "default"} disabled={saving || preference.model === "haiku"} onValueChange={(value) => value && void onUpdate({ ...preference, reasoningEffort: value === "default" ? null : value })}><SelectTrigger aria-label="思考强度"><SelectValue>{preference.reasoningEffort === null ? "Default" : preference.reasoningEffort === "xhigh" ? "XHigh" : preference.reasoningEffort[0].toUpperCase() + preference.reasoningEffort.slice(1)}</SelectValue></SelectTrigger><SelectContent><SelectItem value="default">Default</SelectItem>{preference.model !== "haiku" && claudeEfforts.map((effort) => <SelectItem key={effort} value={effort}>{effort === "xhigh" ? "XHigh" : effort[0].toUpperCase() + effort.slice(1)}</SelectItem>)}</SelectContent></Select></label>
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
