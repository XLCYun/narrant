import { useCallback, useEffect, useState } from "react";
import { ArrowLeft02Icon, ComputerIcon, GithubIcon, Moon02Icon, Settings02Icon, Sun03Icon } from "@hugeicons/core-free-icons";

import { Button } from "@/components/ui/button";
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Icon } from "@/components/ui/icon";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { githubIdentityAvatarUrl } from "@/components/github-identity-entry";
import type { GitHubIdentityLifecycleBinding } from "@/github/use-github-identity-lifecycle";
import type { ApplicationSettingsApplication, BabysitModelCatalogEntry, BabysitModelPreference, BabysitProvider, BabysitProviderPreference, BabysitSettings, ReasoningEffort, ThemePreference } from "@/settings/application-settings";
import { useApplicationTheme, type ApplicationThemeBinding } from "@/settings/use-application-theme";
import { useCodexExecutablePreference, type CodexExecutablePreferenceBinding } from "@/settings/use-codex-executable-preference";
import { cn } from "@/lib/utils";

type Section = "appearance" | "github" | "babysit";

const sections: Array<{ id: Section; label: string }> = [
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
export function SettingsPage({ theme, codex, modelApplication, githubIdentity, initialSection = "appearance", initialGitHubSignOutConfirmation = false, initialModelEditing = false, onBack }: { theme: ApplicationThemeBinding; codex: CodexExecutablePreferenceBinding; modelApplication: ApplicationSettingsApplication; githubIdentity?: GitHubIdentityLifecycleBinding; initialSection?: Section; initialGitHubSignOutConfirmation?: boolean; initialModelEditing?: boolean; onBack(): void }) {
  // 保留旧公开 props 直到父 Issue 整合调用点；Babysit 新 UI 统一从 application 投影读取。
  void codex;
  void initialModelEditing;
  const [section, setSection] = useState<Section>(initialSection);
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
        {section === "babysit" && <BabysitProviderSettings application={modelApplication} />}
      </div>
    </div>
  </section>;
}

/** 为测试与 Preview 提供从公共 Application Settings 端口开始的完整目的地入口。 */
export function ApplicationSettingsDestination({ application, githubIdentity, initialSection, initialGitHubSignOutConfirmation, initialModelEditing, onBack }: { application: ApplicationSettingsApplication; githubIdentity?: GitHubIdentityLifecycleBinding; initialSection?: Section; initialGitHubSignOutConfirmation?: boolean; initialModelEditing?: boolean; onBack(): void }) {
  const theme = useApplicationTheme(application);
  const codex = useCodexExecutablePreference(application);
  return <SettingsPage theme={theme} codex={codex} modelApplication={application} githubIdentity={githubIdentity} initialSection={initialSection} initialGitHubSignOutConfirmation={initialGitHubSignOutConfirmation} initialModelEditing={initialModelEditing} onBack={onBack} />;
}

const defaultProviderPreference: BabysitProviderPreference = { executable: null, model: null, reasoningEffort: null };
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
  const [customModel, setCustomModel] = useState("");
  const [customEditing, setCustomEditing] = useState(false);
  const [catalog, setCatalog] = useState<{ revision: string; models: BabysitModelCatalogEntry[] }>();

  useEffect(() => {
    let current = true;
    void application.load().then((settings) => {
      if (!current) return;
      setSaved(settings.babysit ?? { provider: "codex", codex: { ...defaultProviderPreference, executable: settings.codexExecutable ?? null }, claudeCode: defaultProviderPreference });
    }).catch((reason) => current && setError(errorDetail(reason, "无法读取 Babysit 设置")));
    return () => { current = false; };
  }, [application]);

  if (!saved) return <section aria-labelledby="babysit-title"><h2 id="babysit-title" className="font-heading text-xl font-semibold">宝宝模式</h2><p role={error ? "alert" : "status"} className="mt-4 text-sm text-muted-foreground">{error ?? "正在读取设置…"}</p></section>;
  const provider = saved.provider;
  const key = provider === "codex" ? "codex" : "claudeCode";
  const preference = saved[key];

  /** 乐观提交新快照，失败时恢复先前权威值并显示错误。 */
  async function commit(next: BabysitSettings, operation: () => Promise<{ babysit?: BabysitSettings }>) {
    if (saving) return;
    const previous = saved!;
    setSaved(next); setSaving(true); setError(undefined);
    try {
      const result = await operation();
      setSaved(result.babysit ?? next);
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
      <div className="rounded-xl border bg-background p-4" data-testid={`${provider}-preferences`}>
        <p className="text-sm font-medium">{provider === "codex" ? "Codex" : "Claude Code"} 可执行文件</p>
        <p className="mt-1 break-all font-mono text-xs text-muted-foreground">{preference.executable ?? "自动发现"}</p>
        <div className="mt-3 flex gap-2"><Button variant="outline" disabled={saving} onClick={async () => {
          const previous = saved; setSaving(true); setError(undefined);
          try { const result = await application.chooseBabysitExecutable(provider, preference); if (result?.babysit) setSaved(result.babysit); }
          catch (reason) { setSaved(previous); setError(`${errorDetail(reason, "无法保存可执行文件")}，已恢复原设置。`); }
          finally { setSaving(false); }
        }}>选择{provider === "codex" ? " Codex" : " Claude Code"}可执行文件</Button>
        {preference.executable && <Button variant="ghost" disabled={saving} onClick={() => void updatePreference({ ...preference, executable: null })}>恢复自动发现</Button>}</div>
      </div>
      {provider === "codex" && <div className="grid gap-4">
        {!catalog && <Button variant="outline" className="justify-self-start" disabled={saving} onClick={async () => { try { const result = await application.listBabysitModels("codex"); setCatalog(result); } catch (reason) { setError(errorDetail(reason, "无法获取模型目录")); } }}>修改模型偏好</Button>}
        {catalog && <><label className="grid gap-2 text-sm font-medium">模型<Select value={preference.model ?? "__default"} disabled={saving} onValueChange={(value) => {
          if (!value) return;
          const model = value === "__default" ? null : value;
          const supported = catalog.models.find((item) => item.id === model)?.supportedReasoningEfforts;
          const reasoningEffort = preference.reasoningEffort && supported?.includes(preference.reasoningEffort) ? preference.reasoningEffort : null;
          void updatePreference({ ...preference, model, reasoningEffort }, catalog.revision);
        }}><SelectTrigger aria-label="模型"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="__default">Default</SelectItem>{catalog.models.map((model) => <SelectItem key={model.id} value={model.id}>{model.name}</SelectItem>)}</SelectContent></Select></label>
        <label className="grid gap-2 text-sm font-medium">思考强度<Select value={preference.reasoningEffort ?? "default"} disabled={saving || !preference.model} onValueChange={(value) => value && void updatePreference({ ...preference, reasoningEffort: value === "default" ? null : value }, catalog.revision)}><SelectTrigger aria-label="思考强度"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="default">Default</SelectItem>{catalog.models.find((model) => model.id === preference.model)?.supportedReasoningEfforts.map((effort) => <SelectItem key={effort} value={effort}>{effort}</SelectItem>)}</SelectContent></Select></label></>}
      </div>}
      {provider === "claude-code" && <>
        <label className="grid gap-2 text-sm font-medium">模型<Select value={preference.model && !claudeModels.some((item) => item.value === preference.model) ? "custom" : preference.model ?? "__default"} disabled={saving} onValueChange={(value) => {
          if (value === undefined) return;
          if (value === "custom") { setCustomModel(""); setCustomEditing(true); return; }
          setCustomEditing(false);
          const model = value === "__default" ? null : value;
          const reasoningEffort = model === "haiku" ? null : preference.reasoningEffort;
          void updatePreference({ ...preference, model, reasoningEffort });
        }}><SelectTrigger aria-label="模型"><SelectValue>{claudeModels.find((item) => item.value === (preference.model ?? ""))?.label ?? preference.model}</SelectValue></SelectTrigger><SelectContent>{claudeModels.map((item) => <SelectItem key={item.value || "default"} value={item.value || "__default"}>{item.label}</SelectItem>)}</SelectContent></Select></label>
        {(customEditing || (preference.model !== null && !claudeModels.some((item) => item.value === preference.model))) && <label className="grid gap-2 text-sm font-medium">自定义模型<input aria-label="自定义模型" className="h-9 rounded-md border bg-transparent px-3" value={customModel || preference.model || ""} onChange={(event) => setCustomModel(event.target.value)} onBlur={() => customModel.trim() && void updatePreference({ ...preference, model: customModel.trim() })} /></label>}
        <label className="grid gap-2 text-sm font-medium">思考强度<Select value={preference.reasoningEffort ?? "default"} disabled={saving || preference.model === "haiku"} onValueChange={(value) => value && void updatePreference({ ...preference, reasoningEffort: value === "default" ? null : value })}><SelectTrigger aria-label="思考强度"><SelectValue /></SelectTrigger><SelectContent><SelectItem value="default">Default</SelectItem>{claudeEfforts.map((effort) => <SelectItem key={effort} value={effort}>{effort === "xhigh" ? "XHigh" : effort[0].toUpperCase() + effort.slice(1)}</SelectItem>)}</SelectContent></Select></label>
      </>}
    </div>
  </section>;
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

/** Application Settings 中管理 Codex Executable Preference 的公共呈现组件。 */
export function CodexExecutableSettings({ binding, modelApplication, initialModelEditing = false }: { binding: CodexExecutablePreferenceBinding; modelApplication: ApplicationSettingsApplication; initialModelEditing?: boolean }) {
  return <section aria-labelledby="babysit-title">
    <h2 id="babysit-title" className="font-heading text-xl font-semibold">宝宝模式</h2>
    <p className="mt-1 text-sm text-muted-foreground">为之后启动的逐行解释选择 Codex 可执行文件。</p>
    {binding.error && <p role="alert" className="mt-4 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">{binding.error}</p>}
    <div aria-busy={!binding.loaded || binding.saving || undefined} className="mt-6 min-w-0 rounded-xl border bg-background p-4">
      <div className="flex min-w-0 flex-col gap-4 sm:flex-row sm:items-center">
        <div className="min-w-0 flex-1">
          <p className="text-sm font-medium">Codex 可执行文件</p>
          <p className="mt-1 break-all font-mono text-xs text-muted-foreground" title={binding.executable ?? undefined} data-testid="codex-executable-value">{binding.executable ?? "自动发现"}</p>
        </div>
        <div className="flex shrink-0 flex-wrap gap-2">
          <Button variant="outline" disabled={!binding.loaded || binding.saving} onClick={() => void binding.choose()}>{binding.saving ? "正在验证…" : "选择 Codex 可执行文件"}</Button>
          {binding.executable && <Button variant="ghost" disabled={binding.saving} onClick={() => void binding.clear()}>恢复自动发现</Button>}
        </div>
      </div>
    </div>
    <BabysitModelSettings application={modelApplication} initialEditing={initialModelEditing} />
  </section>;
}

/** 模型编辑器的延迟目录状态；目录只存在于 editing 分支。 */
type ModelEditorState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "failed"; detail: string }
  | { kind: "editing"; revision: string; models: BabysitModelCatalogEntry[] };

/** 管理延迟模型目录、草稿与显式提交，目录永不进入持久化投影。 */
function BabysitModelSettings({ application, initialEditing }: { application: ApplicationSettingsApplication; initialEditing: boolean }) {
  const [saved, setSaved] = useState<BabysitModelPreference>({ kind: "automatic" });
  const [loaded, setLoaded] = useState(false);
  const [editor, setEditor] = useState<ModelEditorState>({ kind: "idle" });
  const [draft, setDraft] = useState<BabysitModelPreference>({ kind: "automatic" });
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string>();
  const [loadError, setLoadError] = useState<string>();

  /** 读取当前持久化模型偏好；失败时保留现有投影并提供显式重试。 */
  const loadSavedPreference = useCallback(async () => {
    setLoadError(undefined);
    try {
      const settings = await application.load();
      setSaved(settings.babysitModel);
      setLoaded(true);
    } catch (error) {
      setLoadError(errorDetail(error, "无法读取模型设置"));
    }
  }, [application]);

  useEffect(() => {
    void loadSavedPreference();
  }, [loadSavedPreference]);

  useEffect(() => {
    if (initialEditing && loaded && editor.kind === "idle") void loadCatalog();
  // Preview-only initial state is consumed once after the saved preference loads.
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [initialEditing, loaded]);

  /** 进入编辑态时获取新鲜目录；失败或空目录均保留旧偏好。 */
  async function loadCatalog() {
    setEditor({ kind: "loading" });
    setSaveError(undefined);
    try {
      const catalog = await application.listBabysitModels();
      if (catalog.models.length === 0) {
        setEditor({ kind: "failed", detail: "当前没有可用模型" });
        return;
      }
      setDraft(saved);
      setEditor({ kind: "editing", revision: catalog.revision, models: catalog.models });
    } catch (error) {
      setEditor({ kind: "failed", detail: errorDetail(error, "无法获取模型目录") });
    }
  }

  /** 显式保存草稿；失败时编辑态和旧保存值均保持不变。 */
  async function save() {
    setSaving(true);
    setSaveError(undefined);
    try {
      if (editor.kind !== "editing") return;
      const settings = await application.updateBabysitModel(draft, editor.revision);
      setSaved(settings.babysitModel);
      setEditor({ kind: "idle" });
    } catch (error) {
      setSaveError(errorDetail(error, "无法保存模型偏好"));
    } finally {
      setSaving(false);
    }
  }

  const savedLabel = saved.kind === "automatic"
    ? "自动（使用 App Server 默认值）"
    : `${modelName(editor, saved.modelId)} · ${saved.reasoningEffort ?? "模型默认"}`;
  const draftModel = editor.kind === "editing" && draft.kind === "explicit"
    ? editor.models.find((model) => model.id === draft.modelId)
    : undefined;

  return <div className="mt-4 rounded-xl border bg-background p-4">
    <div className="flex flex-col gap-3 sm:flex-row sm:items-center">
      <div className="min-w-0 flex-1"><p className="text-sm font-medium">模型与思考强度</p><p className="mt-1 text-sm text-muted-foreground">{loaded ? savedLabel : "正在读取保存值…"}</p></div>
      {editor.kind === "idle" && <Button variant="outline" disabled={!loaded} onClick={() => void loadCatalog()}>修改模型偏好</Button>}
    </div>
    {loadError && <div className="mt-4 space-y-3"><p role="alert" className="text-sm text-destructive">{loadError}，当前显示值保持不变。</p><Button variant="outline" onClick={() => void loadSavedPreference()}>重试读取模型设置</Button></div>}
    {editor.kind === "loading" && <p role="status" className="mt-4 text-sm text-muted-foreground">正在获取可用模型…</p>}
    {editor.kind === "failed" && <div className="mt-4 space-y-3"><p role="alert" className="text-sm text-destructive">{editor.detail}，原设置已保留。</p><div className="flex gap-2"><Button variant="outline" onClick={() => void loadCatalog()}>重试获取模型</Button><Button variant="ghost" onClick={() => setEditor({ kind: "idle" })}>取消模型修改</Button></div></div>}
    {editor.kind === "editing" && <div className="mt-5 space-y-5">
      {saveError && <p role="alert" className="rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">{saveError}，原设置已保留。</p>}
      <div className="grid gap-4 sm:grid-cols-2">
        <ModelPreferenceSelect models={editor.models} value={draft} onChange={setDraft} />
        <ReasoningEffortSelect model={draftModel} value={draft.kind === "explicit" ? draft.reasoningEffort : null} automatic={draft.kind === "automatic"} onChange={(reasoningEffort) => draft.kind === "explicit" && setDraft({ ...draft, reasoningEffort })} />
      </div>
      {draft.kind === "explicit" && !draftModel && <p role="alert" className="text-sm text-destructive">已保存模型不再可用，请选择自动模式或其他模型。</p>}
      <div className="flex flex-wrap justify-end gap-2"><Button variant="ghost" disabled={saving} onClick={() => { setEditor({ kind: "idle" }); setSaveError(undefined); }}>取消模型修改</Button><Button disabled={saving} onClick={() => void save()}>{saving ? "正在保存…" : "保存模型偏好"}</Button></div>
    </div>}
  </div>;
}

/** 通过动态模型目录呈现自动模式与显式模型选择。 */
function ModelPreferenceSelect({ models, value, onChange }: { models: BabysitModelCatalogEntry[]; value: BabysitModelPreference; onChange(value: BabysitModelPreference): void }) {
  const selectedValue = value.kind === "automatic" ? "mode:automatic" : `model:${value.modelId}`;
  const unavailable = value.kind === "explicit" && !models.some((model) => model.id === value.modelId) ? value.modelId : undefined;
  const selectedModel = value.kind === "explicit" ? models.find((model) => model.id === value.modelId) : undefined;
  const selectedLabel = value.kind === "automatic"
    ? "自动（使用 App Server 默认值）"
    : selectedModel ? `${selectedModel.name}${selectedModel.isDefault ? "（服务端默认）" : ""}` : `${value.modelId}（不可用）`;
  return <label className="grid gap-2 text-sm font-medium">模型
    <Select value={selectedValue} onValueChange={(nextValue) => nextValue && onChange(nextValue === "mode:automatic" ? { kind: "automatic" } : { kind: "explicit", modelId: nextValue.slice("model:".length), reasoningEffort: null })}>
      <SelectTrigger aria-label="模型" className="w-full"><SelectValue>{selectedLabel}</SelectValue></SelectTrigger>
      <SelectContent align="start">
        <SelectItem value="mode:automatic">自动（使用 App Server 默认值）</SelectItem>
        {unavailable && <SelectItem value={`model:${unavailable}`} disabled>{unavailable}（不可用）</SelectItem>}
        {models.map((model) => <SelectItem key={model.id} value={`model:${model.id}`}>{model.name}{model.isDefault ? "（服务端默认）" : ""}</SelectItem>)}
      </SelectContent>
    </Select>
  </label>;
}

/** 仅呈现当前显式模型动态声明的思考强度。 */
function ReasoningEffortSelect({ model, value, automatic, onChange }: { model?: BabysitModelCatalogEntry; value: ReasoningEffort | null; automatic: boolean; onChange(value: ReasoningEffort | null): void }) {
  const selectedValue = value === null ? "effort:default" : `effort:${value}`;
  const selectedLabel = automatic ? "使用 App Server 默认值" : model ? value ?? `模型默认（${model.defaultReasoningEffort}）` : "请选择可用模型";
  return <label className="grid gap-2 text-sm font-medium">思考强度
    <Select value={selectedValue} disabled={automatic || !model} onValueChange={(nextValue) => nextValue && onChange(nextValue === "effort:default" ? null : nextValue.slice("effort:".length))}>
      <SelectTrigger aria-label="思考强度" className="w-full"><SelectValue>{selectedLabel}</SelectValue></SelectTrigger>
      <SelectContent align="start">
        {model && <SelectItem value="effort:default">模型默认（{model.defaultReasoningEffort}）</SelectItem>}
        {model?.supportedReasoningEfforts.map((effort) => <SelectItem key={effort} value={`effort:${effort}`}>{effort}</SelectItem>)}
      </SelectContent>
    </Select>
  </label>;
}

/** 在已有目录时显示服务端名称，否则稳定显示保存的模型标识。 */
function modelName(editor: ModelEditorState, modelId: string) {
  return editor.kind === "editing" ? editor.models.find((model) => model.id === modelId)?.name ?? modelId : modelId;
}

/** 从 typed command 结构化错误或普通异常中提取用户可见详情。 */
function errorDetail(error: unknown, fallback: string) {
  if (error instanceof Error) return error.message;
  if (typeof error === "string" && error.trim().length > 0) return error;
  if (typeof error === "object" && error && "detail" in error && typeof error.detail === "string") return error.detail;
  return fallback;
}
