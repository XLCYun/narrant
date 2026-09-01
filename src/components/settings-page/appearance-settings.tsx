import { ComputerIcon, LanguageCircleIcon, Moon02Icon, Settings02Icon, Sun03Icon } from "@hugeicons/core-free-icons";

import { Icon } from "@/components/ui/icon";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { cn } from "@/lib/utils";
import type { ApplicationLanguage, ThemePreference } from "@/settings/application-settings";
import type { ApplicationSettingsBinding } from "@/settings/use-application-settings";

const themes: Array<{ value: ThemePreference; label: string; description: string; icon: typeof ComputerIcon }> = [
  { value: "system", label: "跟随系统", description: "随操作系统外观自动切换", icon: ComputerIcon },
  { value: "light", label: "浅色", description: "始终使用明亮外观", icon: Sun03Icon },
  { value: "dark", label: "深色", description: "始终使用深色外观", icon: Moon02Icon },
];

const languages: Array<{ value: ApplicationLanguage; label: string; description: string }> = [
  { value: "zh-CN", label: "简体中文", description: "使用简体中文显示应用界面" },
  { value: "en-US", label: "English", description: "Use English for the application interface" },
];

/** 外观设置消费完整应用设置 binding，并呈现主题与语言选择。 */
export function AppearanceSettings({ settings }: { settings: ApplicationSettingsBinding }) {
  return <section aria-labelledby="appearance-title">
    <div className="flex items-center gap-3"><Icon icon={Settings02Icon} className="size-5" /><h2 id="appearance-title" className="font-heading text-xl font-semibold">外观</h2></div>
    <p className="mt-1 text-sm text-muted-foreground">主题会同时应用到应用框架、代码阅读器与品牌图像。</p>
    {settings.error && <p role="alert" className="mt-4 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">{settings.error}</p>}
    <RadioGroup
      value={settings.preference}
      onValueChange={(value) => void settings.choose(value as ThemePreference)}
      disabled={!settings.loaded || settings.saving}
      aria-label="主题"
      aria-busy={!settings.loaded || undefined}
      className="mt-6 grid gap-3 sm:grid-cols-3"
    >
      {themes.map((item) => <label key={item.value} className={cn("relative block cursor-pointer rounded-xl border p-4 text-left outline-none transition-colors hover:bg-accent has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring", settings.preference === item.value && "border-primary bg-primary/5 ring-1 ring-primary", (!settings.loaded || settings.saving) && "pointer-events-none opacity-50")}>
        <RadioGroupItem value={item.value} aria-labelledby={`theme-${item.value}-label`} aria-describedby={`theme-${item.value}-description`} className="absolute top-4 right-4" />
        <span className="pointer-events-none block pr-7"><Icon icon={item.icon} className="mb-3 size-5" /><strong id={`theme-${item.value}-label`} className="block text-sm">{item.label}</strong><span id={`theme-${item.value}-description`} className="mt-1 block text-xs text-muted-foreground">{item.description}</span></span>
      </label>)}
    </RadioGroup>
    <div className="mt-8 border-t pt-6">
      <div className="flex items-center gap-3"><Icon icon={LanguageCircleIcon} className="size-5" /><h3 id="language-title" className="font-heading text-lg font-semibold">语言</h3></div>
      <p className="mt-1 text-sm text-muted-foreground">选择应用界面使用的语言。</p>
      <RadioGroup
        value={settings.language}
        onValueChange={(value) => void settings.chooseLanguage(value as ApplicationLanguage)}
        disabled={!settings.loaded || settings.languageSaving}
        aria-label="语言"
        aria-busy={!settings.loaded || undefined}
        className="mt-4 grid gap-3 sm:grid-cols-2"
      >
        {languages.map((item) => <label key={item.value} className={cn("relative block cursor-pointer rounded-xl border p-4 text-left outline-none transition-colors hover:bg-accent has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring", settings.language === item.value && "border-primary bg-primary/5 ring-1 ring-primary", (!settings.loaded || settings.languageSaving) && "pointer-events-none opacity-50")}>
          <RadioGroupItem value={item.value} aria-labelledby={`language-${item.value}-label`} aria-describedby={`language-${item.value}-description`} className="absolute top-4 right-4" />
          <span className="pointer-events-none block pr-7"><strong id={`language-${item.value}-label`} className="block text-sm">{item.label}</strong><span id={`language-${item.value}-description`} className="mt-1 block text-xs text-muted-foreground">{item.description}</span></span>
        </label>)}
      </RadioGroup>
    </div>
  </section>;
}
