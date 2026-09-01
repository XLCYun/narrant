import { ComputerIcon, LanguageCircleIcon, Moon02Icon, Settings02Icon, Sun03Icon } from "@hugeicons/core-free-icons";

import { Icon } from "@/components/ui/icon";
import { cn } from "@/lib/utils";
import type { ApplicationLanguage, ThemePreference } from "@/settings/application-settings";
import type { ApplicationThemeBinding } from "@/settings/use-application-theme";

const themes: Array<{ value: ThemePreference; label: string; description: string; icon: typeof ComputerIcon }> = [
  { value: "system", label: "跟随系统", description: "随操作系统外观自动切换", icon: ComputerIcon },
  { value: "light", label: "浅色", description: "始终使用明亮外观", icon: Sun03Icon },
  { value: "dark", label: "深色", description: "始终使用深色外观", icon: Moon02Icon },
];

const languages: Array<{ value: ApplicationLanguage; label: string; description: string }> = [
  { value: "zh-CN", label: "简体中文", description: "使用简体中文显示应用界面" },
  { value: "en-US", label: "English", description: "Use English for the application interface" },
];

/** 外观设置消费完整主题 binding，并隐藏主题保存状态的呈现细节。 */
export function AppearanceSettings({ theme }: { theme: ApplicationThemeBinding }) {
  return <section aria-labelledby="appearance-title">
    <div className="flex items-center gap-3"><Icon icon={Settings02Icon} className="size-5" /><h2 id="appearance-title" className="font-heading text-xl font-semibold">外观</h2></div>
    <p className="mt-1 text-sm text-muted-foreground">主题会同时应用到应用框架、代码阅读器与品牌图像。</p>
    {theme.error && <p role="alert" className="mt-4 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">{theme.error}</p>}
    <div role="radiogroup" aria-label="主题" aria-busy={!theme.loaded || undefined} className="mt-6 grid gap-3 sm:grid-cols-3">
      {themes.map((item) => <button key={item.value} type="button" role="radio" aria-checked={theme.preference === item.value} disabled={!theme.loaded || theme.saving} className={cn("rounded-xl border p-4 text-left outline-none transition-colors hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring", theme.preference === item.value && "border-primary bg-primary/5 ring-1 ring-primary")} onClick={() => void theme.choose(item.value)}>
        <Icon icon={item.icon} className="mb-3 size-5" /><strong className="block text-sm">{item.label}</strong><span className="mt-1 block text-xs text-muted-foreground">{item.description}</span>
      </button>)}
    </div>
    <div className="mt-8 border-t pt-6">
      <div className="flex items-center gap-3"><Icon icon={LanguageCircleIcon} className="size-5" /><h3 id="language-title" className="font-heading text-lg font-semibold">语言</h3></div>
      <p className="mt-1 text-sm text-muted-foreground">选择应用界面使用的语言。</p>
      <div role="radiogroup" aria-label="语言" aria-busy={!theme.loaded || undefined} className="mt-4 grid gap-3 sm:grid-cols-2">
        {languages.map((item) => <button key={item.value} type="button" role="radio" aria-label={item.label} aria-checked={theme.language === item.value} disabled={!theme.loaded || theme.languageSaving} className={cn("rounded-xl border p-4 text-left outline-none transition-colors hover:bg-accent focus-visible:ring-2 focus-visible:ring-ring", theme.language === item.value && "border-primary bg-primary/5 ring-1 ring-primary")} onClick={() => void theme.chooseLanguage(item.value)}>
          <strong className="block text-sm">{item.label}</strong><span className="mt-1 block text-xs text-muted-foreground">{item.description}</span>
        </button>)}
      </div>
    </div>
  </section>;
}
