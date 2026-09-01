import { useState } from "react";
import { ArrowLeft02Icon, GithubIcon } from "@hugeicons/core-free-icons";

import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import type { GitHubIdentityLifecycleBinding } from "@/github/use-github-identity-lifecycle";
import type { ApplicationSettingsApplication } from "@/settings/application-settings";
import type { SettingsSection } from "@/settings/settings-section";
import { useApplicationSettings, type ApplicationSettingsBinding } from "@/settings/use-application-settings";

import { AppearanceSettings } from "./appearance-settings";
import { BabysitProviderSettings } from "./babysit/babysit-provider-settings";
import { GitHubSettings } from "./github-settings";

const sections: Array<{ id: SettingsSection; label: string }> = [
  { id: "appearance", label: "外观" },
  { id: "github", label: "GitHub" },
  { id: "babysit", label: "宝宝模式" },
];

/** Application Settings 页面稳定 interface。 */
export type SettingsPageProps = {
  settings: ApplicationSettingsBinding;
  application: ApplicationSettingsApplication;
  githubIdentity?: GitHubIdentityLifecycleBinding;
  initialSection?: SettingsSection;
  initialGitHubSignOutConfirmation?: boolean;
  initialCustomApiKeyVisible?: boolean;
  autoSaveCustomConnection?: boolean;
  onBack(): void;
};

/** 独立 Application Settings 目的地，负责页面布局和分区编排。 */
export function SettingsPage({
  settings,
  application,
  githubIdentity,
  initialSection = "appearance",
  initialGitHubSignOutConfirmation = false,
  initialCustomApiKeyVisible = false,
  autoSaveCustomConnection = false,
  onBack,
}: SettingsPageProps) {
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
        {section === "appearance" && <AppearanceSettings settings={settings} />}
        {section === "github" && (githubIdentity
          ? <GitHubSettings identity={githubIdentity} initiallyConfirming={initialGitHubSignOutConfirmation} />
          : <section><div className="flex items-center gap-3"><Icon icon={GithubIcon} className="size-5" /><h2 className="font-heading text-xl font-semibold">GitHub</h2></div><p className="mt-4 text-sm text-muted-foreground">GitHub Identity 设置将在后续版本中提供。</p></section>)}
        {section === "babysit" && <BabysitProviderSettings application={application} initialCustomApiKeyVisible={initialCustomApiKeyVisible} autoSaveCustomConnection={autoSaveCustomConnection} />}
      </div>
    </div>
  </section>;
}

/** Preview 与测试从公共 Application Settings port 启动完整设置目的地。 */
export type ApplicationSettingsDestinationProps = Omit<SettingsPageProps, "settings">;

/** 从 Application Settings port 构造应用设置 binding，再呈现设置页。 */
export function ApplicationSettingsDestination(props: ApplicationSettingsDestinationProps) {
  const settings = useApplicationSettings(props.application);
  return <SettingsPage settings={settings} {...props} />;
}
