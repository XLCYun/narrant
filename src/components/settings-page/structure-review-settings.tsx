import { useEffect, useRef, useState } from "react";
import { Settings02Icon } from "@hugeicons/core-free-icons";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Icon } from "@/components/ui/icon";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { cn } from "@/lib/utils";
import type { ApplicationSettingsApplication, RustDependencyScope } from "@/settings/application-settings";

const scopes: RustDependencyScope[] = ["repository-only", "standard-library", "all-dependencies"];
const languages = [{ value: "rust", label: "Rust" }];

/** 通过设备设置端口编排按语言选择、即时保存与失败恢复。 */
export function StructureReviewSettings({ application }: { application: ApplicationSettingsApplication }) {
  const { t } = useTranslation(["structure", "common"]);
  const [language, setLanguage] = useState("rust");
  const [scope, setScope] = useState<RustDependencyScope>();
  const [error, setError] = useState<string>();
  const [saving, setSaving] = useState(false);
  const savingRef = useRef(false);
  const generation = useRef(0);
  const [reload, setReload] = useState(0);

  useEffect(() => {
    const current = ++generation.current;
    setScope(undefined);
    setError(undefined);
    setSaving(false);
    savingRef.current = false;
    void application.load().then((settings) => {
      if (current === generation.current) setScope(settings.structureReview.rust.dependencyScope);
    }).catch(() => {
      if (current === generation.current) setError(t("settingsLoadFailed"));
    });
    return () => { generation.current += 1; };
  }, [application, reload, t]);

  /** 保存成功后才改变选中项；失败保留旧值并允许再次提交。 */
  async function chooseScope(next: RustDependencyScope) {
    if (!scope || next === scope || savingRef.current) return;
    const current = generation.current;
    savingRef.current = true;
    setSaving(true);
    setError(undefined);
    try {
      const saved = await application.updateRustDependencyScope(next);
      if (current === generation.current) setScope(saved.structureReview.rust.dependencyScope);
    } catch {
      if (current === generation.current) setError(t("settingsSaveFailed"));
    } finally {
      if (current === generation.current) {
        savingRef.current = false;
        setSaving(false);
      }
    }
  }

  const disabled = !scope || saving;
  return <section aria-labelledby="structure-settings-title">
    <div className="flex items-center gap-3"><Icon icon={Settings02Icon} className="size-5" /><h2 id="structure-settings-title" className="font-heading text-xl font-semibold">{t("title")}</h2></div>
    <p className="mt-1 text-sm text-muted-foreground">{t("settingsDescription")}</p>
    {error && <div role="alert" className="mt-4 rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">
      <p>{error}</p>
      {!scope && <Button variant="outline" size="sm" className="mt-2" onClick={() => setReload((value) => value + 1)}>{t("common:retry")}</Button>}
    </div>}
    <div className="mt-6 max-w-xs">
      <label id="structure-language-label" className="mb-2 block text-sm font-medium">{t("settingsLanguage")}</label>
      <Select items={languages} value={language} onValueChange={(value) => value && setLanguage(value)} disabled={saving}>
        <SelectTrigger aria-labelledby="structure-language-label" className="w-full"><SelectValue /></SelectTrigger>
        <SelectContent><SelectItem value="rust">Rust</SelectItem></SelectContent>
      </Select>
    </div>
    {language === "rust" && <div className="mt-6 border-t pt-6">
      <h3 id="rust-dependencies-title" className="font-heading text-lg font-semibold">{t("dependencyScope")}</h3>
      <p className="mt-1 text-sm text-muted-foreground">{t("dependencyScopeDescription")}</p>
      <RadioGroup value={scope ?? "repository-only"} onValueChange={(value) => void chooseScope(value as RustDependencyScope)}
        aria-labelledby="rust-dependencies-title" aria-busy={disabled || undefined} disabled={disabled} className="mt-4 gap-3">
        {scopes.map((value) => <label key={value} className={cn("relative block cursor-pointer rounded-xl border p-4 transition-colors hover:bg-accent has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring", scope === value && "border-primary bg-primary/5 ring-1 ring-primary", disabled && "pointer-events-none opacity-50")}>
          <RadioGroupItem value={value} aria-labelledby={`rust-${value}-label`} aria-describedby={`rust-${value}-description`} className="absolute top-4 right-4" />
          <span className="pointer-events-none block pr-7">
            <strong id={`rust-${value}-label`} className="block text-sm">{t(`scope.${value}.label`)}</strong>
            <span id={`rust-${value}-description`} className="mt-1 block text-sm leading-relaxed text-muted-foreground">{t(`scope.${value}.description`)}</span>
          </span>
        </label>)}
      </RadioGroup>
      <p role="status" className="mt-4 text-sm text-muted-foreground">{t(saving ? "settingsSaving" : "settingsNextAnalysis")}</p>
    </div>}
  </section>;
}
