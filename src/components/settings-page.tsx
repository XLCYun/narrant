import { useState } from "react";
import { ArrowLeft02Icon, ComputerIcon, GithubIcon, Moon02Icon, Settings02Icon, Sun03Icon } from "@hugeicons/core-free-icons";

import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import type { ApplicationSettingsApplication, ThemePreference } from "@/settings/application-settings";
import { useApplicationTheme, type ApplicationThemeBinding } from "@/settings/use-application-theme";
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
export function SettingsPage({ theme, onBack }: { theme: ApplicationThemeBinding; onBack(): void }) {
  const [section, setSection] = useState<Section>("appearance");
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
            {themes.map((item) => <button key={item.value} type="button" role="radio" aria-checked={theme.preference === item.value} disabled={!theme.loaded} className={cn("rounded-xl border p-4 text-left outline-none transition-colors hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring", theme.preference === item.value && "border-primary bg-primary/5 ring-1 ring-primary")} onClick={() => void theme.choose(item.value)}>
              <Icon icon={item.icon} className="mb-3 size-5" /><strong className="block text-sm">{item.label}</strong><span className="mt-1 block text-xs text-muted-foreground">{item.description}</span>
            </button>)}
          </div>
        </section>}
        {section === "github" && <section><div className="flex items-center gap-3"><Icon icon={GithubIcon} className="size-5" /><h2 className="font-heading text-xl font-semibold">GitHub</h2></div><p className="mt-4 text-sm text-muted-foreground">GitHub Identity 设置将在后续版本中提供。</p></section>}
        {section === "babysit" && <section><h2 className="font-heading text-xl font-semibold">宝宝模式</h2><p className="mt-4 text-sm text-muted-foreground">宝宝模式设置将在后续版本中提供。</p></section>}
      </div>
    </div>
  </section>;
}

/** 为测试与 Preview 提供从公共 Application Settings 端口开始的完整目的地入口。 */
export function ApplicationSettingsDestination({ application, onBack }: { application: ApplicationSettingsApplication; onBack(): void }) {
  const theme = useApplicationTheme(application);
  return <SettingsPage theme={theme} onBack={onBack} />;
}
